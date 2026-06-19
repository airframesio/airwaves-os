#!/usr/bin/env bash
set -euo pipefail

# Airwaves OS Build Script
# Clones armbian/build at a pinned tag and runs compile with our userpatches.

# v26.5.1: bumped from v26.2.1 to fix mainline-kernel archive patch drift that
# blocked the sunxi (Allwinner) and mainline-rockchip (rk3399/rk3568/rk3328)
# boards — at v26.2.1 the verisilicon-AV1 / megous usb-gadget patches failed to
# apply and Armbian's prebuilt kernel was absent from the OCI cache, forcing a
# from-source build that aborted. v26.5.1's archive patches match the kernel tags
# (validated: sunxi orangepizero3 + vendor rock-5b canaries build clean).
ARMBIAN_BUILD_TAG="${ARMBIAN_BUILD_TAG:-v26.5.1}"
ARMBIAN_BUILD_DIR="${ARMBIAN_BUILD_DIR:-.armbian-build}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Airwaves OS Build System"
echo "    Armbian build tag: ${ARMBIAN_BUILD_TAG}"
echo "    Build directory:   ${ARMBIAN_BUILD_DIR}"

# Fetch the armbian/build source at the pinned tag INTO ${ARMBIAN_BUILD_DIR}.
# CI restores the build cache (output/debs, cache/*) into this directory BEFORE
# this runs, so it can already exist without the armbian checkout — a plain
# `git clone` would refuse the non-empty dir and a `[ ! -d ]` guard would skip
# it (leaving compile.sh missing). Detect a valid checkout by compile.sh and
# populate via init+fetch+checkout, which works in a non-empty dir and preserves
# the restored cache subdirs (armbian gitignores output/ and cache/).
populate_armbian() {
    mkdir -p "${ARMBIAN_BUILD_DIR}"
    git -C "${ARMBIAN_BUILD_DIR}" init -q
    if git -C "${ARMBIAN_BUILD_DIR}" remote get-url origin >/dev/null 2>&1; then
        git -C "${ARMBIAN_BUILD_DIR}" remote set-url origin https://github.com/armbian/build
    else
        git -C "${ARMBIAN_BUILD_DIR}" remote add origin https://github.com/armbian/build
    fi
    git -C "${ARMBIAN_BUILD_DIR}" fetch --depth 1 origin \
        "refs/tags/${ARMBIAN_BUILD_TAG}:refs/tags/${ARMBIAN_BUILD_TAG}"
    git -C "${ARMBIAN_BUILD_DIR}" checkout -q -f "${ARMBIAN_BUILD_TAG}"
}

if [ ! -f "${ARMBIAN_BUILD_DIR}/compile.sh" ]; then
    echo "==> Fetching armbian/build at ${ARMBIAN_BUILD_TAG} (preserving any restored cache)..."
    populate_armbian
else
    CURRENT_TAG=$(git -C "${ARMBIAN_BUILD_DIR}" describe --tags --exact-match 2>/dev/null || echo "unknown")
    if [ "${CURRENT_TAG}" != "${ARMBIAN_BUILD_TAG}" ]; then
        echo "==> Build tag changed (${CURRENT_TAG} -> ${ARMBIAN_BUILD_TAG}), re-fetching..."
        populate_armbian
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
# NOTE: do NOT override REVISION to brand the filename — Armbian bakes REVISION
# into the base-files package version, and a value like 1.0.37 sorts BELOW the
# stock Debian base-files (13.x/12.x), so apt rejects it as a downgrade and the
# rootfs build fails. The Airwaves version is stamped into the image *filename*
# after the build instead (see the workflow's rename step).
BUILD_CONFIG="${1:-airwaves}"
shift 2>/dev/null || true

cd "${ARMBIAN_BUILD_DIR}"
./compile.sh "${BUILD_CONFIG}" "$@"
