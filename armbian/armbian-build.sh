#!/usr/bin/env bash
set -eu

if [ ! -d "armbian-build" ]; then
  git clone --depth=1 https://github.com/armbian/build armbian-build
  cd armbian-build || exit
  touch .ignore_changes
fi

mkdir -p armbian-build/output/
mkdir -p armbian-build/userpatches/
cp -a base/customize-image.sh armbian-build/userpatches/

BOARD=${BOARD:-rpi4}
BRANCH=${BRANCH:-current}
RELEASE=${RELEASE:-focal}

BUILD_ARGS="docker BOARD=${BOARD} KERNEL_ONLY=no KERNEL_CONFIGURE=no BUILD_MINIMAL=yes BUILD_DESKTOP=no BRANCH=${BRANCH} RELEASE=${RELEASE} WIREGUARD=no EXTRAWIFI=no PROGRESS_LOG_TO_FILE=yes"

time armbian-build/compile.sh ${BUILD_ARGS}

