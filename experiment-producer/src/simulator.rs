use futures::future;
use rand::Rng;
use rdkafka::message::OwnedHeaders;
use serde::Deserialize;
use sqlx::{Pool, Postgres};
use std::time::{Duration, Instant};
use tokio::{task::JoinHandle, time};
use tracing::{debug, info, Instrument, Span};
use uuid::Uuid;

use event_hash::NotificationType;

use crate::config::{ConfigEntry, UncheckedTempRange};
use crate::database;
use crate::events::{self, EventWrapper, ExperimentSchemas, KafkaTopicProducer, RecordData};
use crate::metric::Metrics;

#[derive(Clone, Copy)]
pub enum ExperimentStage {
    Uninitialized,
    Configuration,
    Stabilization,
    CarryOut,
    Terminated,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "UncheckedTempRange")]
pub struct TempRange {
    pub lower_threshold: f32,
    pub upper_threshold: f32,
}

impl TryFrom<UncheckedTempRange> for TempRange {
    type Error = String;

    fn try_from(unchecked_temp_range: UncheckedTempRange) -> Result<Self, Self::Error> {
        Self::new(
            unchecked_temp_range.lower_threshold,
            unchecked_temp_range.upper_threshold,
        )
        .ok_or(format!(
            "Invalid temperature range for experiment: {:?}",
            unchecked_temp_range
        ))
    }
}

impl TempRange {
    pub fn new(lower_threshold: f32, upper_threshold: f32) -> Option<Self> {
        if lower_threshold > upper_threshold {
            return None;
        }
        Some(Self {
            upper_threshold,
            lower_threshold,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TemperatureSample {
    cur: f32,
    temp_range: TempRange,
}

impl TemperatureSample {
    pub fn is_out_of_range(&self) -> bool {
        self.cur > self.temp_range.upper_threshold || self.cur < self.temp_range.lower_threshold
    }

    pub fn cur(&self) -> f32 {
        self.cur
    }

    pub fn iter_mut(&mut self, delta: f32, len: usize, random_range: f32) -> IterMut {
        IterMut {
            sample: self,
            iteration: 0,
            delta,
            len,
            random_range,
        }
    }

    pub fn stabilization_samples(&mut self, len: usize) -> IterMut {
        let TempRange {
            lower_threshold,
            upper_threshold,
        } = self.temp_range;
        let final_temperature = lower_threshold + (upper_threshold - lower_threshold) / 2_f32;
        let delta = (final_temperature - self.cur) / (len as f32);
        self.iter_mut(delta, len, 0.0)
    }

    pub fn carry_out_samples(&mut self, len: usize) -> IterMut {
        let TempRange {
            lower_threshold,
            upper_threshold,
        } = self.temp_range;
        self.iter_mut(0.0, len, upper_threshold - lower_threshold)
    }
}

#[derive(Clone, Debug)]
pub struct ExperimentConfiguration {
    pub experiment_id: String,
    researcher: String,
    sensors: Vec<String>,
    sample_rate: u64,
    temp_range: TempRange,
    stabilization_samples: u16,
    carry_out_samples: u16,
    secret_key: String,
    topic: String,
    topic_document: Option<String>,
}

impl ExperimentConfiguration {
    pub fn new(
        researcher: String,
        num_sensors: usize,
        sample_rate: u64,
        temp_range: TempRange,
        stabilization_samples: u16,
        carry_out_samples: u16,
        secret_key: String,
        topic: String,
        topic_document: Option<String>,
    ) -> Self {
        let sensors: Vec<_> = (0..num_sensors)
            .map(|_| format!("{}", Uuid::new_v4()))
            .collect();
        Self {
            experiment_id: format!("{}", Uuid::new_v4()),
            researcher,
            sensors,
            sample_rate,
            temp_range,
            stabilization_samples,
            carry_out_samples,
            secret_key,
            topic,
            topic_document,
        }
    }
}

impl From<ConfigEntry> for ExperimentConfiguration {
    fn from(config_entry: ConfigEntry) -> ExperimentConfiguration {
        let ConfigEntry {
            num_sensors,
            researcher,
            sample_rate,
            temp_range,
            stabilization_samples,
            carry_out_samples,
            start_time: _,
            secret_key,
            start_temperature: _,
            topic,
            topic_document,
        } = config_entry;
        Self::new(
            researcher,
            num_sensors,
            sample_rate,
            temp_range,
            stabilization_samples,
            carry_out_samples,
            secret_key,
            topic,
            topic_document,
        )
    }
}

pub struct Experiment {
    experiment_schemas: ExperimentSchemas,
    sample: TemperatureSample,
    measurements: Vec<Measurement>,
    stage: ExperimentStage,
    config: ExperimentConfiguration,
    producer: KafkaTopicProducer,
    pool: Option<Pool<Postgres>>,
    metrics: Metrics,
}

impl Experiment {
    pub fn new(
        start: f32,
        config: ExperimentConfiguration,
        producer: KafkaTopicProducer,
        pool: Option<Pool<Postgres>>,
        metrics: Metrics,
    ) -> Self {
        metrics.experiment_gauge.inc();
        let sample = TemperatureSample {
            cur: start,
            temp_range: config.temp_range,
        };
        Experiment {
            experiment_schemas: ExperimentSchemas::new(),
            stage: ExperimentStage::Uninitialized,
            measurements: Vec::new(),
            sample,
            producer,
            config,
            pool,
            metrics,
        }
    }

    async fn stage_configuration(&mut self) {
        self.stage = ExperimentStage::Configuration;
        let record = RecordData {
            payload: self.experiment_schemas.experiment_configured_event(
                &self.config.experiment_id,
                &self.config.researcher,
                &self.config.sensors,
                self.config.temp_range,
            ),
            key: Some(&self.config.experiment_id),
            headers: OwnedHeaders::new().add("record_name", "experiment_configured"),
        };
        self.producer
            .send_event(record, &self.config.topic)
            .await
            .expect("Failed to produce message");
    }

    async fn stage_stabilization(&mut self) {
        self.stage = ExperimentStage::Stabilization;
        let record = RecordData {
            payload: self
                .experiment_schemas
                .stabilization_started_event(&self.config.experiment_id),
            key: Some(&self.config.experiment_id),
            headers: OwnedHeaders::new().add("record_name", "stabilization_started"),
        };
        self.producer
            .send_event(record, &self.config.topic)
            .await
            .expect("Failed to produce message");

        // Stabilization Temperature Samples
        let stabilization_samples = self
            .sample
            .stabilization_samples(self.config.stabilization_samples.into());
        let stabilization_events = events::temperature_events(
            &mut self.experiment_schemas,
            stabilization_samples,
            &self.config.experiment_id,
            &self.config.researcher,
            &self.config.sensors,
            &self.stage,
            &self.config.secret_key,
        );

        for (sensor_events, _span, measurement) in stabilization_events {
            measurement
                .persist_sensor_events(
                    &self.producer,
                    self.pool.clone(),
                    &self.config.topic,
                    &self.config.experiment_id,
                    sensor_events,
                    self.config.sample_rate,
                )
                .await;
        }
    }

    async fn stage_carry_out(&mut self) {
        self.stage = ExperimentStage::CarryOut;
        let record = RecordData {
            payload: self
                .experiment_schemas
                .experiment_started_event(&self.config.experiment_id),
            key: Some(&self.config.experiment_id),
            headers: OwnedHeaders::new().add("record_name", "experiment_started"),
        };
        self.producer
            .send_event(record, &self.config.topic)
            .await
            .expect("Failed to produce message");

        let carry_out_samples = self
            .sample
            .carry_out_samples(self.config.carry_out_samples.into());
        let carry_out_events = events::temperature_events(
            &mut self.experiment_schemas,
            carry_out_samples,
            &self.config.experiment_id,
            &self.config.researcher,
            &self.config.sensors,
            &self.stage,
            &self.config.secret_key,
        );
        for (sensor_events, _span, measurement) in carry_out_events {
            measurement
                .persist_sensor_events(
                    &self.producer,
                    self.pool.clone(),
                    &self.config.topic,
                    &self.config.experiment_id,
                    sensor_events,
                    self.config.sample_rate,
                )
                .await;
            self.measurements.push(measurement);
        }

        self.stage = ExperimentStage::Terminated;
        let record = RecordData {
            payload: self
                .experiment_schemas
                .experiment_terminated_event(&self.config.experiment_id),
            key: Some(&self.config.experiment_id),
            headers: OwnedHeaders::new().add("record_name", "experiment_terminated"),
        };
        self.producer
            .send_event(record, &self.config.topic)
            .await
            .expect("Failed to produce message");

        if let Some(topic_document) = &self.config.topic_document {
            let record = RecordData {
                payload: self.experiment_schemas.experiment_document_event(
                    &self.config.experiment_id,
                    &self.measurements,
                    self.config.temp_range,
                ),
                headers: OwnedHeaders::new(),
                key: Some(&self.config.experiment_id),
            };
            self.producer
                .send_event(record, topic_document)
                .await
                .expect("Failed to produce message");
        }
    }

    pub async fn run(&mut self) {
        let start = Instant::now();
        info!(stage = "configuration");
        self.stage_configuration().await;
        info!(stage = "stabilization");
        let stabilization = Instant::now();
        self.stage_stabilization().await;
        info!(stage = "carry out");
        let carry_out = Instant::now();
        self.stage_carry_out().await;
        info!(
            stage = "terminated",
            elapsed = start.elapsed().as_millis(),
            stabilization = (carry_out - stabilization).as_millis(),
            carry_out = carry_out.elapsed().as_millis()
        );
    }
}

impl Drop for Experiment {
    fn drop(&mut self) {
        self.metrics.experiment_gauge.dec();
    }
}

#[derive(Debug)]
pub struct Measurement {
    pub measurement_id: String,
    pub timestamp: f64,
    pub temperature: f32,
    pub notification_type: Option<NotificationType>,
}

impl Measurement {
    pub async fn persist_sensor_events(
        &self,
        producer: &KafkaTopicProducer,
        pool: Option<Pool<Postgres>>,
        topic: &str,
        experiment_id: &str,
        sensor_events: Vec<EventWrapper>,
        period_millis: u64,
    ) {
        let sleep_handle = tokio::spawn(async move {
            time::sleep(Duration::from_millis(period_millis)).await;
        });
        if let (Some(pool), Some(_)) = (pool, &self.notification_type) {
            let experiment_id = experiment_id.to_string();
            let measurement_id = self.measurement_id.clone();

            tokio::spawn(async move {
                database::insert_ground_truth(
                    &pool,
                    experiment_id.as_str(),
                    measurement_id.as_str(),
                )
                .await
                .expect("Insert should not fail");
            });
        }
        let span = Span::current();
        let mut handles: Vec<JoinHandle<_>> = sensor_events
            .into_iter()
            .map(|event| {
                let record = RecordData {
                    payload: event,
                    key: Some(experiment_id.to_string()),
                    headers: OwnedHeaders::new().add("record_name", "sensor_temperature_measured"),
                };
                let producer = producer.clone();
                let topic = topic.to_string().clone();
                tokio::spawn(
                    async move {
                        producer
                            .send_event(record, &topic)
                            .await
                            .expect("Failed to produce message");
                    }
                    .instrument(span.clone()),
                )
            })
            .collect();
        handles.push(sleep_handle);
        future::join_all(handles).await;
    }
}

pub struct IterMut<'a> {
    sample: &'a mut TemperatureSample,
    delta: f32,
    len: usize,
    iteration: usize,
    random_range: f32,
}

impl<'a> Iterator for IterMut<'a> {
    type Item = TemperatureSample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.iteration >= self.len {
            return None;
        }

        self.sample.cur += self.delta;
        if self.random_range != 0.0 {
            let relative_val = rand::thread_rng().gen_range(-100.0..100.0);
            let absolute_val = relative_val * self.random_range / 100.0;
            self.sample.cur += absolute_val;
        }
        self.iteration += 1;
        let ret = if (self.sample.cur - self.sample.temp_range.upper_threshold).abs() <= 0.01 {
            let mut sample = *self.sample;
            sample.cur = self.sample.temp_range.upper_threshold - 0.011;
            sample
        } else if (self.sample.cur - self.sample.temp_range.lower_threshold).abs() <= 0.01 {
            let mut sample = *self.sample;
            sample.cur = self.sample.temp_range.lower_threshold + 0.011;
            sample
        } else {
            *self.sample
        };
        debug!(avg_temperature = ret.cur);
        Some(ret)
    }
}

pub fn compute_sensor_temperatures(
    sensors: &[String],
    average_temperature: f32,
) -> Vec<(&'_ str, f32)> {
    let mut cumulative_temperature = 0.0;
    let mut sensor_events = sensors[..sensors.len() - 1]
        .iter()
        .map(|sensor_id| {
            let relative_diff = rand::thread_rng().gen_range(-100.0..100.0);
            let sensor_temperature = average_temperature + relative_diff * 1.0 / 100.0;
            debug!(sensor = sensor_id, temperature = sensor_temperature);
            cumulative_temperature += sensor_temperature;
            (&**sensor_id, sensor_temperature)
        })
        .collect::<Vec<(&'_ str, f32)>>();
    let sensor_id = &sensors[sensors.len() - 1];
    let sensor_temperature = (sensors.len() as f32) * average_temperature - cumulative_temperature;
    debug!(sensor = sensor_id, temperature = sensor_temperature);
    sensor_events.push((sensor_id, sensor_temperature));
    sensor_events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_boundaries_precision() {
        let mut sample = TemperatureSample {
            cur: 9.0,
            temp_range: TempRange::new(10.0, 12.0).unwrap(),
        };
        let mut stabilization_iter = sample.stabilization_samples(2);
        let next_sample = stabilization_iter.next().unwrap();
        assert!((next_sample.cur() - next_sample.temp_range.lower_threshold).abs() > 0.01);
        let next_sample = stabilization_iter.next().unwrap();
        assert!(next_sample.cur == 11.0);

        let mut sample = TemperatureSample {
            cur: 13.0,
            temp_range: TempRange::new(10.0, 12.0).unwrap(),
        };
        let mut stabilization_iter = sample.stabilization_samples(2);
        let next_sample = stabilization_iter.next().unwrap();
        assert!((next_sample.cur() - next_sample.temp_range.upper_threshold).abs() > 0.01);
        let next_sample = stabilization_iter.next().unwrap();
        assert!(next_sample.cur == 11.0);
    }
}
