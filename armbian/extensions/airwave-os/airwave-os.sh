#!/bin/bash

function user_config__airwave_os_extra_packages() {
  display_alert "Add additional debian packages for airwaveOS depdencncies" "custom_airwave" "info"
  add_packages_to_image armbian-config git golang avahi-daemon avahi-utils nala sudo
}
	
CustomizeArmbian() {
  echo "Running AROS customization installer..."

  mkdir -p /opt/aros
  cp -aR /tmp/overlay/* /opt/aros
  chmod -R +x /opt/aros/scripts

#  bash /tmp/overlay/customize-install-airwaves-os.sh $RELEASE $LINUXFAMILY $BOARD $BUILD_DESKTOP
}

. /opt/aros/build.config

ImportFile() {
  echo "Importing file"
}

GenerateConfig() {
  echo "Generating config"
}

InstallAROS() {

  echo ""
  cat << EOF
    _    _                                      ___  ____  
   / \  (_)_ ____      ____ ___   _____  ___   / _ \/ ___| 
  / _ \ | | '__\ \ /\ / / _\` \ \ / / _ \/ __| | | | \___ \ 
 / ___ \| | |   \ V  V / (_| |\ V /  __/\__ \ | |_| |___) |
/_/   \_\_|_|    \_/\_/ \__,_| \_/ \___||___/  \___/|____/ 
   Airwaves OS v1.0.0

   Board    : $BOARD
   Family   : $LINUXFAMILY
   Release  : $RELEASE
   Hostname : $HOSTNAME
EOF
  echo ""
  echo "  Installing AROS base"

  echo root:airframes | chpasswd
  rm /root/.not_logged_in_yet

  export LANG=C LC_ALL="en_US.UTF-8"
  export DEBIAN_FRONTEND=noninteractive
  export APT_LISTCHANGES_FRONTEND=none

  echo "  Config:"

  echo "    - /etc/update-motd.d/10-armbian-header"
  chmod -x /etc/update-motd.d/10-armbian-header

  echo "    + /etc/update-motd.d/15-aros-header"
  cp /tmp/overlay/config/15-aros-header /etc/update-motd.d/
  chmod +x /etc/update-motd.d/15-aros-header

  echo "    + /etc/update-motd.d/50-aros-help"
  cp /tmp/overlay/config/50-aros-help /etc/update-motd.d/
  chmod +x /etc/update-motd.d/50-aros-help

  echo "    + /etc/avahi/services/aros.service"
  cp /tmp/overlay/config/templates/avahi-aros.service.template /etc/avahi/services/aros.service

  echo "    + /root/.bashrc"
  cp /tmp/overlay/config/templates/bashrc-custom.template /root/.bashrc

  echo "    + /etc/aros/config.json"
  mkdir -p /etc/aros
  cp /tmp/overlay/config/templates/aros-config.json.template /etc/aros/config.json

  echo "    + /etc/systemd/system/aros-first-run.service"
  cp /tmp/overlay/config/templates/systemd-aros-first-run.service.template /etc/systemd/system/aros-first-run.service
  systemctl --no-reload enable aros-first-run.service

  echo "    + /etc/systemd/system/aros-manager.service"
  cp /tmp/overlay/config/templates/systemd-aros-manager.service.template /etc/systemd/system/aros-manager.service
  systemctl --no-reload enable aros-manager.service

  echo "    + /opt/aros/.needs-first-run"
  touch /opt/aros/.needs-first-run

  echo "    + setting hostname to ${HOSTNAME}"
  /tmp/overlay/scripts/change-hostname.sh $HOSTNAME

  echo ""

  echo "  Components:"
  InstallDocker
  InstallTailscale

  chage -d 0 root

}

InstallDocker() {
  echo "    * Installing Docker"
  curl -fsSL https://get.docker.com | sh 2> /dev/null > /dev/null
}

InstallTailscale() {
  echo "    * Installing Tailscale"
  curl -fsSL https://pkgs.tailscale.com/stable/ubuntu/focal.noarmor.gpg | sudo tee /usr/share/keyrings/tailscale-archive-keyring.gpg >/dev/null
  curl -fsSL https://pkgs.tailscale.com/stable/ubuntu/focal.tailscale-keyring.list | sudo tee /etc/apt/sources.list.d/tailscale.list >/dev/null
  sudo apt-get -qq update > /dev/null
  sudo apt-get -qq install tailscale > /dev/null
}

InstallAROS "$@"
