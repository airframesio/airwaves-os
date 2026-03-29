#!/usr/bin/env bash
set -euo pipefail

# Airwaves OS Build Script
# Clones armbian/build at a pinned tag and runs compile with our userpatches.

ARMBIAN_BUILD_TAG="${ARMBIAN_BUILD_TAG:-v25.02}"
ARMBIAN_BUILD_DIR="${ARMBIAN_BUILD_DIR:-.armbian-build}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Airwaves OS Build System"
echo "    Armbian build tag: ${ARMBIAN_BUILD_TAG}"
echo "    Build directory:   ${ARMBIAN_BUILD_DIR}"

# Clone armbian/build at pinned tag if not present or if tag changed
if [ ! -d "${ARMBIAN_BUILD_DIR}" ]; then
    echo "==> Cloning armbian/build at ${ARMBIAN_BUILD_TAG}..."
    git clone --depth 1 --branch "${ARMBIAN_BUILD_TAG}" \
        https://github.com/armbian/build "${ARMBIAN_BUILD_DIR}"
elif [ -d "${ARMBIAN_BUILD_DIR}/.git" ]; then
    CURRENT_TAG=$(git -C "${ARMBIAN_BUILD_DIR}" describe --tags --exact-match 2>/dev/null || echo "unknown")
    if [ "${CURRENT_TAG}" != "${ARMBIAN_BUILD_TAG}" ]; then
        echo "==> Build tag changed (${CURRENT_TAG} -> ${ARMBIAN_BUILD_TAG}), re-cloning..."
        rm -rf "${ARMBIAN_BUILD_DIR}"
        git clone --depth 1 --branch "${ARMBIAN_BUILD_TAG}" \
            https://github.com/armbian/build "${ARMBIAN_BUILD_DIR}"
    fi
fi

# Ensure output directory exists
mkdir -p "${ARMBIAN_BUILD_DIR}/output"

# Symlink our userpatches into the build directory
# Remove any existing userpatches to avoid stale content
rm -rf "${ARMBIAN_BUILD_DIR}/userpatches"
ln -sfn "${SCRIPT_DIR}/userpatches" "${ARMBIAN_BUILD_DIR}/userpatches"

echo "==> Userpatches linked from ${SCRIPT_DIR}/userpatches"
echo "==> Starting Armbian build..."

# Forward all arguments to compile.sh
# Default config name is 'airwaves' if no config specified
BUILD_CONFIG="${1:-airwaves}"
shift 2>/dev/null || true

cd "${ARMBIAN_BUILD_DIR}"
./compile.sh "${BUILD_CONFIG}" "$@"
