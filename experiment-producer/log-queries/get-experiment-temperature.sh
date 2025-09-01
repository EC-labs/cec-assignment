USAGE='
Usage: get-experiment-temperature.sh <log-file> <experiment-id>

    log-file: Log file containing json structured data
    experiment-id: The experiment whose measurements are to be printed

E.g.: get-experiment-temperature.sh "producer.json.log.2023-10-07" "2b9348c8-9051-4b27-a929-b5a93480fb82"
'

if (( "$#" != 2 )); then
    exit 1
fi

relevant_events=$(jq \
    --arg experiment "$2" \
    'select(any( .spans[]?; .experiment_id == $experiment ))' "$1" \
    | jq 'select(.fields.avg_temperature or .fields.stage or .fields.range_event)'
)


cat <<< $relevant_events \
    | jq '{
        "timestamp": .timestamp, 
        "event_type": (
            if .fields.stage then 
                "new_stage" 
            elif .fields.avg_temperature then
                "avg_temperature" 
            elif .fields.range_event then
                "range_event"
            else
                empty
            end
        ), 
        "value": (
            if .fields.stage then 
                .fields.stage
            elif .fields.avg_temperature then
                { "measurement_id": .span.measurement_id, "average": .fields.avg_temperature }
            elif .fields.range_event then
                { "measurement_id": .span.measurement_id, "event": .fields.range_event }
            else
                empty
            end
        )
    }' \
    | jq -s
