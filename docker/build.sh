#!/bin/bash
cd $(dirname ${BASH_SOURCE[0]})

docker-compose build --pull --parallel && docker-compose push
