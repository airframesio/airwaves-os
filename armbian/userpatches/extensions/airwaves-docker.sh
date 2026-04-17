#!/bin/bash
#
# Airwaves OS - Docker Extension
# Handles: Docker CE installation, container pre-loading, compose setup
#

function extension_prepare_config__airwaves_docker() {
	display_alert "Preparing Airwaves OS Docker configuration" "${EXTENSION}" "info"
}

function user_config__airwaves_docker_packages() {
	display_alert "Adding Docker prerequisite packages" "${EXTENSION}" "info"
	add_packages_to_rootfs ca-certificates curl gnupg apt-transport-https
}

function post_family_tweaks__airwaves_docker_setup() {
	display_alert "Installing Docker CE" "${EXTENSION}" "info"

	# Add Docker's official GPG key and repository directly (no pipes through chroot_sdcard)
	run_host_command_logged mkdir -p "${SDCARD}/etc/apt/keyrings"

	# Determine distro for Docker repo (debian or ubuntu)
	local distro_id
	distro_id=$(chroot "${SDCARD}" /bin/bash -c '. /etc/os-release && echo $ID')
	local distro_codename
	distro_codename=$(chroot "${SDCARD}" /bin/bash -c '. /etc/os-release && echo $VERSION_CODENAME')
	local arch
	arch=$(chroot "${SDCARD}" dpkg --print-architecture)

	display_alert "Docker repo: ${distro_id} ${distro_codename} ${arch}" "${EXTENSION}" "info"

	# Download Docker GPG key
	curl -fsSL "https://download.docker.com/linux/${distro_id}/gpg" -o "${SDCARD}/etc/apt/keyrings/docker.asc"
	chmod a+r "${SDCARD}/etc/apt/keyrings/docker.asc"

	# Add Docker apt repository
	echo "deb [arch=${arch} signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/${distro_id} ${distro_codename} stable" \
		> "${SDCARD}/etc/apt/sources.list.d/docker.list"

	# Install Docker packages
	chroot_sdcard apt-get update
	chroot_sdcard apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin

	# Enable Docker service
	chroot_sdcard systemctl --no-reload enable docker.service
	chroot_sdcard systemctl --no-reload enable containerd.service

	# Add airwaves user to docker group
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
