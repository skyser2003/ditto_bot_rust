#!/bin/bash
cd $(dirname ${BASH_SOURCE[0]})

docker-compose build --pull --parallel --build-arg TARGETARCH=arm64 && docker-compose push
