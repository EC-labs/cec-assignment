use async_broadcast::broadcast;
use tracing::{span, Instrument, Level};
use clap::ArgMatches;
use futures::future;
use rand::Rng;
use std::{fs, sync::Arc};
use tokio::{
    sync::{mpsc::Receiver, RwLock},
    time::{self, Duration},
};

use crate::experiment::ExperimentDocument;
use crate::generator::{self, APIQuery};
use crate::metric::{MetricServer, Metrics};
use crate::request::{Host, Requestor, RequestorConfiguration};

pub struct BatchSize {
    max: u16,
    min: u16,
}

impl BatchSize {
    pub fn new(min: u16, max: u16) -> Option<Self> {
        if min > max {
            None
        } else {
            Some(Self { min, max })
        }
    }
}

pub struct ExperimentReceiverConfig {
    hosts: Vec<Host>,
    requestor_config: RequestorConfiguration,
    batch_size: BatchSize,
    stable_rate: u16,
    num_generations: u8,
}

impl From<&mut ArgMatches> for ExperimentReceiverConfig {
    fn from(args: &mut ArgMatches) -> Self {
        let file = args.remove_one::<String>("hosts-file").expect("Required");
        let content =
            fs::read_to_string(&file).expect(format!("File `{}` should exist", file).as_str());
        let hosts: Vec<Host> =
            serde_json::from_str(content.as_str()).expect("Could not deserialize config file");
        let batch_size = BatchSize::new(
            args.remove_one::<u16>("min-batch-size").expect("Required"),
            args.remove_one::<u16>("max-batch-size").expect("Required"),
        )
        .expect("min-batch-size should be smaller than max-batch-size");
        let stable_rate = args
            .remove_one::<u16>("stable-rate-duration")
            .expect("Required");
        let num_generations = args.remove_one::<u8>("num-generations").expect("Required");
        let requestor_config = RequestorConfiguration::from(args);
        Self {
            requestor_config,
            hosts,
            batch_size,
            stable_rate,
            num_generations,
        }
    }
}

pub struct ExperimentReceiver {
    config: ExperimentReceiverConfig,
    experiment_rx: Receiver<ExperimentDocument>,
    experiments: Arc<RwLock<Vec<Arc<RwLock<ExperimentDocument>>>>>,
}

impl ExperimentReceiver {
    pub fn new(
        config: ExperimentReceiverConfig,
        experiment_rx: Receiver<ExperimentDocument>,
    ) -> Self {
        Self {
            config,
            experiment_rx,
            experiments: Arc::new(RwLock::new(vec![])),
        }
    }

    async fn add_first_experiment(&mut self) {
        let experiment = self.experiment_rx.recv().await.expect("Sender available");
        self.experiments
            .write()
            .await
            .push(Arc::new(RwLock::new(experiment)));
    }

    /// Spawn 1 thread per group
    fn create_requestors(
        &self,
        batch_rx: async_broadcast::Receiver<Arc<Vec<APIQuery>>>,
        metrics: Metrics,
    ) {
        for host in self.config.hosts.iter() {
            let batch_rx = batch_rx.clone();
            let mut requestor = Requestor::new(
                self.config.requestor_config.clone(),
                host.clone(),
                batch_rx,
                metrics.clone(),
            );

            let host_name = host.host_name.clone();
            tokio::spawn(async move {
                let span = span!(
                    Level::INFO,
                    "http-generator",
                    host=host_name,
                );
                requestor.start().instrument(span).await;
            });
        }
    }

    pub async fn start(mut self) {
        let (batch_tx, batch_rx) = broadcast::<Arc<Vec<APIQuery>>>(1000);

        // Create a sample counter metric family utilizing the above custom label
        // type, representing the number of HTTP requests received.
        let metrics = Metrics::new();
        let metric_server = MetricServer::new(metrics.clone());
        metric_server.start();

        self.add_first_experiment().await;

        self.create_requestors(batch_rx, metrics);

        // Each iteration receives messages and generates load for the next minute
        // Sleep for 60 seconds after generating all the requests, if there are new experiments, read
        // them and generate the load, otherwise just generate the load with the experiments available.
        for _ in 0..self.config.num_generations {
            self.receive_experiments().await;
            let mut handles = vec![];
            handles.push(tokio::spawn(async move {
                time::sleep(Duration::from_millis(self.config.stable_rate as u64 * 1000)).await;
            }));
            let batch_size = {
                let mut rng = rand::thread_rng();
                rng.gen_range(self.config.batch_size.min..self.config.batch_size.max)
            };
            handles.append(
                &mut (0..self.config.stable_rate)
                    .map(|_| {
                        let experiments = self.experiments.clone();
                        let batch_tx = batch_tx.clone();
                        tokio::spawn(async move {
                            generator::generate(experiments.clone(), batch_size, batch_tx).await
                        })
                    })
                    .collect(),
            );
            future::join_all(handles).await;
        }
    }

    async fn receive_experiments(&mut self) {
        let mut experiments = self.experiments.write().await;
        while let Ok(experiment) = self.experiment_rx.try_recv() {
            experiments.push(Arc::new(RwLock::new(experiment)));
        }
    }
}
