#!/bin/bash

function extension_prepare_config__airwaves_os() {
  display_alert "Target image will have Airwaves OS preinstalled" "${EXTENSION}" "info"
}

function user_config__airwaves_os_extra_packages() {
  display_alert "Add additional debian packages for Airwaves OS dependencies" "${EXTENSION}" "info"
  add_packages_to_rootfs git golang avahi-daemon avahi-utils nala sudo
}

function pre_install_kernel_debs__add_airwaves_scripts() {
  display_alert "Copying airwaves-os scripts to image->/opt/airwaves" "${EXTENSION}" "info"

  run_host_command_logged mkdir -p "${SDCARD}"/opt/airwaves
  run_host_command_logged cp -aR "${EXTENSION_DIR}"/* "${SDCARD}"/opt/airwaves/
  run_host_command_logged chmod -R +x "${SDCARD}"/opt/airwaves/scripts

}

function post_family_tweaks__install_airwaves_os_base() {

  display_alert "Installing Airwaves OS base" "${EXTENSION}" "info"

#  export LANG=C LC_ALL="en_US.UTF-8"
#  export DEBIAN_FRONTEND=noninteractive
#  export APT_LISTCHANGES_FRONTEND=none


  display_alert "install motd messages" "${EXTENSION}" "info"
  run_host_command_logged cp "${EXTENSION_DIR}"/config/15-airwaves-header "${SDCARD}"/etc/update-motd.d/
  run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/15-airwaves-header
  run_host_command_logged cp "${EXTENSION_DIR}"/config/50-airwaves-help "${SDCARD}"/etc/update-motd.d/
  run_host_command_logged chmod +x "${SDCARD}"/etc/update-motd.d/50-airwaves-help

  display_alert "install config files" "${EXTENSION}" "info"
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/bashrc-custom.template "${SDCARD}"/root/.bashrc
  run_host_command_logged mkdir -p "${SDCARD}"/etc/airwaves
  run_host_command_logged mkdir -p "${SDCARD}"/opt/airwaves
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/airwaves-config.json.template "${SDCARD}"/etc/airwaves/config.json
  run_host_command_logged touch "${SDCARD}"/opt/airwaves/.needs-first-run

  display_alert "build.config airwaves.config symlink hacks" "${EXTENSION}" "warn"
  chroot_sdcard ln -sF /opt/airwaves/config/build.config /opt/airwaves/build.config
  #chroot_sdcard ln -sF /opt/airwaves/config/build.config /opt/airwaves/airwaves.config

  display_alert "install systemd units" "${EXTENSION}" "info"
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/avahi-api.service.template "${SDCARD}"/etc/avahi/services/airwaves-api.service
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/avahi-config.service.template "${SDCARD}"/etc/avahi/services/airwaves-config.service
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/avahi-web.service.template "${SDCARD}"/etc/avahi/services/airwaves-web.service
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/systemd-airwaves-first-run.service.template "${SDCARD}"/etc/systemd/system/airwaves-first-run.service
  chroot_sdcard systemctl --no-reload enable airwaves-first-run.service
  run_host_command_logged cp "${EXTENSION_DIR}"/config/templates/systemd-airwaves-manager.service.template "${SDCARD}"/etc/systemd/system/airwaves-manager.service
  chroot_sdcard systemctl --no-reload enable airwaves-manager.service

}
