#!/bin/bash
#
# Airwaves OS - Networking Extension
# Handles: systemd-networkd, avahi/mDNS, WiFi support
#

function extension_prepare_config__airwaves_networking() {
	display_alert "Preparing Airwaves OS networking" "${EXTENSION}" "info"
}

function user_config__airwaves_networking_packages() {
	display_alert "Adding networking packages" "${EXTENSION}" "info"
	add_packages_to_rootfs avahi-daemon avahi-utils libnss-mdns
}

function post_family_tweaks__airwaves_networking_setup() {
	display_alert "Configuring Airwaves OS networking" "${EXTENSION}" "info"

	# Configure avahi for mDNS discovery
	# The hostname will be set dynamically by airwaves-init (airwaves-XXXXXX)
	# Avahi will automatically advertise <hostname>.local

	# Install avahi service definitions for web UI discovery
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/avahi-airwaves-web.service \
		"${SDCARD}"/etc/avahi/services/airwaves-web.service

	# Enable avahi-daemon
	chroot_sdcard systemctl --no-reload enable avahi-daemon.service

	# Configure systemd-resolved to not conflict with avahi
	# (on minimal builds, systemd-networkd is the default)
	if [ -d "${SDCARD}/etc/systemd/resolved.conf.d" ] || mkdir -p "${SDCARD}/etc/systemd/resolved.conf.d"; then
		cat > "${SDCARD}/etc/systemd/resolved.conf.d/airwaves.conf" << 'EOF'
[Resolve]
# Let avahi handle .local mDNS
MulticastDNS=no
EOF
	fi

	display_alert "Networking configured (systemd-networkd + avahi)" "${EXTENSION}" "info"
}
