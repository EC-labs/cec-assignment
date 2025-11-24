use async_broadcast::Receiver;
use clap::ArgMatches;
use futures::{stream, StreamExt};
use reqwest::{Client, Error, RequestBuilder, Response};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration as stdDuration, Instant},
};
use tokio::{
    sync::RwLock,
    time::{self, Duration},
};

use crate::metric::Metrics;
use crate::{
    experiment::{ExperimentDocument, Measurement},
    metric::{ResponseCountLabels, ResponseType},
};
use crate::{generator::APIQuery, metric::RequestRateLabels};

#[derive(Debug)]
pub enum ResponseError {
    ServerError,
    BodyDecodingError,
    DeserializationError,
    ValidationError,
}

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Server response error")
    }
}

impl std::error::Error for ResponseError {}

#[derive(Deserialize, Clone)]
pub struct Host {
    host_name: String,
    base_url: String,
}

#[derive(Clone)]
pub struct RequestorConfiguration {
    lag: u8,
    retries: u8,
    max_in_flight: u16,
}

impl From<&mut ArgMatches> for RequestorConfiguration {
    fn from(args: &mut ArgMatches) -> Self {
        Self {
            lag: args.remove_one::<u8>("requestor-lag").expect("Required"),
            retries: args
                .remove_one::<u8>("requestor-retries")
                .expect("Required"),
            max_in_flight: args
                .remove_one::<u16>("requestor-max-in-flight")
                .expect("Required"),
        }
    }
}

pub struct Requestor {
    host: Host,
    batch_rx: Receiver<Arc<Vec<APIQuery>>>,
    client: Client,
    metrics: Metrics,
    config: RequestorConfiguration,
}

impl Requestor {
    pub fn new(
        config: RequestorConfiguration,
        host: Host,
        batch_rx: Receiver<Arc<Vec<APIQuery>>>,
        metrics: Metrics,
    ) -> Self {
        Self {
            host,
            batch_rx,
            client: Client::new(),
            metrics,
            config,
        }
    }

    async fn prepare_temperature_request(
        &self,
        experiment: Arc<RwLock<ExperimentDocument>>,
        start_time: f64,
        end_time: f64,
    ) -> RequestBuilder {
        self.client
            .get(format!("{}/temperature", self.host.base_url))
            .query(&[
                ("experiment-id", experiment.read().await.experiment.as_str()),
                ("start-time", format!("{}", start_time).as_str()),
                ("end-time", format!("{}", end_time).as_str()),
            ])
    }

    async fn validate_temperature_response(
        &self,
        response: Result<Response, Error>,
        experiment: Arc<RwLock<ExperimentDocument>>,
        start_time: f64,
        end_time: f64,
    ) -> Result<(), ResponseError> {
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                return Err(ResponseError::ServerError);
            }
        };

        let url = response.url().clone();
        let content = match response.text().await {
            Ok(content) => content,
            Err(_) => return Err(ResponseError::BodyDecodingError),
        };

        let mut measurements: Vec<Measurement> = match serde_json::from_str(&content) {
            Ok(measurements) => measurements,
            Err(e) => {
                println!(
                    "Group: {}\tError: {:?}\nContent: {}",
                    self.host.host_name, e, content
                );
                return Err(ResponseError::DeserializationError);
            }
        };
        measurements.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let experiment_read = experiment.read().await;
        let measurements_cmp = experiment_read
            .get_measurements_slice(start_time, end_time)
            .expect("Valid start and end times");

        if measurements_cmp != measurements {
            println!(
                "Group: {}\nQuery: {:?}\nGround truth: {}\nReceived: {}",
                self.host.host_name,
                url,
                serde_json::to_string(&measurements_cmp).unwrap(),
                serde_json::to_string(&measurements).unwrap(),
            );
            Err(ResponseError::ValidationError)
        } else {
            Ok(())
        }
    }

    async fn make_temperature_request(
        &self,
        experiment: Arc<RwLock<ExperimentDocument>>,
        start_time: f64,
        end_time: f64,
    ) -> (Result<(), ResponseError>, stdDuration) {
        let request = self
            .prepare_temperature_request(experiment.clone(), start_time, end_time)
            .await;
        let start = Instant::now();
        let response = tokio::spawn(async move { request.send().await })
            .await
            .expect("Join should not fail");
        let duration = start.elapsed();

        (
            self.validate_temperature_response(response, experiment, start_time, end_time)
                .await,
            duration,
        )
    }

    async fn prepare_out_of_bounds_request(
        &self,
        experiment: Arc<RwLock<ExperimentDocument>>,
    ) -> RequestBuilder {
        self.client
            .get(format!("{}/temperature/out-of-range", self.host.base_url))
            .query(&[("experiment-id", experiment.read().await.experiment.as_str())])
    }

    async fn validate_out_of_bounds_response(
        &self,
        response: Result<Response, Error>,
        experiment: Arc<RwLock<ExperimentDocument>>,
    ) -> Result<(), ResponseError> {
        let response = match response {
            Ok(response) => response,
            Err(_) => return Err(ResponseError::ServerError),
        };

        let url = response.url().clone();
        let content = match response.text().await {
            Ok(content) => content,
            Err(_) => return Err(ResponseError::BodyDecodingError),
        };

        let mut measurements: Vec<Measurement> = match serde_json::from_str(&content) {
            Ok(measurements) => measurements,
            Err(e) => {
                println!(
                    "Group: {}\tError: {:?}\nContent: {}",
                    self.host.host_name, e, content
                );
                return Err(ResponseError::DeserializationError);
            }
        };
        measurements.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let is_none = experiment.read().await.get_cached_out_of_bounds().is_none();
        if is_none {
            let mut experiment = experiment.write().await;
            if experiment.get_cached_out_of_bounds().is_none() {
                let measurements = experiment.compute_out_of_bounds();
                experiment.set_out_of_bounds(measurements);
            }
        }
        let experiment_read = experiment.read().await;
        let measurements_cmp = experiment_read.get_cached_out_of_bounds().unwrap();
        if measurements_cmp != measurements {
            println!(
                "Group: {}\nQuery: {:?}\nGround truth: {}\nReceived: {}",
                self.host.host_name,
                url,
                serde_json::to_string(&measurements_cmp).unwrap(),
                serde_json::to_string(&measurements).unwrap()
            );
            Err(ResponseError::ValidationError)
        } else {
            Ok(())
        }
    }

    async fn make_out_of_bounds_request(
        &self,
        experiment: Arc<RwLock<ExperimentDocument>>,
    ) -> (Result<(), ResponseError>, stdDuration) {
        let request = self.prepare_out_of_bounds_request(experiment.clone()).await;
        let start = Instant::now();
        let response = tokio::spawn(async move { request.send().await })
            .await
            .expect("Join should not fail");
        let duration = start.elapsed();

        (
            self.validate_out_of_bounds_response(response, experiment)
                .await,
            duration,
        )
    }

    fn update_counters(
        &self,
        response_type: &Result<(), ResponseError>,
        rtt: &stdDuration,
        endpoint: &str,
    ) {
        self.metrics
            .response_count
            .get_or_create(&ResponseCountLabels {
                host_name: self.host.host_name.clone(),
                endpoint: endpoint.to_string(),
                response_type: ResponseType::from(response_type),
            })
            .inc();
        if let Ok(()) = response_type {
            self.metrics
                .response_time_histogram
                .get_or_create(&ResponseCountLabels {
                    host_name: self.host.host_name.clone(),
                    endpoint: endpoint.to_string(),
                    response_type: ResponseType::from(response_type),
                })
                .observe(rtt.as_millis() as f64 / 1000.0);
        }
    }

    fn update_gauge_target(&self, value: i64) {
        self.metrics
            .target_request_rate
            .get_or_create(&RequestRateLabels {
                host_name: self.host.host_name.clone(),
            })
            .set(value);
    }

    fn update_gauge_effective(&self, value: i64) {
        self.metrics
            .effective_request_rate
            .get_or_create(&RequestRateLabels {
                host_name: self.host.host_name.clone(),
            })
            .set(value);
    }

    async fn process_batch(&mut self, batch: Arc<Vec<APIQuery>>) {
        self.update_gauge_target(batch.len() as i64);
        let start = Instant::now();
        let sleep_handle = tokio::spawn(time::sleep(Duration::from_millis(1000)));
        stream::iter(batch.iter())
            .map(|query| async {
                let mut endpoint = "".to_string();
                for _ in 0..=self.config.retries {
                    let query = query.clone();
                    match query {
                        APIQuery::Temperature {
                            experiment,
                            start_time,
                            end_time,
                        } => {
                            endpoint = "/temperature".to_string();
                            let (response_type, rtt) = self
                                .make_temperature_request(experiment.clone(), start_time, end_time)
                                .await;
                            if let Err(ResponseError::ServerError) = response_type {
                                continue;
                            } else {
                                return (response_type, rtt, endpoint);
                            }
                        }
                        APIQuery::OutOfBounds { experiment } => {
                            endpoint = "/temperature/out-of-bounds".to_string();
                            let (response_type, rtt) =
                                self.make_out_of_bounds_request(experiment.clone()).await;
                            if let Err(ResponseError::ServerError) = response_type {
                                continue;
                            } else {
                                return (response_type, rtt, endpoint);
                            }
                        }
                    }
                }
                return (
                    Err(ResponseError::ServerError),
                    Duration::new(0, 0),
                    endpoint,
                );
            })
            .boxed()
            .buffer_unordered(self.config.max_in_flight as usize)
            .for_each(async |response| {
                match response {
                    (Err(response_type), rtt, endpoint) => {
                        self.update_counters(&Err(response_type), &rtt, &endpoint)
                    }
                    (Ok(_), rtt, endpoint) => {
                        self.update_counters(&Ok(()), &rtt, &endpoint);
                    }
                }
            })
            .await;

        sleep_handle.await.expect("Should not fail");
        let duration = start.elapsed();
        self.update_gauge_effective(
            ((batch.len() as f64) / (duration.as_millis() as f64 / 1000.0)).round() as i64,
        );
        println!("Performed {} requests", batch.len());
    }

    pub async fn start(&mut self) {
        time::sleep(Duration::from_millis(self.config.lag as u64 * 1000)).await;
        while let Ok(batch) = self.batch_rx.recv().await {
            self.process_batch(batch).await;
        }
    }
}
