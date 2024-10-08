#!/bin/bash

grafana_credentials() {
    username=$1; shift
    password=$1; shift
    echo "USERNAME=${username}"$'\n'"PASSWORD=${password}"
}

set -euo pipefail

script="$(basename $0)"
scriptd="$(dirname "$(realpath $0)")"

USAGE="
Usage: $script <num-groups> <group-credentials-dir>

Args: 
    num-groups: The number of groups
    group-credentials-dir: Directory where the different group credentials directories can be found. E.g. .../cec-infrastructure-2024/creds/groups if groups contains directory entries for group0, group1, etc...
"

if (( $# != 2 )); then
    echo "$USAGE"
    exit 1
fi

groups=$1; shift
if ! [[ $groups =~ [0-9]+ ]]; then 
    echo "$USAGE"
    exit 1
fi

groupsd=$1; shift
if ! [[ -d $groupsd ]]; then
    echo "$USAGE"
    exit 1 
fi

for (( i=18; i<$groups; i++ )); do

    username="group$i"
    password="$(tr -dc 'A-Za-z0-9!#$%&'\''()*+,-./:;<=>?@[\]^_`{|}~' </dev/urandom | head -c 13; echo)"

    groupd="${groupsd}/${username}"
    if ! [[ -d "$groupd" ]]; then
        echo "${groupd} is not a directory in ${groupsd}"
        exit 1
    fi

    curl -X POST \
        -H "Accept: application/json" \
        -H "Content-Type: application/json" \
        -L \
        -d '{
          "name": "'"group$i"'",
          "login": "'"group$i"'",
          "password": "'"$password"'",
          "email": "'"group$i@uu.nl"'",
          "OrgId": 1
        }' \
        https://landau:${PASSWORD}@grafana.cec.4400app.me/api/admin/users

    grafana_credentials "$username" "$password" > "$groupd/grafana"
done
