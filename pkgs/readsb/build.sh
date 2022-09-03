#!/usr/bin/env bash

SCRIPT=$(realpath "$0")
SCRIPT_PATH=$(dirname "$SCRIPT")
source ${SCRIPT_PATH}/../common.sh

# Configurable Envs

SOURCE_NAME="readsb"
SOURCE_VERSION="3.14.1592~dev"
SOURCE_GITHUB_REPO="wiedehopf/${SOURCE_NAME}"
SOURCE_URL="https://github.com/${SOURCE_GITHUB_REPO}/tarball/dev"
DEPENDENCIES="git build-essential debhelper libusb-1.0-0-dev librtlsdr-dev librtlsdr0 pkg-config libncurses-dev zlib1g-dev zlib1g libzstd-dev libzstd1"

# Derivative Envs

PACKAGE_NAME="${SOURCE_NAME}-${SOURCE_VERSION}"
TEMP_PATH="/tmp/aros/pkgs/${SOURCE_NAME}"
TAR_FILE="${PACKAGE_NAME}.tgz"
BUILD_PATH="${PACKAGE_NAME}/build"

# Build

#sudo apt install -y ${DEPENDENCIES} 2>/dev/null
mkdir -p ${TEMP_PATH}
cd ${TEMP_PATH}
rm -rf ${TAR_FILE} ${PACKAGE_NAME}
wget -q -O ${TAR_FILE} ${SOURCE_URL}
mkdir -p ${PACKAGE_NAME}
tar zxvf ${TAR_FILE} --strip-components=1 -C ${PACKAGE_NAME}

cd ${PACKAGE_NAME}
export DEB_BUILD_OPTIONS=noddebs
dpkg-buildpackage -b -Prtlsdr -ui -uc -us
cd ..

PACKAGE_FILE=$(ls ${SOURCE_NAME}*.deb | head -1)
mkdir -p ${DEST_PATH}
sha256sum ${PACKAGE_FILE} > ${PACKAGE_FILE}.sha256
mv ${PACKAGE_FILE} $DEST_PATH/
mv ${PACKAGE_FILE}.sha256 $DEST_PATH/

if [ -z "${KEEP_TEMP}" ]; then
  rm -rf ${TEMP_PATH}
fi

cd $EXEC_PATH
