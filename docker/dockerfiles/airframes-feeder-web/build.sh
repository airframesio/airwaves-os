#!/usr/bin/env bash

DOCKER_VERSION="0.0.2"

rm -rf feeder-web
gh repo clone airframesio/feeder-web

cd feeder-web
sudo docker build -t ghcr.io/airframesio/feeder-web .
sudo docker push ghcr.io/airframesio/feeder-web:0.0.2
