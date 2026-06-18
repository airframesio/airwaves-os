#!/bin/bash
#
# Airwaves OS - Live USB extension (x86 only)
#
# Makes the rootfs capable of booting as a live system (Debian live-boot) so the
# CI can build a separate "Live USB" image whose whole OS loads into RAM (toram).
# Everything here is DORMANT on a normal/embedded boot (root=UUID): live-boot's
# initramfs scripts and the tmpfs-docker unit only activate when the kernel is
# booted with `boot=live`. The actual live image (squashfs + GPT + GRUB) is
# assembled by the workflow's "Build Live USB" step from this rootfs.
#
# On a live boot the root is an overlay (squashfs + RAM tmpfs), and Docker's
# overlay2 driver can't run on top of overlayfs — so we mount a RAM tmpfs at
# /var/lib/docker (live-only) and the stack loads its images into RAM.
#

function extension_prepare_config__airwaves_live() {
	[[ "${BOARD}" == "uefi-x86" ]] || return 0
	display_alert "Preparing Airwaves OS Live USB extension" "${EXTENSION}" "info"
}

# Host tools needed to assemble the live image on the runner.
function add_host_dependencies__airwaves_live() {
	[[ "${BOARD}" == "uefi-x86" ]] || return 0
	declare -g EXTRA_BUILD_DEPS="${EXTRA_BUILD_DEPS:-} squashfs-tools grub-efi-amd64-bin grub-pc-bin dosfstools e2fsprogs"
}

# live-boot in the rootfs makes the initramfs understand boot=live + toram. It is
# inert unless the kernel cmdline says boot=live, so the embedded image that
# ships the same rootfs is unaffected. (No live-config: Airwaves owns first-boot.)
function user_config__airwaves_live_packages() {
	[[ "${BOARD}" == "uefi-x86" ]] || return 0
	display_alert "Adding live-boot packages" "${EXTENSION}" "info"
	add_packages_to_rootfs live-boot live-boot-initramfs-tools
}

function post_family_tweaks__airwaves_live_setup() {
	[[ "${BOARD}" == "uefi-x86" ]] || return 0
	display_alert "Installing Live USB (toram) support" "${EXTENSION}" "info"

	# --- auto-RAM-gate initramfs hook ---------------------------------------
	# Runs from the initramfs during a live boot, BEFORE live-boot's 9990-main
	# (prefix 0050 < 9990). initramfs-tools SOURCES these scripts, so setting
	# TORAM here is seen by live-boot's copy logic. We enable toram only when
	# there's enough free RAM to hold the squashfs + overlay + the RAM docker
	# store, with headroom; otherwise we leave it running from USB. An explicit
	# `toram`/`notoram` on the cmdline always wins.
	local premount="${SDCARD}/usr/share/initramfs-tools/scripts/live-premount"
	run_host_command_logged mkdir -p "${premount}"
	cat > "${premount}/0050-airwaves-autotoram" <<'HOOK'
#!/bin/sh
# Airwaves OS: decide toram automatically based on available RAM vs squashfs size.
PREREQ=""
prereqs() { echo "$PREREQ"; }
case "$1" in prereqs) prereqs; exit 0 ;; esac

# An explicit choice on the cmdline always wins.
case " $(cat /proc/cmdline 2>/dev/null) " in
    *" toram "*|*" toram="*) TORAM="true";  export TORAM; exit 0 ;;
    *" notoram "*)           TORAM="";      export TORAM; exit 0 ;;
esac

# Find the live medium's recorded squashfs size (written at build time).
SQ_BYTES=0
for dev in /dev/sd*[0-9] /dev/nvme*n*p* /dev/mmcblk*p*; do
    [ -b "$dev" ] || continue
    mp="$(mktemp -d /run/aw-livescan.XXXXXX 2>/dev/null)" || continue
    if mount -t ext4 -o ro "$dev" "$mp" 2>/dev/null; then
        if [ -f "$mp/live/filesystem.size" ]; then
            SQ_BYTES="$(cat "$mp/live/filesystem.size" 2>/dev/null || echo 0)"
            umount "$mp" 2>/dev/null; rmdir "$mp" 2>/dev/null; break
        fi
        umount "$mp" 2>/dev/null
    fi
    rmdir "$mp" 2>/dev/null
done

MEM_KB="$(awk '/^MemAvailable:/{print $2; exit}' /proc/meminfo 2>/dev/null || echo 0)"
MEM_BYTES=$(( MEM_KB * 1024 ))
# Headroom for the overlay tmpfs + the RAM-backed /var/lib/docker the stack uses.
OVERHEAD=$(( 1024 * 1024 * 1024 ))

if [ "$SQ_BYTES" -gt 0 ] && [ "$MEM_BYTES" -gt $(( SQ_BYTES + OVERHEAD )) ]; then
    TORAM="true"; export TORAM
    echo "airwaves-autotoram: loading to RAM (MemAvailable=${MEM_KB}kB squashfs=${SQ_BYTES}B)"
else
    echo "airwaves-autotoram: running from USB (MemAvailable=${MEM_KB}kB squashfs=${SQ_BYTES}B)"
fi
exit 0
HOOK
	run_host_command_logged chmod +x "${premount}/0050-airwaves-autotoram"

	# --- live-only tmpfs for /var/lib/docker --------------------------------
	# On a live (overlay) root, Docker's overlay2 store can't live on overlayfs.
	# Mount a RAM tmpfs at /var/lib/docker so the stack runs in RAM. The condition
	# makes this a no-op on a normal/embedded boot (real disk for docker).
	cat > "${SDCARD}/etc/systemd/system/airwaves-live-docker-tmpfs.service" <<'UNIT'
[Unit]
Description=Airwaves OS live: RAM tmpfs for /var/lib/docker
Documentation=https://airwavesos.com
ConditionKernelCommandLine=boot=live
DefaultDependencies=no
After=local-fs.target
Before=docker.service airwaves-init.service airwaves-containers.service
RequiresMountsFor=/var/lib

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/mount -t tmpfs -o size=75%,mode=0710 tmpfs /var/lib/docker

[Install]
WantedBy=multi-user.target
UNIT
	chroot_sdcard systemctl --no-reload enable airwaves-live-docker-tmpfs.service
}
