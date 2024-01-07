#!/bin/bash
set -e -o pipefail
cd $(dirname ${BASH_SOURCE[0]})

DOCKER_BUILDKIT=1 docker compose build --pull --parallel --build-arg TARGETARCH=x86_64
echo "======== build image inspect"
docker image inspect registry.skyser.kr/shipduck/ditto_bot_rust
echo "======== push image"
docker-compose push
