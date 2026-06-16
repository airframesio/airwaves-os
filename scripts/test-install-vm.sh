#!/usr/bin/env bash
#
# test-install-vm.sh — automated QEMU test for the Airwaves x86 USB installer.
#
# Boots a CI-built x86 image as a "USB" disk plus a blank "internal" disk in a
# QEMU x86_64 UEFI VM, drives `airwaves-install` against the blank disk over the
# serial console, then reboots from the internal disk ALONE and asserts the
# installed system boots (manager /health responds on a forwarded port).
#
# Works on Apple Silicon: x86_64 is emulated via TCG (no acceleration), so runs
# are slow (tens of minutes) but real and repeatable.
#
# Requires: qemu (brew install qemu), expect, curl, qemu-img.
#
# Usage:
#   scripts/test-install-vm.sh --image /path/to/airwaves-uefi-x86.img [--ram 2560] [--target-size 16G]
#
set -euo pipefail

IMAGE=""
RAM=2560
TARGET_SIZE="16G"
SSH_USER="airwaves"
SSH_PASS="airwaves"
WORK="$(mktemp -d /tmp/airwaves-vmtest.XXXXXX)"
HOST_API_PORT=18080   # forwards to guest gateway :80
OVERLAY_DIR=""        # if set, serve these scripts to the guest before install
OVERLAY_PORT=18099    # host http port the guest fetches overlay scripts from

log() { echo "[vmtest] $*" >&2; }
die() { echo "[vmtest] ERROR: $*" >&2; exit 1; }

# Default overlay dir = the working-tree installer scripts, so iterating on
# airwaves-install/airwaves-firstrun doesn't need an image rebuild.
DEFAULT_OVERLAY="$(cd "$(dirname "$0")/.." && pwd)/armbian/userpatches/extensions/airwaves-os/scripts"

while [ $# -gt 0 ]; do
    case "$1" in
        --image) IMAGE="$2"; shift 2;;
        --ram) RAM="$2"; shift 2;;
        --target-size) TARGET_SIZE="$2"; shift 2;;
        --overlay-scripts) OVERLAY_DIR="${2:-$DEFAULT_OVERLAY}"; shift 2;;
        --overlay) OVERLAY_DIR="$DEFAULT_OVERLAY"; shift 1;;
        *) die "unknown arg: $1";;
    esac
done
[ -n "${IMAGE}" ] || die "usage: $0 --image <x86.img>"
[ -f "${IMAGE}" ] || die "image not found: ${IMAGE}"
command -v qemu-system-x86_64 >/dev/null || die "qemu-system-x86_64 not installed (brew install qemu)"
command -v expect >/dev/null || die "expect not installed"
command -v qemu-img >/dev/null || die "qemu-img not installed"

# Locate OVMF UEFI firmware shipped with qemu.
QEMU_SHARE="$(dirname "$(command -v qemu-system-x86_64)")/../share/qemu"
OVMF_CODE=""
for c in edk2-x86_64-code.fd OVMF_CODE.fd; do
    [ -f "${QEMU_SHARE}/${c}" ] && OVMF_CODE="${QEMU_SHARE}/${c}" && break
done
[ -n "${OVMF_CODE}" ] || die "OVMF UEFI firmware not found under ${QEMU_SHARE}"
# Writable UEFI variable store: copy the vars TEMPLATE (not the code file) so
# the firmware can boot and persist boot entries.
OVMF_VARS_SRC=""
for v in edk2-x86_64-vars.fd edk2-i386-vars.fd OVMF_VARS.fd; do
    [ -f "${QEMU_SHARE}/${v}" ] && OVMF_VARS_SRC="${QEMU_SHARE}/${v}" && break
done
[ -n "${OVMF_VARS_SRC}" ] || die "OVMF UEFI vars template not found under ${QEMU_SHARE}"
cp "${OVMF_VARS_SRC}" "${WORK}/OVMF_VARS.fd"
USB_IMG="${WORK}/usb.img"
case "${IMAGE}" in
    *.img.xz|*.xz)
        log "Decompressing image to a scratch USB disk (${IMAGE})..."
        command -v xz >/dev/null || die "xz not installed (brew install xz)"
        xz -dc "${IMAGE}" > "${USB_IMG}" ;;
    *)
        log "Copying image to a scratch USB disk (${IMAGE})..."
        cp "${IMAGE}" "${USB_IMG}" ;;
esac
TARGET_IMG="${WORK}/internal.qcow2"
qemu-img create -f qcow2 "${TARGET_IMG}" "${TARGET_SIZE}" >/dev/null
log "Work dir: ${WORK}"

qemu_common=(
    qemu-system-x86_64
    -machine q35
    -cpu qemu64
    -m "${RAM}"
    -drive "if=pflash,format=raw,readonly=on,file=${OVMF_CODE}"
    -drive "if=pflash,format=raw,file=${WORK}/OVMF_VARS.fd"
    -netdev "user,id=n0,hostfwd=tcp::${HOST_API_PORT}-:80"
    -device "virtio-net,netdev=n0"
    -nographic
)

# Optionally serve the working-tree installer scripts so each run tests the
# latest code against the existing image (no rebuild). The guest reaches the
# host at 10.0.2.2 (QEMU user-mode gateway).
OVERLAY_CMD=""
HTTP_PID=""
if [ -n "${OVERLAY_DIR}" ]; then
    [ -f "${OVERLAY_DIR}/airwaves-install" ] || die "overlay dir has no airwaves-install: ${OVERLAY_DIR}"
    log "Overlaying scripts from ${OVERLAY_DIR} (served on :${OVERLAY_PORT})..."
    ( cd "${OVERLAY_DIR}" && exec python3 -m http.server "${OVERLAY_PORT}" --bind 127.0.0.1 ) >/dev/null 2>&1 &
    HTTP_PID=$!
    trap 'kill "${HTTP_PID}" 2>/dev/null || true' EXIT
    OVERLAY_CMD="sudo sh -c 'cd /opt/airwaves/scripts && for f in airwaves-install airwaves-firstrun; do curl -fsS http://10.0.2.2:${OVERLAY_PORT}/\$f -o \$f && chmod +x \$f; done' ; echo OVERLAY_DONE"
fi

# ---- Phase 1: boot live USB, install to the blank internal disk -------------
log "Phase 1: booting live USB image, installing to internal disk..."
expect <<EXPECT || die "Phase 1 (install) failed or timed out"
set timeout 1800
spawn ${qemu_common[@]} \
    -drive file=${USB_IMG},format=raw,if=none,id=usb0 -device usb-ehci -device usb-storage,drive=usb0,bootindex=0 \
    -drive file=${TARGET_IMG},format=qcow2,if=virtio
# Wait for login prompt
expect {
    timeout { puts "TIMEOUT waiting for login"; exit 1 }
    -re "login: $"
}
send "${SSH_USER}\r"
expect -re "Password: $"
send "${SSH_PASS}\r"
expect -re "\\\$ $|# $"
# Overlay the latest installer scripts (no-op if not requested).
if {"${OVERLAY_CMD}" ne ""} {
    send "${OVERLAY_CMD}\r"
    expect { timeout { puts "TIMEOUT overlaying scripts"; exit 1 } -re "OVERLAY_DONE" }
}
# Identify the blank internal target (virtio disk, typically /dev/vda; the USB
# is the boot device). airwaves-install --list-json picks non-removable disks.
send "sudo bash -c 'AIRWAVES_INSTALL_APPLY=1 /opt/airwaves/scripts/airwaves-install --target \$(/opt/airwaves/scripts/airwaves-install --list-json | jq -r \".[0].device\")' ; echo INSTALL_RC=\$?\r"
# Stream until completion marker
expect {
    timeout { puts "TIMEOUT during install"; exit 1 }
    -re "INSTALL_RC=0" { }
    -re "INSTALL_RC=\[1-9\]" { puts "INSTALL FAILED"; exit 1 }
}
send "sudo poweroff\r"
expect eof
EXPECT
log "Phase 1 complete (install reported success)."

# ---- Phase 2: boot from the internal disk ALONE, assert it comes up ---------
log "Phase 2: booting from the internal disk only, asserting Airwaves health..."
"${qemu_common[@]}" \
    -drive "file=${TARGET_IMG},format=qcow2,if=virtio,bootindex=0" \
    -serial "file:${WORK}/boot2.log" -daemonize -pidfile "${WORK}/qemu.pid" \
    || die "failed to launch phase-2 VM"
QEMU_PID="$(cat "${WORK}/qemu.pid")"
trap 'kill "${QEMU_PID}" 2>/dev/null || true' EXIT

ok=""
for i in $(seq 1 60); do
    sleep 10
    if curl -fsS -m 4 "http://localhost:${HOST_API_PORT}/api/v1/system/overview" >/dev/null 2>&1; then
        ok=1; break
    fi
    log "  waiting for installed system to come up... (${i}/60)"
done
kill "${QEMU_PID}" 2>/dev/null || true

if [ -n "${ok}" ]; then
    log "PASS: installed system booted from internal disk and the manager is responding."
    exit 0
else
    log "FAIL: installed system did not come up. Last serial output:"
    tail -40 "${WORK}/boot2.log" 2>/dev/null || true
    exit 1
fi
