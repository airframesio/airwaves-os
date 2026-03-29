#!/usr/bin/env bash
set -euo pipefail

# Build the airwaves-gateway container image with the control app bundled in.
# Clones airwaves-os-control and builds it into the nginx image.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTROL_APP_REPO="${CONTROL_APP_REPO:-https://github.com/airframesio/airwaves-os-control}"
CONTROL_APP_REF="${CONTROL_APP_REF:-main}"
CONTROL_APP_DIR="${SCRIPT_DIR}/control-app"
IMAGE_TAG="${IMAGE_TAG:-airwaves-gateway:latest}"

echo "==> Building airwaves-gateway"
echo "    Control app: ${CONTROL_APP_REPO}#${CONTROL_APP_REF}"

# Clone or update the control app
if [ -d "${CONTROL_APP_DIR}" ]; then
    echo "==> Updating control app..."
    git -C "${CONTROL_APP_DIR}" fetch origin
    git -C "${CONTROL_APP_DIR}" checkout "${CONTROL_APP_REF}"
    git -C "${CONTROL_APP_DIR}" pull origin "${CONTROL_APP_REF}" 2>/dev/null || true
else
    echo "==> Cloning control app..."
    git clone --depth 1 --branch "${CONTROL_APP_REF}" "${CONTROL_APP_REPO}" "${CONTROL_APP_DIR}"
fi

# Build the Docker image
echo "==> Building Docker image: ${IMAGE_TAG}"
docker build -t "${IMAGE_TAG}" "${SCRIPT_DIR}"

# Cleanup
echo "==> Cleaning up control app clone"
rm -rf "${CONTROL_APP_DIR}"

echo "==> Done: ${IMAGE_TAG}"
