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
	add_packages_to_rootfs parted gdisk dosfstools e2fsprogs rsync dialog

	# x86 UEFI install needs GRUB for amd64. ARM boards use u-boot (out of scope
	# for the wizard for now), so only add GRUB on amd64.
	if [[ "${ARCH}" == "amd64" ]]; then
		display_alert "Adding GRUB EFI packages for x86 installer" "${EXTENSION}" "info"
		add_packages_to_rootfs grub-efi-amd64 grub-efi-amd64-bin grub2-common efibootmgr
	fi
}

function post_family_tweaks__airwaves_installer_setup() {
	display_alert "Enabling Airwaves first-run console wizard" "${EXTENSION}" "info"

	# The first-run wizard unit (scripts are copied by airwaves-base's
	# pre_install_kernel_debs hook, which copies the whole scripts/ dir).
	if [ -f "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-firstrun.service ]; then
		run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-firstrun.service \
			"${SDCARD}"/etc/systemd/system/airwaves-firstrun.service
		chroot_sdcard systemctl --no-reload enable airwaves-firstrun.service
	fi
}
