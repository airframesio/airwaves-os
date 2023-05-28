## inspired from rpardini's docker-ce extension
# https://github.com/rpardini/armbian-build/blob/extensions/userpatches/extensions/docker-ce.sh

function extension_prepare_config__tailscale() {
	display_alert "Target image will have Tailscale Inc. preinstalled" "${EXTENSION}" "info"
}

function pre_customize_image__add_tailscale_to_image() {
	display_alert "Adding Tailscale Inc. package for release ${RELEASE}" "${EXTENSION}" "info"

	# Add gpg-key... Updated for Jammy, does not use apt-key.
	display_alert "Adding gpg-key for Tailscale Inc." "${EXTENSION}" "info"
	run_host_command_logged mkdir -pv "${SDCARD}"/usr/share/keyrings
	run_host_command_logged curl --max-time 60 -4 -fsSL "https://pkgs.tailscale.com/stable/ubuntu/jammy.noarmor.gpg" "|" gpg --dearmor -o "${SDCARD}"/usr/share/keyrings/tailscale.gpg

	# Add sources.list
	if [[ "${DISTRIBUTION}" == "Debian" ]]; then
		display_alert "Adding sources.list for Tailscale Inc." "Debian :: ${EXTENSION}" "info"
		run_host_command_logged echo "deb [arch=${ARCH} signed-by=/usr/share/keyrings/tailscale.gpg] https://pkgs.tailscale.com/stable/debian ${RELEASE} stable" "|" tee "${SDCARD}"/etc/apt/sources.list.d/tailscale.list
	elif [[ "${DISTRIBUTION}" == "Ubuntu" ]]; then
		display_alert "Adding sources.list for Tailscale Inc." "Ubuntu :: ${EXTENSION}" "info"
		run_host_command_logged echo "deb [arch=${ARCH} signed-by=/usr/share/keyrings/tailscale.gpg] https://pkgs.tailscale.com/stable/ubuntu ${RELEASE} stable" "|" tee "${SDCARD}"/etc/apt/sources.list.d/tailscale.list
	else
		exit_with_error "Unknown distribution: ${DISTRIBUTION}"
	fi

	display_alert "Updating package lists with Tailscale Inc. repos" "${EXTENSION}" "info"
	do_with_retries 3 chroot_sdcard_apt_get_update

	display_alert "Installing Tailscale Inc. packages" "${EXTENSION}: 'tailscale' et al" "info"
	chroot_sdcard_apt_get_install tailscale
}
