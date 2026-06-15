#!/usr/bin/env bash
#
# test-install-latest.sh — download the most recent CI-built uefi-x86 image
# artifact and run the automated QEMU install test against it. One command for
# the full validation cycle on a dev machine.
#
# Requires: gh (authenticated), qemu, expect, xz. Run from the repo root.
#
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
RELEASE="${1:-trixie}"
OUT="$(mktemp -d /tmp/airwaves-image.XXXXXX)"

echo "[fetch] finding latest successful build-image-simple run for uefi-x86/${RELEASE}..."
run_id="$(gh run list --workflow build-image-simple.yml --status success --limit 20 \
    --json databaseId,displayTitle \
    --jq "map(select(.displayTitle|test(\"uefi-x86.*${RELEASE}\";\"i\")))[0].databaseId")"
[ -n "${run_id}" ] && [ "${run_id}" != "null" ] || { echo "No successful uefi-x86/${RELEASE} build found"; exit 1; }
echo "[fetch] downloading artifact from run ${run_id}..."
gh run download "${run_id}" -n "airwaves-os-uefi-x86-${RELEASE}" -D "${OUT}"

img="$(ls "${OUT}"/*.img.xz 2>/dev/null | head -1)"
[ -n "${img}" ] || { echo "No .img.xz in artifact"; ls -R "${OUT}"; exit 1; }
echo "[fetch] image: ${img}"
exec "${HERE}/test-install-vm.sh" --image "${img}"
