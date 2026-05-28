#!/bin/bash
#
# Airwaves OS - Base Extension
# Handles: identity, users, directories, MOTD, config structure
#

function extension_prepare_config__airwaves_base() {
	display_alert "Preparing Airwaves OS base configuration" "${EXTENSION}" "info"
}

function user_config__airwaves_base_packages() {
	display_alert "Adding base packages for Airwaves OS" "${EXTENSION}" "info"
	add_packages_to_rootfs sudo bash-completion curl wget jq toilet ca-certificates gnupg lsb-release
}

function pre_install_kernel_debs__copy_airwaves_files() {
	display_alert "Copying Airwaves OS files to image" "${EXTENSION}" "info"

	# Create directory structure
	run_host_command_logged mkdir -p "${SDCARD}"/opt/airwaves/{scripts,images}
	run_host_command_logged mkdir -p "${SDCARD}"/etc/airwaves

	# Copy extension data (config, scripts, templates)
	local extension_data_dir="${SRC}/userpatches/extensions/airwaves-os"
	if [ -d "${extension_data_dir}" ]; then
		run_host_command_logged cp -aR "${extension_data_dir}/config" "${SDCARD}"/opt/airwaves/
		run_host_command_logged cp -aR "${extension_data_dir}/scripts" "${SDCARD}"/opt/airwaves/
		run_host_command_logged chmod -R +x "${SDCARD}"/opt/airwaves/scripts
	fi
}

function post_family_tweaks__airwaves_base_setup() {
	display_alert "Installing Airwaves OS base system" "${EXTENSION}" "info"

	# Create airwaves user (docker group added later by airwaves-docker extension)
	display_alert "Creating airwaves user" "${EXTENSION}" "info"
	chroot_sdcard useradd -m -s /bin/bash -G sudo,plugdev airwaves
	# Set password using chpasswd via heredoc (avoids pipe parsing issues in chroot)
	echo "airwaves:airwaves" | chroot "${SDCARD}" /usr/sbin/chpasswd

	# Install MOTD
	display_alert "Installing MOTD" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/15-airwaves-header "${SDCARD}"/etc/update-motd.d/
	run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/15-airwaves-header
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/50-airwaves-help "${SDCARD}"/etc/update-motd.d/
	run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/50-airwaves-help

	# Install configuration
	display_alert "Installing configuration files" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/config.json.template "${SDCARD}"/etc/airwaves/config.json
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/airwaves-release "${SDCARD}"/etc/airwaves-release
	# Stamp build metadata
	chroot_sdcard sed -i "s/^AIRWAVES_BUILD_DATE=.*/AIRWAVES_BUILD_DATE=$(date -u +%Y-%m-%dT%H:%M:%SZ)/" /etc/airwaves-release
	chroot_sdcard sed -i "s/^AIRWAVES_BUILD_BOARD=.*/AIRWAVES_BUILD_BOARD=${BOARD}/" /etc/airwaves-release

	# Install app catalog
	display_alert "Installing app catalog" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/catalog.json "${SDCARD}"/etc/airwaves/catalog.json

	# Seed config-version markers + updater state directory. These track the
	# installed compose/catalog revision so the updater can detect changes.
	display_alert "Seeding updater state" "${EXTENSION}" "info"
	run_host_command_logged mkdir -p "${SDCARD}"/etc/airwaves/update
	echo '{"compose": 1, "catalog": 1, "channel": "stable"}' > "${SDCARD}"/etc/airwaves/.versions.json

	# Install system updater service (triggered on demand by the manager,
	# not enabled at boot).
	display_alert "Installing airwaves-update service" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-update.service "${SDCARD}"/etc/systemd/system/airwaves-update.service

	# Mark as needing first run
	run_host_command_logged touch "${SDCARD}"/opt/airwaves/.needs-first-run

	# Install init service
	display_alert "Installing airwaves-init service" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-init.service "${SDCARD}"/etc/systemd/system/airwaves-init.service
	chroot_sdcard systemctl --no-reload enable airwaves-init.service
}
