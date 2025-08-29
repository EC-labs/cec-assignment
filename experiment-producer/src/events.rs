use apache_avro::types::{Record, Value};
use apache_avro::{Reader, Schema, Writer};
use rdkafka::{
    config::ClientConfig,
    error::KafkaError,
    message::{OwnedHeaders, OwnedMessage, ToBytes},
    producer::{FutureProducer, FutureRecord},
};
use std::collections::HashMap;
use std::{fs, time::Duration};
use tracing::{trace, debug, info, span, Level, Span};
use uuid::Uuid;

use event_hash::{HashData, NotificationType};

use crate::metric::{EventCountLabels, Metrics};
use crate::simulator::{self, ExperimentStage, IterMut, Measurement, TempRange, TemperatureSample};
use crate::time;

/// `Vec<u8>` wrapper
///
/// FutureRecord::payload requires a type that implements the trait `ToBytes` as an argument. This is our
/// custom type to implement the trait.
pub struct EventWrapper(Vec<u8>);

impl ToBytes for EventWrapper {
    fn to_bytes(&self) -> &[u8] {
        &self.0
    }
}

pub struct ExperimentSchemas {
    schemas: HashMap<&'static str, Schema>,
    raw_schema: HashMap<&'static str, String>,
}

impl ExperimentSchemas {
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
            raw_schema: HashMap::new(),
        }
    }

    pub fn experiment_configured_event(
        &mut self,
        experiment_id: &str,
        researcher: &str,
        sensors: &[String],
        temp_range: TempRange,
    ) -> EventWrapper {
        let schema = self
            .schemas
            .entry("experiment-producer/schemas/experiment_configured_event.avsc")
            .or_insert_with(|| {
                let raw_schema = self
                    .raw_schema
                    .entry("experiment-producer/schemas/experiment_configured.avsc")
                    .or_insert_with(|| {
                        fs::read_to_string("experiment-producer/schemas/experiment_configured.avsc")
                            .unwrap()
                    });
                Schema::parse_str(raw_schema).unwrap()
            });
        let mut writer = Writer::new(schema, Vec::new());

        let mut record = Record::new(writer.schema()).unwrap();
        record.put("experiment", experiment_id);
        record.put("researcher", researcher);
        let sensors = Value::Array(sensors.iter().map(|v| (&**v).into()).collect());
        record.put("sensors", sensors);

        let schema_json: serde_json::Value = serde_json::from_str(
            self.raw_schema
                .get("experiment-producer/schemas/experiment_configured.avsc")
                .unwrap(),
        )
        .unwrap();
        let temp_schema_json = &schema_json["fields"][3]["type"];

        let temp_schema = Schema::parse_str(&temp_schema_json.to_string()).unwrap();
        let mut record_temp_range = Record::new(&temp_schema).unwrap();
        record_temp_range.put("upper_threshold", Value::Float(temp_range.upper_threshold));
        record_temp_range.put("lower_threshold", Value::Float(temp_range.lower_threshold));
        record.put("temperature_range", record_temp_range);
        writer.append(record).unwrap();

        EventWrapper(writer.into_inner().unwrap())
    }

    pub fn stabilization_started_event(&mut self, experiment_id: &str) -> EventWrapper {
        let schema = self
            .schemas
            .entry("experiment-producer/schemas/stabilization_started.avsc")
            .or_insert_with(|| {
                let raw_schema =
                    fs::read_to_string("experiment-producer/schemas/stabilization_started.avsc")
                        .unwrap();
                Schema::parse_str(&raw_schema).unwrap()
            });
        let mut writer = Writer::new(schema, Vec::new());

        let mut record = Record::new(writer.schema()).unwrap();
        record.put("experiment", experiment_id);

        let current_time = time::current_epoch();
        record.put("timestamp", Value::Double(current_time));
        writer.append(record).unwrap();

        EventWrapper(writer.into_inner().unwrap())
    }

    pub fn experiment_started_event(&mut self, experiment_id: &str) -> EventWrapper {
        let schema = self
            .schemas
            .entry("experiment-producer/schemas/experiment_started.avsc")
            .or_insert_with(|| {
                let raw_schema =
                    fs::read_to_string("experiment-producer/schemas/experiment_started.avsc")
                        .unwrap();
                Schema::parse_str(&raw_schema).unwrap()
            });
        let mut writer = Writer::new(schema, Vec::new());

        let mut record = Record::new(writer.schema()).unwrap();
        record.put("experiment", experiment_id);

        let current_time = time::current_epoch();
        record.put("timestamp", Value::Double(current_time));
        writer.append(record).unwrap();

        EventWrapper(writer.into_inner().unwrap())
    }

    pub fn experiment_terminated_event(&mut self, experiment_id: &str) -> EventWrapper {
        let schema = self
            .schemas
            .entry("experiment-producer/schemas/experiment_started.avsc")
            .or_insert_with(|| {
                let raw_schema =
                    fs::read_to_string("experiment-producer/schemas/experiment_started.avsc")
                        .unwrap();
                Schema::parse_str(&raw_schema).unwrap()
            });
        let mut writer = Writer::new(schema, Vec::new());

        let mut record = Record::new(writer.schema()).unwrap();
        record.put("experiment", experiment_id);

        let current_time = time::current_epoch();
        record.put("timestamp", Value::Double(current_time));

        writer.append(record).unwrap();
        EventWrapper(writer.into_inner().unwrap())
    }

    pub fn temperature_measured_event(
        &mut self,
        experiment: &str,
        measurement_id: &str,
        sensor: &str,
        temperature: f32,
        timestamp: f64,
        measurement_hash: &str,
    ) -> EventWrapper {
        let schema = self
            .schemas
            .entry("experiment-producer/schemas/sensor_temperature_measured.avsc")
            .or_insert_with(|| {
                let raw_schema = fs::read_to_string(
                    "experiment-producer/schemas/sensor_temperature_measured.avsc",
                )
                .unwrap();
                Schema::parse_str(&raw_schema).unwrap()
            });
        let mut writer = Writer::new(schema, Vec::new());

        let mut record = Record::new(writer.schema()).unwrap();
        record.put("experiment", experiment);
        record.put("sensor", sensor);
        record.put("measurement_id", measurement_id);
        record.put("temperature", temperature);
        record.put("measurement_hash", measurement_hash);
        record.put("timestamp", Value::Double(timestamp));

        writer.append(record).unwrap();
        let encoded = writer.into_inner().unwrap();
        EventWrapper(encoded)
    }

    pub fn experiment_document_event(
        &mut self,
        experiment_id: &str,
        measurements: &[Measurement],
        temp_range: TempRange,
    ) -> EventWrapper {
        let schema = self
            .schemas
            .entry("experiment-producer/schemas/experiment_document.avsc")
            .or_insert_with(|| {
                let raw_schema =
                    fs::read_to_string("experiment-producer/schemas/experiment_document.avsc")
                        .unwrap();
                Schema::parse_str(&raw_schema).unwrap()
            });
        let mut writer = Writer::new(schema, Vec::new());

        let schema_json: serde_json::Value =
            serde_json::from_str(&schema.canonical_form()).unwrap();
        let measurement_schema = &schema_json["fields"][1]["type"]["items"];
        let measurement_schema = Schema::parse_str(&measurement_schema.to_string())
            .expect("Valid measurement avro schema");
        let temp_schema = &schema_json["fields"][2]["type"];
        let temp_schema = Schema::parse_str(&temp_schema.to_string()).unwrap();

        let mut record = Record::new(writer.schema()).unwrap();
        record.put("experiment", experiment_id);
        let measurements = Value::Array(
            measurements
                .iter()
                .map(|measurement| {
                    let mut record =
                        Record::new(&measurement_schema).expect("Valid measurement schema");
                    record.put("timestamp", Value::Double(measurement.timestamp));
                    record.put("temperature", Value::Float(measurement.temperature));
                    record.into()
                })
                .collect(),
        );
        record.put("measurements", measurements);

        let mut record_temp_range = Record::new(&temp_schema).unwrap();
        record_temp_range.put("upper_threshold", Value::Float(temp_range.upper_threshold));
        record_temp_range.put("lower_threshold", Value::Float(temp_range.lower_threshold));
        record.put("temperature_range", record_temp_range);

        writer.append(record).unwrap();
        EventWrapper(writer.into_inner().unwrap())
    }
}

fn compute_notification_type(
    curr_sample: TemperatureSample,
    prev_sample: Option<TemperatureSample>,
    stage: &ExperimentStage,
) -> Option<NotificationType> {
    match (stage, prev_sample) {
        (ExperimentStage::Stabilization, Some(prev_sample)) => {
            if !curr_sample.is_out_of_range() && prev_sample.is_out_of_range() {
                debug!(range_event = "Stabilized");
                Some(NotificationType::Stabilized)
            } else {
                None
            }
        }
        (ExperimentStage::CarryOut, Some(prev_sample)) => {
            if curr_sample.is_out_of_range() && !prev_sample.is_out_of_range() {
                debug!(range_event = "OutOfRange");
                Some(NotificationType::OutOfRange)
            } else {
                None
            }
        }
        (ExperimentStage::Stabilization, None) => {
            if !curr_sample.is_out_of_range() {
                debug!(range_event = "Stabilized");
                Some(NotificationType::Stabilized)
            } else {
                None
            }
        }
        (ExperimentStage::CarryOut, None) => {
            if curr_sample.is_out_of_range() {
                debug!(range_event = "OutOfRange");
                Some(NotificationType::OutOfRange)
            } else {
                None
            }
        }
        (_, _) => None,
    }
}

pub fn temperature_events<'b>(
    experiment_schemas: &'b mut ExperimentSchemas,
    sample_iter: IterMut<'b>,
    experiment_id: &'b str,
    researcher: &'b str,
    sensors: &'b [String],
    stage: &'b ExperimentStage,
    secret_key: &'b str,
) -> Box<dyn Iterator<Item = (Vec<EventWrapper>, Span, Measurement)> + 'b + Send> {
    let mut prev_sample = None;

    Box::new(sample_iter.map(move |sample| {
        let measurement_id = format!("{}", Uuid::new_v4());
        let span = span!(tracing::Level::INFO, "measurement", measurement_id);
        let _enter = span.enter();
        debug!(avg_temperature = sample.cur);
        let current_time = time::current_epoch();

        let notification_type = compute_notification_type(sample, prev_sample, stage);
        let hash_data = HashData {
            notification_type: notification_type.clone(),
            timestamp: current_time,
            experiment_id: experiment_id.into(),
            measurement_id: measurement_id.clone(),
            researcher: researcher.into(),
        };
        let measurement = Measurement {
            measurement_id: measurement_id.clone(),
            temperature: sample.cur(),
            timestamp: current_time,
            notification_type,
        };
        let measurement_hash = hash_data.encrypt(secret_key.as_bytes());
        prev_sample = Some(sample);

        let sensor_events = simulator::compute_sensor_temperatures(sensors, sample.cur())
            .into_iter()
            .map(|(sensor_id, sensor_temperature)| {
                experiment_schemas.temperature_measured_event(
                    experiment_id,
                    measurement_id.as_str(),
                    sensor_id,
                    sensor_temperature,
                    current_time,
                    &measurement_hash,
                )
            })
            .collect();
        drop(_enter);
        (sensor_events, span, measurement)
    }))
}

pub struct RecordData<K: ToBytes, T: ToBytes> {
    pub payload: T,
    pub key: Option<K>,
    pub headers: OwnedHeaders,
}

#[derive(Clone)]
pub struct KafkaTopicProducer {
    producer: FutureProducer, // partition: Option<usize>
    metrics: Metrics,
}

impl KafkaTopicProducer {
    pub fn new(brokers: &str, metrics: Metrics, use_ssl: bool) -> Self {
        let mut client_config = ClientConfig::new();
        client_config
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .set("linger.ms", "100")
            .set("queue.buffering.max.kbytes", "8388608")
            .set("queue.buffering.max.messages", "1000000");

        if use_ssl {
            info!("Client configured with SSL");
            client_config
                .set("security.protocol", "SSL")
                .set("ssl.ca.location", "experiment-producer/auth/ca.crt")
                .set(
                    "ssl.keystore.location",
                    "experiment-producer/auth/kafka.keystore.pkcs12",
                )
                .set("ssl.keystore.password", "cc2023");
        }

        let producer: FutureProducer = client_config.create().expect("Producer creation error");

        // For some reason this is required so the first level
        // span is printed to stdout. This happens because of the
        // call to ClientConfig::new()
        span!(Level::INFO, "");

        KafkaTopicProducer { producer, metrics }
    }

    fn update_count<K>(&self, topic: &str, key: Option<&K>)
    where
        K: ToBytes,
    {
        self.metrics
            .event_count
            .get_or_create(&EventCountLabels {
                key: key.map(|key| {
                    String::from_utf8(key.to_bytes().to_vec()).expect("Key should be utf-8 encoded")
                }),
                topic: topic.to_string(),
            })
            .inc();
    }

    pub async fn send_event<'a, K, T>(
        &self,
        record: RecordData<K, T>,
        topic: &str,
    ) -> Result<(i32, i64), (KafkaError, OwnedMessage)>
    where
        T: ToBytes,
        K: ToBytes + std::fmt::Debug,
    {
        let mut future_record: FutureRecord<'_, K, T> = FutureRecord::to(topic)
            .payload(&record.payload)
            .headers(record.headers);

        trace!(
            topic,
            key = format!("{:?}", record.key),
            record = format!(
                "{:?}",
                Reader::new(record.payload.to_bytes())
                    .unwrap()
                    .map(|value| value.unwrap())
                    .collect::<Vec<apache_avro::types::Value>>()
            )
        );

        if record.key.is_some() {
            future_record = future_record.key(record.key.as_ref().unwrap());
        }
        self.update_count(topic, record.key.as_ref());

        self.producer
            .send(future_record, Duration::from_secs(0))
            .await
    }
}
