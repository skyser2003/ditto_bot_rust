#!/bin/bash
set -e -o pipefail
cd $(dirname ${BASH_SOURCE[0]})

echo "======== push image"
docker compose push
