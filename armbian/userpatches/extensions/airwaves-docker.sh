#!/bin/bash
#
# Airwaves OS - Docker Extension
# Handles: Docker CE installation, container pre-loading, compose setup
#
# Note: Armbian v25.02 does not include a docker-ce extension.
# We install Docker directly via the official install script during
# the post_family_tweaks phase.
#

function extension_prepare_config__airwaves_docker() {
	display_alert "Preparing Airwaves OS Docker configuration" "${EXTENSION}" "info"
}

function user_config__airwaves_docker_packages() {
	display_alert "Adding Docker prerequisite packages" "${EXTENSION}" "info"
	add_packages_to_rootfs ca-certificates curl gnupg lsb-release apt-transport-https
}

function post_family_tweaks__airwaves_docker_setup() {
	display_alert "Installing Docker CE" "${EXTENSION}" "info"

	# Install Docker CE using the official convenience script
	# This runs inside the chroot during image build
	chroot_sdcard bash -c 'curl -fsSL https://get.docker.com | sh' || {
		display_alert "Docker install via get.docker.com failed, trying manual method" "${EXTENSION}" "warn"

		# Fallback: manual Docker repo setup
		chroot_sdcard bash -c '
			install -m 0755 -d /etc/apt/keyrings
			curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
			chmod a+r /etc/apt/keyrings/docker.asc
			echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian $(. /etc/os-release && echo "$VERSION_CODENAME") stable" > /etc/apt/sources.list.d/docker.list
			apt-get update
			apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
		'
	}

	# Enable Docker service
	chroot_sdcard systemctl --no-reload enable docker.service
	chroot_sdcard systemctl --no-reload enable containerd.service

	# Add airwaves user to docker group (group created by Docker install)
	chroot_sdcard usermod -aG docker airwaves || true

	display_alert "Setting up Airwaves OS Docker infrastructure" "${EXTENSION}" "info"

	# Install docker-compose configuration
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/docker-compose.yml.template \
		"${SDCARD}"/etc/airwaves/docker-compose.yml

	# Install container management service
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-containers.service \
		"${SDCARD}"/etc/systemd/system/airwaves-containers.service
	chroot_sdcard systemctl --no-reload enable airwaves-containers.service

	display_alert "Docker infrastructure configured" "${EXTENSION}" "info"
}
