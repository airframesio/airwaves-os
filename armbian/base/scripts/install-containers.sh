#!/usr/bin/env bash

. /opt/aros/aros.config

docker create -p 80:80 -p 443:443 --name aros_web --restart=always \
  -v /var/run/docker.sock:/var/run/docker.sock -v /etc/aros:/etc/aros \
  ghcr.io/airframesio/feeder-web:${CONTAINER_AROS_FEEDER_WEB}

docker volume create portainer_data
docker create -p 8000:8000 -p 9443:9443 --name portainer --restart=always \
  -v /var/run/docker.sock:/var/run/docker.sock -v portainer_data:/data \
  portainer/portainer-ce:${CONTAINER_PORTAINER}
