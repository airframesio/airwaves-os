#!/usr/bin/env bash

SCRIPT=$(realpath "$0")
SCRIPT_PATH=$(dirname "$SCRIPT")
source ${SCRIPT_PATH}/../common.sh

# Configurable Envs

SOURCE_NAME="dumphfdl"
SOURCE_VERSION="1.3.0"
SOURCE_GITHUB_REPO="szpajder/dumphfdl"
SOURCE_URL="https://github.com/${SOURCE_GITHUB_REPO}/archive/refs/tags/v${SOURCE_VERSION}.tar.gz"
DEPENDENCIES="build-essential cmake git libglib2.0-dev pkg-config libconfig++-dev libliquid-dev libfftw3-dev libsqlite3-dev libzmq3-dev"

# Derivative Envs

PACKAGE_NAME="${SOURCE_NAME}-${SOURCE_VERSION}"
TEMP_PATH="/tmp/aros/pkgs/${SOURCE_NAME}"
TAR_FILE="${PACKAGE_NAME}.tgz"
BUILD_PATH="${PACKAGE_NAME}/build"

# Build

sudo apt install -y ${DEPENDENCIES} 2>/dev/null
mkdir -p ${TEMP_PATH}
cd ${TEMP_PATH}
rm -rf ${TAR_FILE} ${PACKAGE_NAME}
wget -q -O ${TAR_FILE} ${SOURCE_URL}
tar zxvf ${TAR_FILE}

cd ${PACKAGE_NAME}
patch -u -i ${SCRIPT_PATH}/patch1

cmake -Bbuild
cmake --build build -j 2>&1
CPACK_DEBIAN_FILE_NAME="DEB-DEFAULT" cpack --config build/CPackConfig.cmake -G DEB 2>&1

mkdir -p ${DEST_PATH}
mv *.deb $DEST_PATH/
mv *.sha256 $DEST_PATH/

if [ -z "${KEEP_TEMP}" ]; then
  rm -rf ${TEMP_PATH}
fi

cd $EXEC_PATH
