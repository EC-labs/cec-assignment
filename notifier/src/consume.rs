use apache_avro::{from_value, Reader};
use clap::ArgMatches;
use event_hash::{HashData, NotificationType};
use rand::Rng;
use rdkafka::{
    client::ClientContext,
    config::ClientConfig,
    consumer::stream_consumer::StreamConsumer,
    consumer::{CommitMode, Consumer, ConsumerContext, Rebalance},
    message::{Headers, Message},
};
use reqwest::Client;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tokio::time::{self, Duration};

#[derive(Deserialize, Debug)]
struct SensorTemperatureMeasured {
    experiment: String,
    sensor: String,
    measurement_id: String,
    timestamp: f64,
    temperature: f32,
    measurement_hash: String,
}

struct CustomContext;

impl ClientContext for CustomContext {}

impl ConsumerContext for CustomContext {
    fn post_rebalance(&self, rebalance: &Rebalance) {
        println!("Post rebalance {:?}", rebalance);
    }
}

pub struct ConsumeConfiguration {
    secret_key: String,
    group_id: String,
    brokers: String,
    topic: String,
    notifications_host: String,
    token: Arc<str>,
}

impl From<&mut ArgMatches> for ConsumeConfiguration {
    fn from(args: &mut ArgMatches) -> Self {
        let secret_key = args.remove_one::<String>("secret-key").expect("Required");
        let brokers = args.remove_one::<String>("broker-list").expect("Required");
        let group_id = args.remove_one::<String>("group-id").expect("Required");
        let topic = args.remove_one::<String>("topic").expect("Required");
        let notifications_host = args
            .remove_one::<String>("notifications-host")
            .expect("Required");
        let token = args
            .remove_one::<String>("token")
            .expect("Missing required arg");

        ConsumeConfiguration {
            secret_key,
            group_id,
            brokers,
            topic,
            notifications_host,
            token: token.into(),
        }
    }
}

pub struct Consume {
    config: ConsumeConfiguration,
    consumer: StreamConsumer<CustomContext>,
    client: Client,
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

        let client = Client::new();

        Self {
            config,
            consumer,
            client,
        }
    }

    pub async fn start(&self) {
        self.consumer
            .subscribe(&[self.config.topic.as_str()])
            .expect("Can't subscribe to specified topics");
        self.read_loop().await;
    }

    async fn read_loop(&self) {
        loop {
            match self.consumer.recv().await {
                Err(e) => println!("Kafka error: {}", e),
                Ok(b) => {
                    let m = b.detach();
                    let headers = m.headers();
                    if let None = headers {
                        continue;
                    }
                    let headers = headers.unwrap();
                    let record_name =
                        String::from_utf8(headers.get(0).unwrap().1.to_vec()).expect("Valid utf-8");
                    if record_name != "sensor_temperature_measured" {
                        continue;
                    }
                    let reader = Reader::new(m.payload().unwrap()).unwrap();
                    for value in reader {
                        let sensor_measurement: SensorTemperatureMeasured =
                            from_value::<SensorTemperatureMeasured>(&value.unwrap())
                                .expect("Received invalid event")
                                .into();

                        let hash_data = HashData::decrypt(
                            self.config.secret_key.as_bytes(),
                            &sensor_measurement.measurement_hash,
                        )
                        .expect("Valid measurement_hash");
                        if hash_data.notification_type.is_none() {
                            break;
                        }
                        let mut map = HashMap::new();
                        map.insert("researcher", hash_data.researcher);
                        map.insert("measurement_id", hash_data.measurement_id);
                        map.insert("experiment_id", hash_data.experiment_id);
                        map.insert("cipher_data", sensor_measurement.measurement_hash);

                        let token = self.config.token.clone();
                        match hash_data.notification_type {
                            Some(NotificationType::OutOfRange) => {
                                let client = self.client.clone();
                                let notifications_host = self.config.notifications_host.clone();
                                tokio::spawn(async move {
                                    let sleep_secs = {
                                        let mut rng = rand::thread_rng();
                                        rng.gen_range(0..5)
                                    };
                                    time::sleep(Duration::from_millis(sleep_secs * 1000)).await;
                                    map.insert("notification_type", "OutOfRange".into());
                                    client
                                        .post(format!(
                                            "http://{}:3000/api/notify",
                                            notifications_host
                                        ))
                                        .query(&[("token", token)])
                                        .json(&map)
                                        .send()
                                        .await
                                        .expect("Failed to notify");
                                    println!("Notify {:?}", map.get("measurement_id"));
                                });
                            }
                            Some(NotificationType::Stabilized) => {
                                let client = self.client.clone();
                                let notifications_host = self.config.notifications_host.clone();
                                tokio::spawn(async move {
                                    let sleep_secs = {
                                        let mut rng = rand::thread_rng();
                                        rng.gen_range(0..5)
                                    };
                                    time::sleep(Duration::from_millis(sleep_secs * 1000)).await;
                                    map.insert("notification_type", "Stabilized".into());
                                    client
                                        .post(format!(
                                            "http://{}:3000/api/notify",
                                            notifications_host
                                        ))
                                        .query(&[("token", token)])
                                        .json(&map)
                                        .send()
                                        .await
                                        .expect("Failed to notify");
                                    println!("Notify {:?}", map.get("measurement_id"));
                                });
                            }
                            _ => {}
                        }
                    }
                    self.consumer.commit_message(&b, CommitMode::Async).unwrap();
                }
            };
        }
    }
}
