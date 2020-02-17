#!/bin/bash

cd $(dirname ${BASH_SOURCE[0]})
cd docker
docker-compose build --pull --parallel && docker-compose push

