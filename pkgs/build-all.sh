#!/usr/bin/env bash

echo "ReceiverOS Package Builder 0.1.0"
echo ""

RELEASE_VERSION="${RELEASE_VERSION:-0.0.8}"
RELEASE_FILE="../releases/${RELEASE_VERSION}.yaml"
RELEASE_NAME=$(yq '.release.name' ${RELEASE_FILE})

TEMP_PATH="/tmp/aros/pkgs"
BUILD_LOG="${TEMP_PATH}/build.log"

# Functions

build_package() {
  name=$1
  version=$2
  echo " - ${name} ($version)"
  $name/build.sh $version 3>&1 2>&1 >> ${BUILD_LOG}
}

# Main


mkdir -p ${TEMP_PATH}
touch ${BUILD_LOG}

echo "Release ${RELEASE_VERSION} (${RELEASE_NAME})"
echo ""
echo "Building Packages (from source):"
build_package libacars 2.1.4
build_package acarsdec 3.7.0
build_package dumphfdl 1.3.0
build_package dumpvdl2 2.2.0
build_package vdlm2dec 2.2.0
echo ""
echo "Package build complete."

