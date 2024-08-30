use actix_web::{get, web::Data, App, HttpServer, Responder};
use prometheus_client::{
    encoding::{text, EncodeLabelSet},
    metrics::{counter::Counter, family::Family, gauge::Gauge},
    registry::Registry,
};
use std::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct EventCountLabels {
    pub key: Option<String>,
    pub topic: String,
}

#[derive(Clone)]
pub struct Metrics {
    pub event_count: Family<EventCountLabels, Counter>,
    pub experiment_gauge: Gauge,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            event_count: Family::<EventCountLabels, Counter>::default(),
            experiment_gauge: Gauge::default(),
        }
    }
}

pub struct MetricServer {
    registry: Registry,
}

impl MetricServer {
    pub fn new(metrics: Metrics) -> Self {
        let mut registry = <Registry>::default();
        registry.register(
            "experiment_producer_event_count",
            "Count of events produced",
            metrics.event_count.clone(),
        );
        registry.register(
            "experiment_producer_num_experiments",
            "Number of experiments running",
            metrics.experiment_gauge.clone(),
        );
        Self { registry }
    }

    pub fn start(self) -> JoinHandle<Result<(), std::io::Error>> {
        let state = Data::new(Mutex::new(self.registry));
        let server =
            HttpServer::new(move || App::new().service(get_metrics).app_data(state.clone()))
                .bind(("0.0.0.0", 3001))
                .unwrap()
                .run();
        tokio::spawn(server)
    }
}

#[get("/metrics")]
async fn get_metrics(state: Data<Mutex<Registry>>) -> impl Responder {
    let state = state.lock().unwrap();
    let mut body = String::new();
    text::encode(&mut body, &state).unwrap();
    body
}
