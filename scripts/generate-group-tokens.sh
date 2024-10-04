#!/bin/bash

USAGE="
Usage: generate-group-tokens.sh <num-groups>

    num-groups: integer number of groups
"

if (( $# != 1 )); then
    cat <<< "$USAGE"
    exit 1
fi 


if ! [[ "$1" =~ ^[0-9]+$ ]]; then
    cat <<< "$USAGE"
    exit 1
fi

num_groups="$1"

for (( i=0; i<$num_groups; i++ )); do
    echo "=== group$i ==="
    cargo run -p notifications-service --bin generate-jwt-token -- --client-id "group${i}" > "../cec-infrastructure-2024/creds/groups/group${i}/token"
done
