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
cp -aR patches/* armbian-build/userpatches/

##TODO refactor customize-image.sh to use the extension and hooks model with a userpatches/config-example.conf 
cp -a base/customize-image.sh armbian-build/userpatches/
##TODO rename this airwaveos  default is no longer loaded by default.
cp -a base/config-default.conf armbian-build/userpatches/

#if you want to override these variables, set them up front...   ex.  BOARD=whatever SHOW_DEBUG=yes ./armbian-build.sh

BOARD=${BOARD:-rpi4b}
BRANCH=${BRANCH:-current}
RELEASE=${RELEASE:-jammy}
EXPERT=${EXPERT:-no}
PROGRESS_LOG_TO_FILE=${PROGRESS_LOG_TO_FILE:-yes}
SHOW_DEBUG=${SHOW_DEBUG:-no}
SHOW_COMMANDS=${SHOW_COMMANDS:-no}
TEXT_IS_TOO_DARK=${TEXT_IS_TOO_DARK:-yes}  # if you have highcolor terminal you can set this to no... sometimes exta work needed in tmux etc

BUILD_ARGS="TEXT_IS_TOO_DARK=${TEXT_IS_TOO_DARK} SHOW_DEBUG=${SHOW_DEBUG} SHOW_COMMANDS=${SHOW_COMMANDS} BOARD=${BOARD} BRANCH=${BRANCH} RELEASE=${RELEASE} WIREGUARD=no EXTRAWIFI=no PROGRESS_LOG_TO_FILE=${PROGRESS_LOG_TO_FILE} default"
pushd ./armbian-build
./compile.sh ${BUILD_ARGS}
popd
