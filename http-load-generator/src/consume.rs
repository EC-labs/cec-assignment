use apache_avro::{from_value, Reader};
use clap::ArgMatches;
use rdkafka::{
    client::ClientContext,
    config::ClientConfig,
    consumer::stream_consumer::StreamConsumer,
    consumer::{CommitMode, Consumer, ConsumerContext, Rebalance},
    message::Message,
};
use tokio::{
    sync::mpsc::Sender,
    time::{self, Duration},
};
use tracing::{info, error};

use crate::experiment::{ExperimentDocument, ExperimentDocumentData};

struct CustomContext;

impl ClientContext for CustomContext {}

impl ConsumerContext for CustomContext {
    fn post_rebalance(&self, rebalance: &Rebalance) {
        info!("Post rebalance {:?}", rebalance);
    }
}

pub struct ConsumeConfiguration {
    wait_before_tx: u8,
    group_id: String,
    brokers: String,
    topic: String,
}

impl From<&mut ArgMatches> for ConsumeConfiguration {
    fn from(args: &mut ArgMatches) -> Self {
        let brokers = args.remove_one::<String>("broker-list").expect("Required");
        let group_id = args.remove_one::<String>("group-id").expect("Required");
        let topic = args.remove_one::<String>("topic").expect("Required");
        let wait_before_tx = args
            .remove_one::<u8>("consumer-wait-before-send")
            .expect("Required");

        ConsumeConfiguration {
            group_id,
            brokers,
            topic,
            wait_before_tx,
        }
    }
}

pub struct Consume {
    config: ConsumeConfiguration,
    consumer: StreamConsumer<CustomContext>,
}

impl Consume {
    pub fn new(config: ConsumeConfiguration) -> Self {
        let context = CustomContext;
        let consumer: StreamConsumer<CustomContext> = ClientConfig::new()
            .set("group.id", config.group_id.as_str())
            .set("bootstrap.servers", config.brokers.as_str())
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "6000")
            .set("enable.auto.commit", "true")
            .set("security.protocol", "SSL")
            .set("ssl.ca.location", "auth/ca.crt")
            .set("ssl.keystore.location", "auth/kafka.keystore.pkcs12")
            .set("ssl.keystore.password", "cc2023")
            .create_with_context(context)
            .expect("Consumer creation failed");
        Self { config, consumer }
    }

    pub async fn start(&self, tx: Sender<ExperimentDocument>) {
        self.consumer
            .subscribe(&[self.config.topic.as_str()])
            .expect("Can't subscribe to specified topics");
        self.read_loop(tx).await;
    }

    async fn read_loop(&self, tx: Sender<ExperimentDocument>) {
        loop {
            match self.consumer.recv().await {
                Err(e) => {
                    error!("kafka error: {}", e);
                },
                Ok(b) => {
                    let m = b.detach();
                    let reader = Reader::new(m.payload().unwrap()).unwrap();
                    for value in reader {
                        let mut experiment_document: ExperimentDocument =
                            from_value::<ExperimentDocumentData>(&value.unwrap())
                                .expect("Received invalid event")
                                .into();
                        experiment_document
                            .measurements
                            .sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let tx = tx.clone();
                        let wait_before_tx = self.config.wait_before_tx as u64;
                        tokio::spawn(async move {
                            let experiment_id = experiment_document.experiment.clone();
                            info!("received experiment {}; wait {} seconds", experiment_id, wait_before_tx);
                            time::sleep(Duration::from_millis(wait_before_tx * 1000)).await;
                            tx.send(experiment_document)
                                .await
                                .expect("Receiver available");
                            info!("experiment {} available", experiment_id);
                        });
                    }
                    self.consumer.commit_message(&b, CommitMode::Async).unwrap();
                }
            };
        }
    }
}
