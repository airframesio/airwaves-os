#!/bin/bash

set -ex

# arguments: $RELEASE $LINUXFAMILY $BOARD $BUILD_DESKTOP
#
# This is the image customization script

# NOTE: It is copied to /tmp directory inside the image
# and executed there inside chroot environment
# so don't reference any files that are not already installed

# NOTE: If you want to transfer files between chroot and host
# userpatches/overlay directory on host is bind-mounted to /tmp/overlay in chroot
# The sd card's root path is accessible via $SDCARD variable.

RELEASE=$1
LINUXFAMILY=$2
BOARD=$3
BUILD_DESKTOP=$4

Main() {
  case $RELEASE in
    stretch)
      CustomizeArmbian
      ;;
    buster)
      CustomizeArmbian
      ;;
    bullseye)
      CustomizeArmbian
      ;;
    bionic)
      CustomizeArmbian
      ;;
    focal)
      CustomizeArmbian
      ;;
    jammy)
      CustomizeArmbian
      ;;
  esac
}

CustomizeArmbian() {
  echo "Running AROS customization installer..."

  mkdir -p /opt/aros
  cp -aR /tmp/overlay/* /opt/aros
  chmod -R +x /opt/aros/scripts

  bash /tmp/overlay/customize-install-airwaves-os.sh $RELEASE $LINUXFAMILY $BOARD $BUILD_DESKTOP
}

Main "$@"
