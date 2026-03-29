#!/bin/bash
#
# Airwaves OS - Docker Extension
# Handles: Docker CE installation, container pre-loading, compose setup
#

function extension_prepare_config__airwaves_docker() {
	display_alert "Preparing Airwaves OS Docker configuration" "${EXTENSION}" "info"
	# Enable the docker-ce extension as a dependency
	enable_extension "docker-ce"
}

function user_config__airwaves_docker_packages() {
	display_alert "Adding Docker-related packages" "${EXTENSION}" "info"
	add_packages_to_rootfs docker-compose-plugin
}

function post_family_tweaks__airwaves_docker_setup() {
	display_alert "Setting up Airwaves OS Docker infrastructure" "${EXTENSION}" "info"

	# Install docker-compose configuration
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/docker-compose.yml.template \
		"${SDCARD}"/etc/airwaves/docker-compose.yml

	# Install container management service
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-containers.service \
		"${SDCARD}"/etc/systemd/system/airwaves-containers.service
	chroot_sdcard systemctl --no-reload enable airwaves-containers.service

	# Pre-baked container images will be placed in /opt/airwaves/images/
	# by the CI/CD pipeline. The airwaves-init script loads them on first boot.
	# If no pre-baked images exist, containers will be pulled on first boot.
	display_alert "Docker infrastructure configured" "${EXTENSION}" "info"
}
