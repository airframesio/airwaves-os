#!/usr/bin/env bash
set -euo pipefail

# Airwaves OS Build VM Setup
# Runs ON the Hetzner VM after creation to install build dependencies,
# clone the repo, and prepare for Armbian image builds.

echo "==> Airwaves OS Build VM Setup"
echo "    $(date -u)"
echo ""

# Update system
echo "==> Installing system packages..."
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get upgrade -y
DEBIAN_FRONTEND=noninteractive apt-get install -y \
    git curl wget jq \
    docker.io docker-compose-plugin \
    qemu-user-static binfmt-support \
    build-essential \
    python3 python3-pip \
    unzip pigz \
    acl uuid-runtime \
    dialog

# Enable Docker
systemctl enable docker
systemctl start docker
usermod -aG docker root

# Clone the Airwaves OS repo
echo "==> Cloning Airwaves OS..."
cd /root
if [ ! -d airwaves-os ]; then
    git clone https://github.com/airframesio/airwaves-os.git
fi
cd airwaves-os

# Clone the Armbian build framework
echo "==> Cloning Armbian build framework..."
./armbian/build.sh --help 2>/dev/null || true
# The build script will clone armbian/build on first run

echo ""
echo "============================================"
echo "  Build VM ready!"
echo ""
echo "  Build commands:"
echo "    cd /root/airwaves-os"
echo ""
echo "    # Build for x86 (mini PC / server):"
echo "    ./armbian/build.sh airwaves BOARD=uefi-x86 BRANCH=current RELEASE=bookworm"
echo ""
echo "    # Build for Raspberry Pi 4B:"
echo "    ./armbian/build.sh airwaves BOARD=rpi4b BRANCH=current RELEASE=noble"
echo ""
echo "    # Build for Raspberry Pi 5:"
echo "    ./armbian/build.sh airwaves BOARD=rpi5b BRANCH=current RELEASE=noble"
echo ""
echo "    # Build for Rock 5B:"
echo "    ./armbian/build.sh airwaves BOARD=rock-5b BRANCH=current RELEASE=bookworm"
echo ""
echo "    # Build for Orange Pi 5:"
echo "    ./armbian/build.sh airwaves BOARD=orangepi5 BRANCH=current RELEASE=bookworm"
echo ""
echo "  Images will be in:"
echo "    /root/airwaves-os/.armbian-build/output/images/"
echo "============================================"
