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
	add_packages_to_rootfs sudo bash-completion curl wget jq toilet ca-certificates gnupg lsb-release cloud-guest-utils
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

	# Bake pre-built container images so first boot needs NO network. The CI
	# image-build workflow docker-save's the manager+gateway images of the release
	# pinned in releases/<channel>.json into ${extension_data_dir}/images/*.tar
	# (matching this board's CPU arch). airwaves-init loads them on first boot
	# before falling back to a registry pull. If none are staged (e.g. a local
	# dev build), this is a no-op and the device pulls online on first boot.
	if compgen -G "${extension_data_dir}/images/*.tar" >/dev/null 2>&1; then
		display_alert "Baking pre-built container images" \
			"$(ls -1 "${extension_data_dir}"/images/*.tar | wc -l | tr -d ' ') tarball(s)" "info"
		run_host_command_logged cp -a "${extension_data_dir}"/images/*.tar "${SDCARD}"/opt/airwaves/images/
		run_host_command_logged ls -lh "${SDCARD}/opt/airwaves/images/"
	else
		display_alert "No pre-built container images staged" \
			"first boot will pull from registry (needs network)" "wrn"
	fi
}

function post_family_tweaks__airwaves_base_setup() {
	display_alert "Installing Airwaves OS base system" "${EXTENSION}" "info"

	# Create airwaves user (docker group added later by airwaves-docker extension)
	display_alert "Creating airwaves user" "${EXTENSION}" "info"
	chroot_sdcard useradd -m -s /bin/bash -G sudo,plugdev airwaves || \
		chroot_sdcard usermod -s /bin/bash airwaves
	# Guarantee the home directory exists, is populated from skel, and is owned by
	# airwaves — some build paths leave `useradd -m` without a home, which then
	# breaks SSH login ("Could not chdir to home directory /home/airwaves").
	chroot_sdcard mkdir -p /home/airwaves
	chroot_sdcard cp -rTn /etc/skel /home/airwaves 2>/dev/null || true
	chroot_sdcard chown -R airwaves:airwaves /home/airwaves
	chroot_sdcard chmod 755 /home/airwaves
	# Set password using chpasswd via heredoc (avoids pipe parsing issues in chroot)
	echo "airwaves:airwaves" | chroot "${SDCARD}" /usr/sbin/chpasswd

	# Disable Armbian's interactive first-login wizard. The airwaves user is
	# already fully provisioned at build time (password + sudo/plugdev/docker),
	# and root has ROOTPWD, so first boot is unattended. (post_family_tweaks
	# runs after Armbian touches /root/.not_logged_in_yet.)
	display_alert "Disabling first-login wizard" "${EXTENSION}" "info"
	run_host_command_logged rm -f "${SDCARD}/root/.not_logged_in_yet"

	# Install MOTD
	display_alert "Installing MOTD" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/15-airwaves-header "${SDCARD}"/etc/update-motd.d/
	run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/15-airwaves-header
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/50-airwaves-help "${SDCARD}"/etc/update-motd.d/
	run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/50-airwaves-help

	# Disable Armbian's own header banner so only the Airwaves OS header shows
	# (our 15-airwaves-header uses a distinct MOTD key, so it stays enabled).
	if [ -f "${SDCARD}/etc/default/armbian-motd" ]; then
		run_host_command_logged sed -i 's/^MOTD_DISABLE=.*/MOTD_DISABLE="clear header"/' "${SDCARD}/etc/default/armbian-motd"
	else
		echo 'MOTD_DISABLE="clear header"' > "${SDCARD}/etc/default/armbian-motd"
	fi

	# Install configuration
	display_alert "Installing configuration files" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/config.json.template "${SDCARD}"/etc/airwaves/config.json
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/airwaves-release "${SDCARD}"/etc/airwaves-release
	# Stamp build metadata
	chroot_sdcard sed -i "s/^AIRWAVES_BUILD_DATE=.*/AIRWAVES_BUILD_DATE=$(date -u +%Y-%m-%dT%H:%M:%SZ)/" /etc/airwaves-release
	chroot_sdcard sed -i "s/^AIRWAVES_BUILD_BOARD=.*/AIRWAVES_BUILD_BOARD=${BOARD}/" /etc/airwaves-release
	# Stamp the real Airwaves OS version from the manager crate. The release
	# template ships a placeholder (1.0.0); without this the MOTD + console show
	# a stale version. The repo lives one level above Armbian's SRC (.armbian-build
	# sits in the repo root), and userpatches symlinks back to the repo — try the
	# likely anchors, then fall back to a search.
	local _aw_ver _aw_cn _ct
	for _ct in \
		"${SRC:-}/../containers/airwaves-manager/Cargo.toml" \
		"$(readlink -f "${SRC:-}/userpatches" 2>/dev/null | sed 's#/armbian/userpatches$##')/containers/airwaves-manager/Cargo.toml" \
		"$(dirname "${BASH_SOURCE[0]}")/../../../containers/airwaves-manager/Cargo.toml"; do
		[ -f "${_ct}" ] || continue
		_aw_ver="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "${_ct}" 2>/dev/null | head -1)"
		[ -n "${_aw_ver}" ] && break
	done
	if [ -z "${_aw_ver}" ]; then
		_ct="$(find "${SRC:-.}/.." -maxdepth 4 -path '*containers/airwaves-manager/Cargo.toml' 2>/dev/null | head -1)"
		[ -n "${_ct}" ] && _aw_ver="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "${_ct}" 2>/dev/null | head -1)"
	fi
	if [ -n "${_aw_ver}" ]; then
		display_alert "Stamping Airwaves OS version" "${_aw_ver}" "info"
		sed -i "s/^AIRWAVES_VERSION=.*/AIRWAVES_VERSION=${_aw_ver}/" "${SDCARD}/etc/airwaves-release"
	else
		display_alert "Airwaves OS version" "could not resolve from Cargo.toml; keeping placeholder" "wrn"
	fi
	_aw_cn="$(awk -F= '/^AIRWAVES_CODENAME=/{gsub(/"/,"",$2);print $2}' "${SDCARD}/etc/airwaves-release" 2>/dev/null)"

	# Pre-login console banner (/etc/issue): shown by agetty before login/autologin
	# so the console isn't blank during first-boot work. \n=hostname \l=tty (agetty).
	cat > "${SDCARD}/etc/issue" <<ISSUE

   ===  A I R W A V E S   O S  ===

   Airwaves OS v${_aw_ver:-1.0.0} (${_aw_cn:-})
   Radio software that just works.

   \n  ·  \l

ISSUE

	# (GRUB menu is branded "Airwaves OS v<version>" via UEFI_GRUB_DISTRO_NAME in
	# common-airwaves.conf — Armbian's grub extension runs after this and would
	# overwrite /etc/default/grub anyway.)

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

	# Install + enable filesystem auto-grow service. Runs every boot and expands
	# the root partition/filesystem to fill the disk (idempotent), so VM users
	# can just enlarge the virtual disk and reboot.
	display_alert "Installing airwaves-growfs service" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-growfs.service "${SDCARD}"/etc/systemd/system/airwaves-growfs.service
	chroot_sdcard systemctl --no-reload enable airwaves-growfs.service

	# Mark as needing first run
	run_host_command_logged touch "${SDCARD}"/opt/airwaves/.needs-first-run

	# Install init service
	display_alert "Installing airwaves-init service" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-init.service "${SDCARD}"/etc/systemd/system/airwaves-init.service
	chroot_sdcard systemctl --no-reload enable airwaves-init.service

	# Install pre-config service (applies the AWCFG config partition on first
	# setup, or when an apply.conf trigger is present — runs before init). The
	# airwaves-preconfig script itself is copied with the rest of /opt/airwaves/
	# scripts above.
	display_alert "Installing airwaves-preconfig service" "${EXTENSION}" "info"
	run_host_command_logged cp "${SDCARD}"/opt/airwaves/config/templates/systemd-airwaves-preconfig.service "${SDCARD}"/etc/systemd/system/airwaves-preconfig.service
	chroot_sdcard systemctl --no-reload enable airwaves-preconfig.service
}

# Overlay the pre-built container tarballs onto the FINAL image at assembly time,
# after the rootfs is laid into the mounted image (${MOUNT}) and before unmount.
# pre_install_kernel_debs (above) already bakes them into the rootfs for a fresh
# build, but the production split build (build-os-image.yml) can reuse a CACHED
# rootfs artifact that predates a container bump — the generated tarballs aren't
# part of Armbian's content hash. Re-copying them here guarantees the shipped
# image always carries the current offline containers regardless of caching.
# Fully guarded: a no-op when none are staged, and never fails the build.
function pre_umount_final_image__airwaves_bake_container_images() {
	local src_images="${SRC}/userpatches/extensions/airwaves-os/images"
	if ! compgen -G "${src_images}/*.tar" >/dev/null 2>&1; then
		return 0
	fi
	display_alert "Overlaying pre-built container images into image" \
		"$(ls -1 "${src_images}"/*.tar | wc -l | tr -d ' ') tarball(s)" "info"
	mkdir -p "${MOUNT}/opt/airwaves/images" 2>/dev/null || true
	if cp -a "${src_images}"/*.tar "${MOUNT}/opt/airwaves/images/" 2>/dev/null; then
		run_host_command_logged ls -lh "${MOUNT}/opt/airwaves/images/" || true
	else
		display_alert "Could not overlay container tarballs" "non-fatal; device will pull on first boot" "wrn"
	fi
	return 0
}
