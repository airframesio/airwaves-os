#!/bin/bash

function extension_prepare_config__airwaves_os() {
  display_alert "Target image will have AirwavesOS preinstalled" "${EXTENSION}" "info"
}

function user_config__airwaves_os_extra_packages() {
  display_alert "Add additional debian packages for AirwaveOS dependencies" "airwaves_os" "info"
  add_packages_to_image armbian-config git golang avahi-daemon avahi-utils nala sudo
}
	
function pre_install_kernel_debs__add_aros_scripts() {
  display_alert "Copying airwaves-os scripts to image->/opt/aros" "airwaves_os" "info"

  mkdir -p "${SDCARD}"/opt/aros
  cp -aR "${EXTENSION_DIR}"/* "${SDCARD}"/opt/aros
  chmod -R +x "${SDCARD}"/opt/aros/scripts

}

function post_family_tweaks__install_airwaves_os_base() {

  display_alert "Installing AirwaveOS base" "airwaves_os" "info"

  rm "${SDCARD}"/root/.not_logged_in_yet

#  export LANG=C LC_ALL="en_US.UTF-8"
#  export DEBIAN_FRONTEND=noninteractive
#  export APT_LISTCHANGES_FRONTEND=none


  echo "    - "${SDCARD}"/etc/update-motd.d/10-armbian-header"
  chmod -x "${SDCARD}"/etc/update-motd.d/10-armbian-header

  echo "    + "${SDCARD}"/etc/update-motd.d/15-aros-header"
  cp "${EXTENSION_DIR}"/config/15-aros-header "${SDCARD}"/etc/update-motd.d/
  chmod +x "${SDCARD}"/etc/update-motd.d/15-aros-header

  echo "    + "${SDCARD}"/etc/update-motd.d/50-aros-help"
  cp "${EXTENSION_DIR}"/config/50-aros-help "${SDCARD}"/etc/update-motd.d/
  chmod +x "${SDCARD}"/etc/update-motd.d/50-aros-help

  echo "    + "${SDCARD}"/etc/avahi/services/aros.service"
  cp "${EXTENSION_DIR}"/config/templates/avahi-aros.service.template "${SDCARD}"/etc/avahi/services/aros.service

  echo "    + "${SDCARD}"/root/.bashrc"
  cp "${EXTENSION_DIR}"/config/templates/bashrc-custom.template "${SDCARD}"/root/.bashrc

  echo "    + "${SDCARD}"/etc/aros/config.json"
  mkdir -p "${SDCARD}"/etc/aros
  cp "${EXTENSION_DIR}"/config/templates/aros-config.json.template "${SDCARD}"/etc/aros/config.json

  echo "    + "${SDCARD}"/etc/systemd/system/aros-first-run.service"
  cp "${EXTENSION_DIR}"/config/templates/systemd-aros-first-run.service.template "${SDCARD}"/etc/systemd/system/aros-first-run.service
  systemctl --no-reload enable aros-first-run.service

  echo "    + "${SDCARD}"/etc/systemd/system/aros-manager.service"
  cp "${EXTENSION_DIR}"/config/templates/systemd-aros-manager.service.template "${SDCARD}"/etc/systemd/system/aros-manager.service
  systemctl --no-reload enable aros-manager.service

  echo "    + "${SDCARD}"/opt/aros/.needs-first-run"
  touch "${SDCARD}"/opt/aros/.needs-first-run

  echo "    + setting hostname to ${HOSTNAME}"
  "${EXTENSION_DIR}"/scripts/change-hostname.sh $HOSTNAME

  echo ""

  echo "  Components:"

  chage -d 0 root

}

