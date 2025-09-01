USAGE='
Usage: get-experiments.sh <log-file> [<time-filter>]

Positional Args:
    log-file: Log file containing json structured data

Optional Args:
    time-filter: Timestamp in the format displayed in the standard output by the experiment-producer, e.g. "2023-10-07T15:53:18.160161+02".

Examples: 
    get-experiments.sh "producer.json.log.2023-10-07" "2023-10-07T15:53:18.160161+02"
'

if (( "$#" < 1 )); then
    echo "$USAGE"
    exit 1
fi

if ! [ -z "$2" ]; then
    timefilter="$2"
else
    timefilter="1970-01-01T00:00:00.000000+02"
fi

jq -r --arg timefilter "$timefilter" \
    'select(
        (.timestamp | sub("\\..+"; "Z") | fromdateiso8601)
        >= ($timefilter | sub("\\..+"; "Z") | fromdateiso8601)
    ) | select(.span.experiment_id != null) | .span.experiment_id' \
    "$1" \
    | sort | uniq

