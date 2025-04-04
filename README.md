# Airwaves OS

[![artifact-image-complete-matrix](https://github.com/airframesio/airwaves-os/actions/workflows/artifact-image-complete-matrix.yml/badge.svg)](https://github.com/airframesio/airwaves-os/actions/workflows/artifact-image-complete-matrix.yml)
![CodeRabbit Pull Request Reviews](https://img.shields.io/coderabbit/prs/github/airframesio/airwaves-os)
[![Contributors](https://img.shields.io/github/contributors/airframesio/airwaves-os)](https://github.com/airframesio/airwaves-os/graphs/contributors)
[![Activity](https://img.shields.io/github/commit-activity/m/airframesio/airwaves-os)](https://github.com/airframesio/airwaves-os/pulse)
[![Discord](https://img.shields.io/discord/1067697487927853077?logo=discord)](https://discord.gg/8Ksch7zE)

A radio-focused operating system that is:
- easy to install
- easy to use
- easy to maintain
- attractive and modern
- expandable as interest in the hobbies grow
- intended for hobbyists but suitable for professional use

Airwaves OS is based on [Armbian](https://armbian.com), a computing build framework that allows users to create system images with configurations for various single-board computers (SBCs), and extends it further to provide tunings and support for radio specific hardware and software. It is bundled with a custom user interface and API that allows it to be fully integrated.

More details to come.

## Download

An official public build has not been released yet while we work out some issues.

## Software

* acarsdec
* dumpvdl2
* vdlm2dec
* satdump
* tar1090

Absolutely not listing this out yet, as the OS will have a substantial number of packages and images available to install. Initial focus is on flight tracking feeder software, but will expand rapidly after initial rounds of testing. Some additional areas of focus are:

* ship tracking
* space/satellite tracking
* Air Traffic Controller audio capture and streaming
* Police/Fire/etc audio capture and streaming
* Meshtastic / MeshCore
* Reticulum / Nomad Network

## Building images

Airwaves OS can be built locally on a desktop for various architectures using the Armbian build flow, or via GitHub
for releases.

### Locally

1. Clone the repository to a Linux system, preferably Ubuntu 20.04.
2. Start the build with the build script `./armbian-build.sh`.

### GitHub

GitHub workflows will be triggered that will build the Airwaves OS images for various platforms.

