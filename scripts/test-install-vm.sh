#!/usr/bin/env bash
#
# test-install-vm.sh — automated QEMU test for the Airwaves x86 USB installer.
#
# Boots a CI-built x86 image as a "USB" disk plus a blank "internal" disk in a
# QEMU x86_64 UEFI VM, drives `airwaves-install` against the blank disk OVER SSH
# (a clean channel — the serial console is too noisy with boot logs to drive
# reliably), then reboots from the internal disk ALONE and asserts the installed
# system boots (the manager answers on a forwarded port).
#
# Works on Apple Silicon: x86_64 is emulated via TCG (no acceleration), so runs
# are slow (tens of minutes) but real and repeatable.
#
# Requires: qemu, expect, curl, qemu-img, xz.
#
# Usage:
#   scripts/test-install-vm.sh --image <x86.img(.xz)> [--ram 2560] [--target-size 16G] [--overlay]
#
set -euo pipefail

IMAGE=""; RAM=2560; TARGET_SIZE="16G"; OVERLAY_DIR=""; WEB_MODE=""
SSH_PORT=2222; API_PORT=18080; PW="airwaves"
WORK="$(mktemp -d /tmp/airwaves-vmtest.XXXXXX)"
DEFAULT_OVERLAY="$(cd "$(dirname "$0")/.." && pwd)/armbian/userpatches/extensions/airwaves-os/scripts"

log() { echo "[vmtest] $*" >&2; }
die() { echo "[vmtest] ERROR: $*" >&2; exit 1; }
cleanup() { [ -n "${WORK:-}" ] && pkill -f "${WORK}" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

while [ $# -gt 0 ]; do
    case "$1" in
        --image) IMAGE="$2"; shift 2;;
        --ram) RAM="$2"; shift 2;;
        --target-size) TARGET_SIZE="$2"; shift 2;;
        --overlay) OVERLAY_DIR="$DEFAULT_OVERLAY"; shift 1;;
        --overlay-scripts) OVERLAY_DIR="${2:-$DEFAULT_OVERLAY}"; shift 2;;
        --web) WEB_MODE=1; shift 1;;   # drive the install via the manager web API
        *) die "unknown arg: $1";;
    esac
done
[ -f "${IMAGE}" ] || die "image not found: ${IMAGE}"
for t in qemu-system-x86_64 expect qemu-img xz curl; do command -v "$t" >/dev/null || die "$t not installed"; done

QEMU_SHARE="$(dirname "$(command -v qemu-system-x86_64)")/../share/qemu"
OVMF_CODE=""; for c in edk2-x86_64-code.fd OVMF_CODE.fd; do [ -f "${QEMU_SHARE}/${c}" ] && OVMF_CODE="${QEMU_SHARE}/${c}" && break; done
[ -n "${OVMF_CODE}" ] || die "OVMF firmware not found"
OVMF_VARS_SRC=""; for v in edk2-i386-vars.fd edk2-x86_64-vars.fd OVMF_VARS.fd; do [ -f "${QEMU_SHARE}/${v}" ] && OVMF_VARS_SRC="${QEMU_SHARE}/${v}" && break; done
[ -n "${OVMF_VARS_SRC}" ] || die "OVMF vars template not found"
cp "${OVMF_VARS_SRC}" "${WORK}/OVMF_VARS.fd"

USB_IMG="${WORK}/usb.img"; TARGET_IMG="${WORK}/internal.qcow2"
case "${IMAGE}" in *.xz) log "Decompressing image..."; xz -dc "${IMAGE}" > "${USB_IMG}";; *) cp "${IMAGE}" "${USB_IMG}";; esac
qemu-img create -f qcow2 "${TARGET_IMG}" "${TARGET_SIZE}" >/dev/null
log "Work dir: ${WORK}"

# expect ssh wrapper: argv = user cmd ; sends password, streams output, exits
# with ssh's exit code. (timeout long enough for the install rsync.)
cat > "${WORK}/ssh.exp" <<'EXP'
set timeout 3000
set user [lindex $argv 0]
set cmd  [lindex $argv 1]
spawn ssh -p [lindex $argv 2] -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o PreferredAuthentications=password -o PubkeyAuthentication=no -o ConnectTimeout=8 \
    $user@localhost $cmd
expect {
    -re {[Pp]assword:} { send "[lindex $argv 3]\r"; exp_continue }
    eof
}
catch wait result
exit [lindex $result 3]
EXP
sshx() { expect "${WORK}/ssh.exp" "$1" "$2" "${SSH_PORT}" "${PW}"; }   # $1=user $2=cmd

boot() {  # $1=serial-log  $2..=extra qemu args
    local sl="$1"; shift
    qemu-system-x86_64 -machine q35 -cpu qemu64 -m "${RAM}" \
        -drive "if=pflash,format=raw,readonly=on,file=${OVMF_CODE}" \
        -drive "if=pflash,format=raw,file=${WORK}/OVMF_VARS.fd" \
        -netdev "user,id=n0,hostfwd=tcp::${SSH_PORT}-:22,hostfwd=tcp::${API_PORT}-:80" \
        -device virtio-net,netdev=n0 -display none -serial "file:${sl}" \
        -daemonize -pidfile "${WORK}/pid" "$@" || die "qemu launch failed"
    cat "${WORK}/pid"
}

# ---- Phase 1: boot live USB, install to the blank internal disk -------------
log "Phase 1: booting live USB + blank internal disk..."
P1=$(boot "${WORK}/boot1.log" \
    -drive "file=${USB_IMG},format=raw,if=none,id=u0" -device usb-ehci -device "usb-storage,drive=u0,bootindex=0" \
    -drive "file=${TARGET_IMG},format=qcow2,if=virtio")

if [ -n "${WEB_MODE}" ]; then
    # Drive the install entirely through the manager's web API (the same path
    # the control-app "Install to Disk" wizard uses). Requires the manager
    # container to be up, which on first boot means pulling it (slow on TCG).
    log "Web mode: waiting for the manager API (first-boot container pull is slow)..."
    up=""
    for i in $(seq 1 180); do  # up to ~30 min
        sleep 10
        curl -fsS -m4 "http://localhost:${API_PORT}/api/v1/system/overview" >/dev/null 2>&1 && { up=1; log "  manager up (~$((i*10))s)"; break; }
    done
    [ -n "${up}" ] || { tail -20 "${WORK}/boot1.log" 2>/dev/null; die "manager never came up on the live USB"; }

    disks="$(curl -fsS -m8 "http://localhost:${API_PORT}/api/v1/system/disks")" || die "GET /system/disks failed"
    dev="$(echo "${disks}" | jq -r '.[0].device // empty')"
    [ -n "${dev}" ] || { echo "${disks}"; die "no install target offered by /system/disks"; }
    log "Installing to ${dev} via POST /system/install..."
    curl -fsS -m10 -X POST "http://localhost:${API_PORT}/api/v1/system/install" \
        -H 'Content-Type: application/json' -d "{\"device\":\"${dev}\"}" >/dev/null || die "POST /system/install failed"

    st=""
    for i in $(seq 1 220); do  # up to ~37 min for the install
        sleep 10
        p="$(curl -fsS -m6 "http://localhost:${API_PORT}/api/v1/system/install/progress" 2>/dev/null || echo '{}')"
        st="$(echo "${p}" | jq -r '.state // ""')"
        log "  install: $(echo "${p}" | jq -r '.state // "?"') $(echo "${p}" | jq -r '.phase // ""') $(echo "${p}" | jq -r '.percent // 0')%"
        [ "${st}" = "success" ] && break
        [ "${st}" = "failed" ] && { echo "${p}" | jq -r '.error // "see log"'; die "web install reported failure"; }
    done
    [ "${st}" = "success" ] || die "web install did not finish in time"
    log "Phase 1 (web install) success."
    kill "${P1}" 2>/dev/null || true
    for i in $(seq 1 30); do
        kill -0 "${P1}" 2>/dev/null && { sleep 1; continue; }
        lsof -nP -iTCP:"${API_PORT}" 2>/dev/null | grep -q LISTEN || break
        sleep 1
    done
else

log "Waiting for SSH + discovering login (root vs airwaves)..."
SSH_USER=""; SUDO=""
for i in $(seq 1 40); do
    sleep 15
    out="$(sshx root 'echo PROBE_$(id -u)' 2>/dev/null || true)"
    if echo "$out" | grep -q "PROBE_0"; then SSH_USER=root; SUDO=""; log "  ssh root OK (after ~$((i*15))s)"; break; fi
    if echo "$out" | grep -qiE "permission denied"; then
        a="$(sshx airwaves 'echo PROBE_$(id -u)' 2>/dev/null || true)"
        if echo "$a" | grep -qE "PROBE_[0-9]+"; then SSH_USER=airwaves; SUDO="echo ${PW} | sudo -S "; log "  ssh airwaves OK (root login disabled)"; break; fi
    fi
done
[ -n "${SSH_USER}" ] || { tail -8 "${WORK}/boot1.log" 2>/dev/null; die "SSH never became usable"; }

if [ -n "${OVERLAY_DIR}" ]; then
    log "Overlaying working-tree scripts via SSH (base64-in-command)..."
    for f in airwaves-install airwaves-firstrun; do
        [ -f "${OVERLAY_DIR}/${f}" ] || continue
        b64="$(base64 < "${OVERLAY_DIR}/${f}" | tr -d '\n')"
        sshx "${SSH_USER}" "${SUDO}bash -c 'echo ${b64} | base64 -d > /opt/airwaves/scripts/${f} && chmod +x /opt/airwaves/scripts/${f}'" >/dev/null \
            || die "overlay of ${f} failed"
    done
fi

log "Running airwaves-install on the blank disk (this is the slow part)..."
INSTALL_CMD="${SUDO}bash -c 'TGT=\$(/opt/airwaves/scripts/airwaves-install --list-json | jq -r \".[0].device\"); echo TARGET=\$TGT; AIRWAVES_INSTALL_APPLY=1 /opt/airwaves/scripts/airwaves-install --target \$TGT'"
if sshx "${SSH_USER}" "${INSTALL_CMD}"; then
    log "Phase 1: airwaves-install returned success."
else
    die "airwaves-install failed (see output above)"
fi
sshx "${SSH_USER}" "${SUDO}poweroff" >/dev/null 2>&1 || true
sleep 5
kill "${P1}" 2>/dev/null || true
# Wait for the phase-1 VM to fully exit and release its forward ports before
# phase 2 tries to bind them (else qemu launch fails).
for i in $(seq 1 30); do
    kill -0 "${P1}" 2>/dev/null && { sleep 1; continue; }
    lsof -nP -iTCP:"${SSH_PORT}" -iTCP:"${API_PORT}" 2>/dev/null | grep -q LISTEN || break
    sleep 1
done
fi  # end SSH-vs-web phase 1

# ---- Phase 2: boot from internal disk ALONE, assert manager answers ---------
# Use a FRESH UEFI varstore: phase 1's vars point at the (now-absent) USB boot
# entry, so reusing them drops OVMF to the UEFI shell instead of discovering the
# installed disk's removable bootloader (/EFI/BOOT/BOOTX64.EFI).
cp "${OVMF_VARS_SRC}" "${WORK}/OVMF_VARS.fd"
log "Phase 2: booting from the internal disk only..."
P2=$(boot "${WORK}/boot2.log" -drive "file=${TARGET_IMG},format=qcow2,if=virtio")
ok=""
for i in $(seq 1 60); do
    sleep 10
    if curl -fsS -m 4 "http://localhost:${API_PORT}/api/v1/system/overview" >/dev/null 2>&1; then ok=1; break; fi
    log "  waiting for installed system to come up... (${i}/60)"
done
kill "${P2}" 2>/dev/null || true

if [ -n "${ok}" ]; then
    log "PASS: installed system booted from the internal disk and the manager is responding."
    exit 0
else
    log "FAIL: installed system did not come up. Last serial output:"; tail -40 "${WORK}/boot2.log" 2>/dev/null || true
    exit 1
fi
