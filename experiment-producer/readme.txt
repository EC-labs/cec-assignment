# Usage 

Usage: experiment-producer [OPTIONS] --brokers <broker-list>

E.g.: cargo run -p experiment-producer -- \
    --brokers 13.49.128.80:19093,13.49.128.80:29093,13.49.128.80:39093 \
    --topic client60

Options:
      --secret-key <secret-key>
          <key> is a 32 character string that must match the key being passed to the notifications-service [default: QJUHsPhnA0eiqHuJqsPgzhDozYO4f1zh]
  -b, --brokers <broker-list>
          <broker-list> is a comma-seperated list of brokers. E.g.  For a single local broker `localhost:9092`. For multiple brokers `localhost:9092,localhost:9093`
      --topic <topic>
          [default: experiment]
      --num-sensors <num-sensors>
          [default: 2]
      --sample-rate <sample-rate>
          [default: 100]
      --stabilization-samples <stabilization-samples>
          [default: 2]
      --carry-out-samples <carry-out-samples>
          [default: 20]
      --start-temperature <start-temperature>
          [default: 16]
      --lower-threshold <lower-threshold>
          [default: 25.5]
      --upper-threshold <upper-threshold>
          [default: 26.5]
      --disable-lz4
          Disable LZ4 compression of produced messages (enabled by default)
  -h, --help
          Print help
  -V, --version
          Print version
