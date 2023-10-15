#!/usr/bin/env bash
set -eu

if [ ! -d "armbian-build" ]; then
  git clone --depth=1 https://github.com/armbian/build armbian-build
  cd armbian-build || exit
#  touch .ignore_changes  ## this nightmare finally deprecated
  cd ..
fi

mkdir -p armbian-build/output/
mkdir -p armbian-build/userpatches/

cp -ar armbian-build/armbian-userpatches/* armbian-build/userpatches/

TEXT_IS_TOO_DARK=${TEXT_IS_TOO_DARK:-yes}  # if you have highcolor terminal you can set this to no... sometimes exta work needed in tmux etc

BUILD_ARGS="TEXT_IS_TOO_DARK=${TEXT_IS_TOO_DARK} airwaves-os ${@}"
pushd ./armbian-build
./compile.sh ${BUILD_ARGS}
popd
