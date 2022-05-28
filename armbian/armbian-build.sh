#!/usr/bin/env bash
set -eu

if [ ! -d "armbian-build" ]; then
  git clone --depth=1 https://github.com/armbian/build armbian-build
  cd armbian-build || exit
  touch .ignore_changes
  cd ..
fi

mkdir -p armbian-build/output/
mkdir -p armbian-build/userpatches/overlay
cp -aR base/* armbian-build/userpatches/overlay/
cp -a base/lib.config armbian-build/userpatches/
#cp -aR patches/* armbian-build/userpatches/

cp -a base/customize-image.sh armbian-build/userpatches/
cp -a base/config-default.conf armbian-build/userpatches/

BOARD=${BOARD:-rpi4b}
BRANCH=${BRANCH:-current}
RELEASE=${RELEASE:-jammy}
EXPERT=${EXPERT:-no}
PROGRESS_LOG_TO_FILE=${PROGRESS_LOG_TO_FILE:-yes}

BUILD_ARGS="docker BOARD=${BOARD} BUILD_MINIMAL=yes BUILD_DESKTOP=no BRANCH=${BRANCH} RELEASE=${RELEASE} WIREGUARD=no EXTRAWIFI=no PROGRESS_LOG_TO_FILE=${PROGRESS_LOG_TO_FILE}"

time armbian-build/compile.sh ${BUILD_ARGS}

