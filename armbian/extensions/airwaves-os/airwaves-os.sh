#!/bin/bash

function extension_prepare_config__airwaves_os() {
  display_alert "Target image will have AirwavesOS preinstalled" "${EXTENSION}" "info"
}

function user_config__airwaves_os_extra_packages() {
  display_alert "Add additional debian packages for AirwaveOS dependencies" "${EXTENSION}" "info"
  add_packages_to_image armbian-config git golang avahi-daemon avahi-utils nala sudo
}
	
function pre_install_kernel_debs__add_aros_scripts() {
  display_alert "Copying airwaves-os scripts to image->/opt/aros" "${EXTENSION}" "info"

  run_host_command_logged mkdir -p "${SDCARD}"/opt/aros
  run_host_command_logged cp -aR "${EXTENSION_DIR}"/* "${SDCARD}"/opt/aros
  run_host_command_logged chmod -R +x "${SDCARD}"/opt/aros/scripts

}

function post_family_tweaks__install_airwaves_os_base() {

  display_alert "Installing AirwaveOS base" "${EXTENSION}" "info"

#  export LANG=C LC_ALL="en_US.UTF-8"
#  export DEBIAN_FRONTEND=noninteractive
#  export APT_LISTCHANGES_FRONTEND=none


  display_alert "install motd messages" "${EXTENSION}" "info"
  run_host_command_logged cp "${EXTENSION_DIR}"/config/15-aros-header "${SDCARD}"/etc/update-motd.d/
  run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/15-aros-header
  run_host_command_logged cp "${EXTENSION_DIR}"/config/50-aros-help "${SDCARD}"/etc/update-motd.d/
  run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/50-aros-help

  display_alert "install config files" "${EXTENSION}" "info"
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/bashrc-custom.template "${SDCARD}"/root/.bashrc
  run_host_command_logged mkdir -p "${SDCARD}"/etc/aros
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/aros-config.json.template "${SDCARD}"/etc/aros/config.json
  run_host_command_logged touch "${SDCARD}"/opt/aros/.needs-first-run
 
  display_alert "install systemd units" "${EXTENSION}" "info" 
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/avahi-aros.service.template "${SDCARD}"/etc/avahi/services/aros.service
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/systemd-aros-first-run.service.template "${SDCARD}"/etc/systemd/system/aros-first-run.service
  chroot_sdcard systemctl --no-reload enable aros-first-run.service
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/systemd-aros-manager.service.template "${SDCARD}"/etc/systemd/system/aros-manager.service
  chroot_sdcard systemctl --no-reload enable aros-manager.service

}

