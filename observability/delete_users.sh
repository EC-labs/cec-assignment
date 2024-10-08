#!/bin/bash

set -euo pipefail

for (( i=8; i<21; i++ )); do
    curl -X DELETE \
        -H "Accept: application/json" \
        -H "Content-Type: application/json" \
        -L \
        http://landau:${PASSWORD}@grafana.cec.4400app.me:3009/api/admin/users/$i
done
