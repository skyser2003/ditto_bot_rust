#!/bin/bash

docker-compose build --pull --parallel
docker-compose push
