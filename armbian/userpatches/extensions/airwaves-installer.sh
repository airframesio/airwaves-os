#!/bin/bash
#
# Airwaves OS - Installer Extension
# Adds the dependencies airwaves-install needs (partitioning, filesystems,
# bootloader) and enables the first-run console wizard shown on the live USB.
#

function extension_prepare_config__airwaves_installer() {
	display_alert "Preparing Airwaves OS installer extension" "${EXTENSION}" "info"
}

function user_config__airwaves_installer_packages() {
	display_alert "Adding installer packages for Airwaves OS" "${EXTENSION}" "info"
	# Disk + filesystem + UI tooling used by airwaves-install / airwaves-firstrun.
	# (rsync, jq, util-linux are pulled in elsewhere/by base; listed where needed.)
	# These cover all platforms: ARM u-boot boards write the bootloader via the
	# board's existing u-boot package (write_uboot_platform + dd), and Raspberry
	# Pi stages its FAT firmware partition with dosfstools — no extra packages.
	add_packages_to_rootfs parted gdisk dosfstools e2fsprogs rsync dialog

	# Only x86 UEFI needs GRUB. ARM boards boot via u-boot (raw sectors) or the
	# Pi GPU firmware, so GRUB is added on amd64 only.
	if [[ "${ARCH}" == "amd64" ]]; then
		display_alert "Adding GRUB EFI packages for x86 installer" "${EXTENSION}" "info"
		add_packages_to_rootfs grub-efi-amd64 grub-efi-amd64-bin grub2-common efibootmgr
	fi
}

function post_family_tweaks__airwaves_installer_setup() {
	display_alert "Setting up Airwaves console TUI on tty1" "${EXTENSION}" "info"

	# Console model: a persistent TUI (airwaves-tui) owns tty1 via a getty
	# AUTOLOGIN session, not a oneshot wizard. The old firstrun oneshot
	# Conflicts=getty@tty1 + TTYVHangup left tty1 DEAD after the user chose "Run
	# live" (no shell). getty's respawn now guarantees tty1 always has the TUI;
	# if it ever exits, getty relaunches it. (Scripts are copied to
	# /opt/airwaves/scripts by airwaves-base's pre_install hook.)
	run_host_command_logged chmod +x "${SDCARD}"/opt/airwaves/scripts/airwaves-tui || true

	# getty@tty1 autologin drop-in.
	run_host_command_logged mkdir -p "${SDCARD}"/etc/systemd/system/getty@tty1.service.d
	if [ -f "${SDCARD}"/opt/airwaves/config/templates/getty-tty1-autologin.conf ]; then
		run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/getty-tty1-autologin.conf \
			"${SDCARD}"/etc/systemd/system/getty@tty1.service.d/airwaves-tui.conf
	fi

	# Launch the TUI from the tty1 login session only (serial/SSH get a shell).
	if [ -f "${SDCARD}"/opt/airwaves/config/profile.d-airwaves-tui.sh ]; then
		run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/profile.d-airwaves-tui.sh \
			"${SDCARD}"/etc/profile.d/zz-airwaves-tui.sh
	fi

	# Retire the old first-run oneshot service if it was ever installed (the TUI
	# replaces it; the airwaves-firstrun SCRIPT remains for the --install flow).
	run_host_command_logged rm -f "${SDCARD}"/etc/systemd/system/airwaves-firstrun.service
	chroot_sdcard systemctl --no-reload disable airwaves-firstrun.service 2>/dev/null || true

	# Put the installer on PATH so users can run `sudo airwaves-install` to get
	# the TUI installer (bare invocation) or drive it via flags. The manager
	# still calls the script by its absolute /opt path.
	run_host_command_logged mkdir -p "${SDCARD}"/usr/local/sbin
	run_host_command_logged ln -sf /opt/airwaves/scripts/airwaves-install \
		"${SDCARD}"/usr/local/sbin/airwaves-install
}
