#!/usr/bin/env bash
set -euo pipefail

# Airwaves OS Build Script
# Clones armbian/build at a pinned tag and runs compile with our userpatches.

ARMBIAN_BUILD_TAG="${ARMBIAN_BUILD_TAG:-v26.2.1}"
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

# Brand the image filename with the Airwaves OS version. Armbian otherwise sets
# REVISION to its own framework version (e.g. 26.02.0-trunk), so images come out
# as Airwaves_OS_26.02.0-trunk_<board>_... — the Airwaves version is missing.
# Read the version from the manager crate; on a release tag use the bare version
# (Airwaves_OS_1.0.37_<board>_...), otherwise append the short commit so dev
# images are distinguishable. Respect an explicit REVISION= if one was passed.
have_revision=0
for arg in "$@"; do case "${arg}" in REVISION=*) have_revision=1;; esac; done
if [ "${have_revision}" -eq 0 ]; then
    AW_VER="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' \
        "${SCRIPT_DIR}/../containers/airwaves-manager/Cargo.toml" 2>/dev/null | head -1)"
    if [ -n "${AW_VER}" ]; then
        if git -C "${SCRIPT_DIR}/.." describe --tags --exact-match >/dev/null 2>&1; then
            AW_REVISION="${AW_VER}"
        else
            AW_REVISION="${AW_VER}-$(git -C "${SCRIPT_DIR}/.." rev-parse --short HEAD 2>/dev/null || echo dev)"
        fi
        set -- "$@" "REVISION=${AW_REVISION}"
        echo "    Image revision:    ${AW_REVISION} (Airwaves OS version)"
    fi
fi

cd "${ARMBIAN_BUILD_DIR}"
./compile.sh "${BUILD_CONFIG}" "$@"
