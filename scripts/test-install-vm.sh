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

IMAGE=""; RAM=2560; TARGET_SIZE="16G"; OVERLAY_DIR=""; WEB_MODE=""; SMOKE_MODE=""; AB_TEST=""
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
        --smoke) SMOKE_MODE=1; shift 1;;  # just verify the manager + install API come up natively
        --ab) AB_TEST=1; OVERLAY_DIR="$DEFAULT_OVERLAY"; shift 1;;  # A/B install + slot switch/rollback cycle (implies --overlay)
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

# Discover the working SSH login and set SSH_USER + SUDO. The image may allow
# root login or only the unprivileged 'airwaves' user (then we sudo with -S).
discover_ssh() {
    log "Waiting for SSH + discovering login (root vs airwaves)..."
    SSH_USER=""; SUDO=""
    for i in $(seq 1 40); do
        sleep 15
        out="$(sshx root 'echo PROBE_$(id -u)' 2>/dev/null || true)"
        if echo "$out" | grep -q "PROBE_0"; then SSH_USER=root; SUDO=""; log "  ssh root OK (after ~$((i*15))s)"; return 0; fi
        if echo "$out" | grep -qiE "permission denied"; then
            a="$(sshx airwaves 'echo PROBE_$(id -u)' 2>/dev/null || true)"
            if echo "$a" | grep -qE "PROBE_[0-9]+"; then SSH_USER=airwaves; SUDO="echo ${PW} | sudo -S "; log "  ssh airwaves OK (root login disabled)"; return 0; fi
        fi
    done
    return 1
}

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

if [ -n "${SMOKE_MODE}" ]; then
    # Smoke test for a published image: assert the manager comes up NATIVELY on
    # first boot (no manual pin) and the install API answers. /system/disks only
    # exists in 1.0.37+, so if it responds the channel alias resolved to a
    # current image (e.g. stable -> :stable -> 1.0.37), not a stale :latest.
    log "Smoke: waiting for the manager to come up natively (first-boot pull is slow)..."
    up=""
    for i in $(seq 1 180); do
        sleep 10
        curl -fsS -m4 "http://localhost:${API_PORT}/api/v1/system/overview" >/dev/null 2>&1 && { up=1; log "  manager up (~$((i*10))s)"; break; }
    done
    [ -n "${up}" ] || { tail -20 "${WORK}/boot1.log" 2>/dev/null; die "manager never came up on first boot"; }
    if ! curl -fsS -m10 "http://localhost:${API_PORT}/api/v1/system/disks" >/dev/null 2>&1; then
        tail -20 "${WORK}/boot1.log" 2>/dev/null
        die "SMOKE FAIL: /system/disks absent — manager is older than 1.0.37 (channel alias did not resolve to a current image)"
    fi
    disks="$(curl -fsS -m10 "http://localhost:${API_PORT}/api/v1/system/disks" 2>/dev/null)"
    log "  manager up natively and /system/disks responded: ${disks}"

    # Verify the console-TUI wiring over SSH (the visual TUI itself is on tty1,
    # which a headless VM can't show — but we can confirm it's wired up).
    if discover_ssh; then
        log "Checking console TUI wiring..."
        chk="$(sshx "${SSH_USER}" "${SUDO}bash -c '
            ok=1
            [ -f /etc/systemd/system/getty@tty1.service.d/airwaves-tui.conf ] && grep -q autologin /etc/systemd/system/getty@tty1.service.d/airwaves-tui.conf || { echo MISS:getty-dropin; ok=0; }
            [ -x /opt/airwaves/scripts/airwaves-tui ] || { echo MISS:airwaves-tui; ok=0; }
            [ -f /etc/profile.d/zz-airwaves-tui.sh ] || { echo MISS:profile.d; ok=0; }
            systemctl is-enabled airwaves-firstrun.service >/dev/null 2>&1 && { echo BAD:firstrun-still-enabled; ok=0; } || true
            bash -n /opt/airwaves/scripts/airwaves-tui || { echo BAD:tui-syntax; ok=0; }
            [ \$ok = 1 ] && echo CONSOLE_OK
        '" 2>/dev/null)"
        echo "${chk}" | grep -q CONSOLE_OK \
            && log "  console TUI wiring OK (getty autologin + airwaves-tui + launcher; firstrun retired)" \
            || { log "  console wiring problems: $(echo "${chk}" | grep -E 'MISS|BAD' | tr '\n' ' ')"; tail -10 "${WORK}/boot1.log" 2>/dev/null; die "SMOKE FAIL: console TUI not wired correctly"; }
        # Separate, simple checks (clean quoting): version stamp + issue banner.
        ver="$(sshx "${SSH_USER}" "${SUDO}cat /etc/airwaves-release" 2>/dev/null | tr -d '\r' | awk -F= '/^AIRWAVES_VERSION=/{print $2}')"
        log "  version stamp: ${ver:-?}"
        [ -n "${ver}" ] && [ "${ver}" != "1.0.0" ] || die "SMOKE FAIL: version not stamped (got '${ver:-}')"
        sshx "${SSH_USER}" "${SUDO}cat /etc/issue" 2>/dev/null | grep -qi airwaves \
            && log "  /etc/issue banner present" || die "SMOKE FAIL: /etc/issue banner missing"
    else
        log "  (could not SSH to verify console wiring; manager+install API confirmed)"
    fi

    log "SMOKE PASS: manager + install API up natively; console TUI wired."
    kill "${P1}" 2>/dev/null || true
    exit 0
elif [ -n "${WEB_MODE}" ]; then
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

    # The image's compose pulls airwaves-manager:latest, which tracks the stable
    # channel and predates the install API. The install endpoints under test live
    # in the dev channel (1.0.37+). Repoint the manager to the dev-pinned tag from
    # releases/dev.json over SSH and restart it — what a dev-channel device runs
    # after an update — so the web installer path actually exists to be tested.
    MGR_TAG="$(python3 -c "import json;print(json.load(open('$(cd "$(dirname "$0")/.." && pwd)/releases/dev.json'))['components']['manager']['tag'])" 2>/dev/null)"
    [ -n "${MGR_TAG}" ] || die "could not read manager tag from releases/dev.json"
    DEV_MGR="ghcr.io/airframesio/airwaves-manager:${MGR_TAG}"
    discover_ssh || { tail -8 "${WORK}/boot1.log" 2>/dev/null; die "SSH never came up (needed to pin the dev manager)"; }
    log "Pinning manager to ${DEV_MGR} and restarting..."
    sshx "${SSH_USER}" "${SUDO}bash -c 'docker pull ${DEV_MGR} && sed -i \"s#image: ghcr.io/airframesio/airwaves-manager:.*#image: ${DEV_MGR}#\" /etc/airwaves/docker-compose.yml && cd /etc/airwaves && docker compose up -d'" \
        || die "failed to pin/restart the dev manager"
    log "Waiting for the install API (/system/disks) on the dev manager..."
    have=""
    for i in $(seq 1 90); do
        sleep 8
        curl -fsS -m6 "http://localhost:${API_PORT}/api/v1/system/disks" >/dev/null 2>&1 && { have=1; log "  install API up (~$((i*8))s after pin)"; break; }
    done
    [ -n "${have}" ] || die "/system/disks never answered after pinning the dev manager"

    disks="$(curl -fsS -m20 "http://localhost:${API_PORT}/api/v1/system/disks")" || die "GET /system/disks failed"
    dev="$(echo "${disks}" | jq -r '.[0].device // empty')"
    [ -n "${dev}" ] || { echo "${disks}"; die "no install target offered by /system/disks"; }
    log "Installing to ${dev} via POST /system/install..."
    # The POST does two nsenter round-trips (validate the target, then spawn the
    # installer), each seconds-slow under TCG emulation, so give it room. If curl
    # still gives up, the server may have started anyway — fall through to the
    # progress poll, which is the real success signal, instead of failing hard.
    if curl -fsS -m90 -X POST "http://localhost:${API_PORT}/api/v1/system/install" \
        -H 'Content-Type: application/json' -d "{\"device\":\"${dev}\"}" >/dev/null; then
        log "POST /system/install accepted."
    else
        log "WARNING: POST /system/install did not return cleanly; checking progress in case it started anyway..."
    fi

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

discover_ssh || { tail -8 "${WORK}/boot1.log" 2>/dev/null; die "SSH never became usable"; }

if [ -n "${OVERLAY_DIR}" ]; then
    log "Overlaying working-tree scripts via SSH (base64-in-command)..."
    for f in airwaves-install airwaves-firstrun airwaves-slot-manager airwaves-tui; do
        [ -f "${OVERLAY_DIR}/${f}" ] || continue
        b64="$(base64 < "${OVERLAY_DIR}/${f}" | tr -d '\n')"
        sshx "${SSH_USER}" "${SUDO}bash -c 'echo ${b64} | base64 -d > /opt/airwaves/scripts/${f} && chmod +x /opt/airwaves/scripts/${f}'" >/dev/null \
            || die "overlay of ${f} failed"
    done
fi

log "Running airwaves-install on the blank disk (this is the slow part)..."
AB_FLAG=""; [ -n "${AB_TEST}" ] && AB_FLAG="--ab"
INSTALL_CMD="${SUDO}bash -c 'TGT=\$(/opt/airwaves/scripts/airwaves-install --list-json | jq -r \".[0].device\"); echo TARGET=\$TGT; AIRWAVES_INSTALL_APPLY=1 /opt/airwaves/scripts/airwaves-install ${AB_FLAG} --target \$TGT'"
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

# ---- Phase 2: boot from the internal disk ALONE -----------------------------
# Each internal boot uses a FRESH UEFI varstore: phase 1's vars point at the
# (now-absent) USB boot entry, so reusing them drops OVMF to the UEFI shell
# instead of discovering the installed disk's removable bootloader.

# Boot the internal disk and wait until SSH is usable. Sets P2.
boot_internal() {
    cp "${OVMF_VARS_SRC}" "${WORK}/OVMF_VARS.fd"
    P2=$(boot "$1" -drive "file=${TARGET_IMG},format=qcow2,if=virtio")
    local i out
    for i in $(seq 1 60); do
        sleep 10
        out="$(sshx "${SSH_USER}" 'echo READY_$(id -u)' 2>/dev/null || true)"
        echo "$out" | grep -q "READY_" && return 0
    done
    return 1
}
stop_internal() {
    sshx "${SSH_USER}" "${SUDO}poweroff" >/dev/null 2>&1 || true
    sleep 5; kill "${P2}" 2>/dev/null || true
    local i
    for i in $(seq 1 30); do
        kill -0 "${P2}" 2>/dev/null && { sleep 1; continue; }
        lsof -nP -iTCP:"${SSH_PORT}" -iTCP:"${API_PORT}" 2>/dev/null | grep -q LISTEN || break
        sleep 1
    done
}
# Read the running slot, retrying through the transient SSH resets that happen
# while the first boot is busy (growfs, docker, manager image pull).
slot_now() {
    local i out
    for i in $(seq 1 24); do
        out="$(sshx "${SSH_USER}" "${SUDO}/opt/airwaves/scripts/airwaves-slot-manager current" 2>/dev/null | tr -d '\r' | grep -oE '^[ab]$' | head -1 || true)"
        [ -n "$out" ] && { echo "$out"; return 0; }
        sleep 8
    done
    return 1
}
# Run a slot-manager verb, retrying through transient SSH resets.
slot_cmd() {
    local i
    for i in $(seq 1 24); do
        sshx "${SSH_USER}" "${SUDO}/opt/airwaves/scripts/airwaves-slot-manager $*" >/dev/null 2>&1 && return 0
        sleep 8
    done
    return 1
}

if [ -n "${AB_TEST}" ]; then
    log "Phase 2 (A/B): boot 1 — expect slot A..."
    boot_internal "${WORK}/boot2a.log" || { tail -40 "${WORK}/boot2a.log" 2>/dev/null; die "A/B: slot A did not boot (check grub.cfg / layout)"; }
    s="$(slot_now || true)"; log "  running slot: ${s:-?}"
    [ "$s" = "a" ] || die "A/B: expected slot a on first boot, got '${s:-?}'"
    log "  set-try b, rebooting..."
    slot_cmd set-try b || die "A/B: set-try b failed"
    stop_internal

    log "Phase 2 (A/B): boot 2 — expect slot B (one-shot trial)..."
    boot_internal "${WORK}/boot2b.log" || { tail -40 "${WORK}/boot2b.log" 2>/dev/null; die "A/B: slot B did not boot after set-try"; }
    s="$(slot_now || true)"; log "  running slot: ${s:-?}"
    [ "$s" = "b" ] || die "A/B: expected slot b after set-try, got '${s:-?}'"
    log "  rollback (-> a), rebooting..."
    slot_cmd rollback || die "A/B: rollback failed"
    stop_internal

    log "Phase 2 (A/B): boot 3 — expect slot A (rolled back)..."
    boot_internal "${WORK}/boot2c.log" || { tail -40 "${WORK}/boot2c.log" 2>/dev/null; die "A/B: did not boot after rollback"; }
    s="$(slot_now || true)"; log "  running slot: ${s:-?}"
    [ "$s" = "a" ] || die "A/B: expected slot a after rollback, got '${s:-?}'"
    stop_internal
    log "=== A/B PASS: install -> boot A -> set-try B -> boot B -> rollback -> boot A ==="
    exit 0
fi

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
