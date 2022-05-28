#!/bin/bash

set -e

RELEASE=$1
LINUXFAMILY=$2
BOARD=$3
BUILD_DESKTOP=$4

. aros.config

ImportFile() {
  echo "Importing file"
}

GenerateConfig() {
  echo "Generating config"
}

InstallAROS() {

  echo ""
  cat << EOF
     ___    ____  ____  _____
    /   |  / __ \/ __ \/ ___/
   / /| | / /_/ / / / /\__ \ 
  / ___ |/ _, _/ /_/ /___/ /
 /_/  |_/_/ |_|\____//____/
   Airframes Receiver OS

   Board    : $BOARD
   Family   : $LINUXFAMILY
   Release  : $RELEASE
   Hostname : $HOSTNAME
EOF
  echo ""
  echo "  Installing AROS base"

  echo root:airframes | chpasswd
  rm /root/.not_logged_in_yet

  export LANG=C LC_ALL="en_US.UTF-8"
  export DEBIAN_FRONTEND=noninteractive
  export APT_LISTCHANGES_FRONTEND=none

  echo "  Config:"

  echo "    - /etc/update-motd.d/10-armbian-header"
  chmod -x /etc/update-motd.d/10-armbian-header

  echo "    + /etc/update-motd.d/15-aros-header"
  cp /tmp/overlay/config/15-aros-header /etc/update-motd.d/
  chmod +x /etc/update-motd.d/15-aros-header

  echo "    + /etc/update-motd.d/50-aros-help"
  cp /tmp/overlay/config/50-aros-help /etc/update-motd.d/
  chmod +x /etc/update-motd.d/50-aros-help

  echo "    + /etc/avahi/services/aros.service"
  cp /tmp/overlay/config/templates/avahi-aros.service.template /etc/avahi/services/aros.service

  echo "    + /etc/network/interfaces.d/wlan0.conf"
  cp /tmp/overlay/config/templates/wlan0.conf.template /etc/network/interfaces.d/wlan0.conf

  echo "    + /root/.bashrc"
  cp /tmp/overlay/config/templates/bashrc-custom.template /root/.bashrc

  echo "    + /etc/aros/config.json"
  mkdir -p /etc/aros
  cp /tmp/overlay/config/templates/aros-config.json.template /etc/aros/config.json

  echo "    + setting hostname to ${HOSTNAME}"
  /tmp/overlay/scripts/change_hostname.sh $HOSTNAME

  echo ""

  echo "  Components:"
  InstallDocker
  InstallInitialContainers
  InstallTailscale

  chage -d 0 root

}

InstallDocker() {
  echo "    * Installing Docker"
  curl -fsSL https://get.docker.com | sh 2>&1 > /dev/null
}

InstallInitialContainers() {
  echo "    * Installing initial Docker images:"

  echo "      + airframesio/feeder-web                  [${CONTAINER_AROS_FEEDER_WEB}, enabled]"
  docker pull ghcr.io/airframesio/feeder-web:${CONTAINER_AROS_FEEDER_WEB}

  echo "      + airframesio/feeder-hfdl-dumphfdl        [${CONTAINER_AROS_HFDL_DUMPHFDL}, disabled]"
  echo "      + airframesio/feeder-vdl-dumpvdl2         [${CONTAINER_AROS_VDL_DUMPVDL2}, disabled]"
  echo "      + airframesio/feeder-satcom-aoa           [${CONTAINER_AROS_SATCOM_AOA}, disabled]"
  echo "      + airframesio/feeder-satcom-aoi           [${CONTAINER_AROS_SATCOM_AOI}, disabled]"

  echo "      + portainer                               [${CONTAINER_PORTAINER}, enabled]"
  docker pull portainer/portainer-ce:${CONTAINER_PORTAINER}
}

InstallTailscale() {
  echo "    * Installing Tailscale"
  curl -fsSL https://pkgs.tailscale.com/stable/ubuntu/focal.noarmor.gpg | sudo tee /usr/share/keyrings/tailscale-archive-keyring.gpg >/dev/null
  curl -fsSL https://pkgs.tailscale.com/stable/ubuntu/focal.tailscale-keyring.list | sudo tee /etc/apt/sources.list.d/tailscale.list >/dev/null
  sudo apt-get -qq update
  sudo apt-get -qq install tailscale
}

InstallAROS "$@"
