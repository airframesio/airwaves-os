#!/bin/bash
#
# Airwaves OS - Networking Extension
# Handles: NetworkManager (wifi + ethernet), avahi/mDNS
#
# We use NetworkManager as the single network backend so the manager API and the
# console rescue TUI can both drive Wi-Fi through one tool (nmcli). systemd-networkd
# is masked to avoid a split-brain with NM. The *-wait-online units are masked too:
# blocking boot until the network is up adds a ~1-2 min delay when a device has no
# carrier/DHCP (the "first boot takes forever before the console" symptom), and the
# appliance must come up fast even with no network.
#

function extension_prepare_config__airwaves_networking() {
	display_alert "Preparing Airwaves OS networking" "${EXTENSION}" "info"
}

function user_config__airwaves_networking_packages() {
	display_alert "Adding networking packages" "${EXTENSION}" "info"
	# network-manager pulls wpasupplicant (wifi auth). iw is handy for diagnostics.
	add_packages_to_rootfs network-manager iw avahi-daemon avahi-utils libnss-mdns
}

function post_family_tweaks__airwaves_networking_setup() {
	display_alert "Configuring Airwaves OS networking" "${EXTENSION}" "info"

	# --- NetworkManager as the single backend -------------------------------
	display_alert "Enabling NetworkManager" "${EXTENSION}" "info"
	chroot_sdcard systemctl --no-reload enable NetworkManager.service

	# Mask systemd-networkd so it can't fight NM over the interfaces. Mask the
	# wait-online units (both stacks) so boot never blocks on the network.
	chroot_sdcard systemctl --no-reload mask \
		systemd-networkd.service \
		systemd-networkd.socket \
		systemd-networkd-wait-online.service \
		NetworkManager-wait-online.service || true

	# Drop any Armbian-provided networkd interface configs so a future unmask
	# can't reapply them behind NM's back.
	run_host_command_logged rm -f "${SDCARD}"/etc/systemd/network/*.network 2>/dev/null || true

	# Keep NM's hands off docker's interfaces (docker0, veth*, br-* compose
	# bridges) so container networking is never disrupted by NM.
	run_host_command_logged mkdir -p "${SDCARD}/etc/NetworkManager/conf.d"
	cat > "${SDCARD}/etc/NetworkManager/conf.d/10-airwaves-unmanaged.conf" << 'EOF'
# Airwaves OS: let Docker own its own interfaces; NM manages only real NICs.
[keyfile]
unmanaged-devices=interface-name:docker*;interface-name:veth*;interface-name:br-*;interface-name:vmnet*
EOF

	# --- avahi / mDNS --------------------------------------------------------
	# The hostname is set dynamically by airwaves-init (airwaves-XXXXXX); avahi
	# advertises <hostname>.local so the web UI is discoverable.
	display_alert "Configuring avahi (mDNS)" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/avahi-airwaves-web.service \
		"${SDCARD}"/etc/avahi/services/airwaves-web.service
	chroot_sdcard systemctl --no-reload enable avahi-daemon.service

	# Let avahi own .local; tell systemd-resolved (if present) to stay out of it.
	if [ -d "${SDCARD}/etc/systemd/resolved.conf.d" ] || mkdir -p "${SDCARD}/etc/systemd/resolved.conf.d"; then
		cat > "${SDCARD}/etc/systemd/resolved.conf.d/airwaves.conf" << 'EOF'
[Resolve]
# Let avahi handle .local mDNS
MulticastDNS=no
EOF
	fi

	# Classic interface names (eth0/wlan0) instead of predictable ones (wlp1s0,
	# enp1s0, …) so detection + config are consistent across hardware. x86 boots
	# via GRUB (append to /etc/default/grub.d, sourced by grub-mkconfig); ARM
	# boards read armbianEnv.txt (extraargs).
	display_alert "Forcing classic interface names" "net.ifnames=0 biosdevname=0" "info"
	run_host_command_logged mkdir -p "${SDCARD}/etc/default/grub.d"
	cat > "${SDCARD}/etc/default/grub.d/99-airwaves-cmdline.cfg" << 'EOF'
# Airwaves OS: classic interface names (eth0/wlan0). Appended to the value the
# Armbian grub extension sets in 98-armbian.cfg.
GRUB_CMDLINE_LINUX_DEFAULT="${GRUB_CMDLINE_LINUX_DEFAULT} net.ifnames=0 biosdevname=0"
EOF
	if [ -f "${SDCARD}/boot/armbianEnv.txt" ]; then
		if grep -q '^extraargs=' "${SDCARD}/boot/armbianEnv.txt"; then
			sed -i 's/^extraargs=\(.*\)$/extraargs=\1 net.ifnames=0/' "${SDCARD}/boot/armbianEnv.txt"
		else
			echo 'extraargs=net.ifnames=0' >> "${SDCARD}/boot/armbianEnv.txt"
		fi
	fi

	display_alert "Networking configured (NetworkManager + avahi)" "${EXTENSION}" "info"
}
