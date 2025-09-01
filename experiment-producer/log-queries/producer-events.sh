USAGE='
Usage: producer-events.sh <log-file> [<time-filter>]

Positional Args:
    log-file: Log file containing json structured data

Optional Args:
    time-filter: Timestamp in the format displayed in the standard output by the experiment-producer, e.g. "2023-10-07T15:53:18.160161+02".

Examples: 
    producer-events.sh "producer.json.log.2023-10-07" "2023-10-07T15:53:18.160161+02"
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

script_dir=$(cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd)
PATH="$script_dir:$PATH"

while read -r line; do
    echo 
    echo "*******************************************************"
    echo "*                                                     *"
    echo "*   EXPERIMENT $line   *"
    echo "*                                                     *"
    echo "*******************************************************"
    echo 
    get-experiment-temperature.sh $1 $line
done < <(get-experiments.sh "$1" "$2")
