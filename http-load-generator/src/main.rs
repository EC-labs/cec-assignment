use time::{format_description, UtcOffset};
use clap::{command, value_parser, Arg, ArgAction};
use std::process;
use tokio::sync::mpsc;
use tracing::{Instrument, Level, span, info, error};
use tracing_subscriber::{filter::{LevelFilter, EnvFilter}, fmt::time::OffsetTime, prelude::*};

mod consume;
mod experiment;
mod generator;
mod metric;
mod receiver;
mod request;

use crate::consume::{Consume, ConsumeConfiguration};
use crate::receiver::{ExperimentReceiver, ExperimentReceiverConfig};

fn configure_tracing() {
    let mut layers = vec![];

    let offset = UtcOffset::from_hms(2, 0, 0).expect("Should get CET offset");
    let time_format = format_description::parse(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6][offset_hour sign:mandatory]",
    )
    .expect("format string should be valid");
    let timer = OffsetTime::new(offset, time_format);

    layers.push(
        tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_timer(timer)
            .with_filter(
                EnvFilter::builder()
                    .with_default_directive(LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .boxed(),
    );

    tracing_subscriber::registry().with(layers).init();
}

fn raise_fd_limit(soft: u64) -> Option<()> {
    if let Ok((_, hard)) = rlimit::Resource::NOFILE.get() {
        if soft > hard {
            error!("new soft limit is greater than hard limit: {} > {}", soft, hard);
            return None;
        }
        let _ = rlimit::Resource::NOFILE.set(soft, hard);
        info!("increase open files soft limit to 2048");
    }
    Some(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    configure_tracing();
    info!("initialized tracing");

    raise_fd_limit(2048).expect("failed to increase open files rlimit");
    ctrlc::set_handler(move || {
        info!("received SIGINT");
        process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let mut matches = command!() // requires `cargo` feature
        .next_line_help(true)
        .arg(Arg::new("secret-key")
            .required(false)
            .long("secret-key")
            .env("SECRET_KEY")
            .action(ArgAction::Set)
            .default_value("QJUHsPhnA0eiqHuJqsPgzhDozYO4f1zh")
            .help("<key> is a 32 character string that must match the key being passed to the notifications-service")
        )
        .arg(Arg::new("broker-list")
            .required(true)
            .action(ArgAction::Set)
            .short('b')
            .long("brokers")
            .help("<broker-list> is a comma-seperated list of brokers. E.g.  For a single local broker `localhost:9092`. For multiple brokers `localhost:9092,localhost:9093`")
        )
        .arg(Arg::new("topic")
            .required(true)
            .long("topic")
            .default_value("experiment")
            .action(ArgAction::Set)
        )
        .arg(Arg::new("group-id")
            .required(true)
            .long("group-id")
            .env("GROUP_ID")
            .action(ArgAction::Set)
        )
        .arg(Arg::new("consumer-wait-before-send")
            .required(false)
            .long("consumer-wait-before-send")
            .action(ArgAction::Set)
            .default_value("60")
            .value_parser(value_parser!(u8))
            .help("Time the consumer should wait before forwarding the experiment to the receiver")
        )
        .arg(Arg::new("hosts-file")
            .required(true)
            .long("hosts-file")
            .action(ArgAction::Set)
            .help("The file containing the list of hosts to be queried")
        )
        .arg(Arg::new("requestor-lag")
            .required(false)
            .long("requestor-lag")
            .action(ArgAction::Set)
            .default_value("5")
            .value_parser(value_parser!(u8))
            .help("Time the requestor lags behind the generator.")
        )
        .arg(Arg::new("requestor-retries")
            .required(false)
            .long("requestor-retries")
            .action(ArgAction::Set)
            .default_value("2")
            .value_parser(value_parser!(u8))
            .help("The number of retries in case a request fails due to a server error.")
        )
        .arg(Arg::new("requestor-max-in-flight")
            .required(false)
            .long("requestor-max-in-flight")
            .action(ArgAction::Set)
            .default_value("50")
            .value_parser(value_parser!(u16))
            .help("The maximum number of connections to a host.")
        )
        .arg(Arg::new("min-batch-size")
            .required(false)
            .long("min-batch-size")
            .env("MIN_BATCH_SIZE")
            .action(ArgAction::Set)
            .default_value("100")
            .value_parser(value_parser!(u16))
            .help("The minimum number of queries that has to be performed per second to each host.")
        )
        .arg(Arg::new("max-batch-size")
            .required(false)
            .long("max-batch-size")
            .env("MAX_BATCH_SIZE")
            .action(ArgAction::Set)
            .default_value("200")
            .value_parser(value_parser!(u16))
            .help("The maximum number of queries that can be performed per second to each host.")
        )
        .arg(Arg::new("stable-rate-duration")
            .required(false)
            .long("stable-rate-duration")
            .action(ArgAction::Set)
            .default_value("60")
            .value_parser(value_parser!(u16))
            .help("The number of seconds during which the rate at which the queries are performed to each host remains stable.")
        )
        .arg(Arg::new("num-generations")
            .required(true)
            .long("num-generations")
            .action(ArgAction::Set)
            .value_parser(value_parser!(u8))
            .help("The number of generate iterations to perform.\n\nE.g.:\nIf `--num-generations 2` and `--stable-rate duration 60`, then `60*2` batches of queries would be performed.")
        )
        .get_matches();

    let consume_config = ConsumeConfiguration::from(&mut matches);
    let consume = Consume::new(consume_config);

    let (experiment_tx, experiment_rx) = mpsc::channel(1000);
    let receiver_config = ExperimentReceiverConfig::from(&mut matches);
    let receiver = ExperimentReceiver::new(receiver_config, experiment_rx);
    let receiver_handle = tokio::spawn(receiver.start());

    tokio::spawn(async move {
        let span = span!(
            Level::INFO,
            "consumer",
        );
        consume.start(experiment_tx).instrument(span).await;
    });
    receiver_handle.await.expect("Join should not fail");
    Ok(())
}
