#!/bin/bash
#
# Airwaves OS - Hardware Extension
# Handles: SDR udev rules, kernel module blacklist, driver packages
#

function extension_prepare_config__airwaves_hardware() {
	display_alert "Preparing Airwaves OS hardware support" "${EXTENSION}" "info"
}

function user_config__airwaves_hardware_packages() {
	display_alert "Adding hardware support packages" "${EXTENSION}" "info"
	add_packages_to_rootfs libusb-1.0-0 librtlsdr0 rtl-sdr usbutils pciutils
}

function post_family_tweaks__airwaves_hardware_setup() {
	display_alert "Configuring Airwaves OS hardware support" "${EXTENSION}" "info"

	# Install udev rules for SDR devices
	display_alert "Installing SDR udev rules" "${EXTENSION}" "info"
	cat > "${SDCARD}/etc/udev/rules.d/90-airwaves-sdr.rules" << 'UDEV_EOF'
# Airwaves OS - SDR Device Rules
# Ensures SDR devices are accessible without root and creates stable symlinks

# RTL-SDR (RTL2832U)
SUBSYSTEM=="usb", ATTR{idVendor}=="0bda", ATTR{idProduct}=="2838", MODE="0666", GROUP="plugdev", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="0bda", ATTR{idProduct}=="2832", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# RTL-SDR Blog V4
SUBSYSTEM=="usb", ATTR{idVendor}=="0bda", ATTR{idProduct}=="2838", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# Airspy Mini / Airspy R2
SUBSYSTEM=="usb", ATTR{idVendor}=="1d50", ATTR{idProduct}=="60a1", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# Airspy HF+
SUBSYSTEM=="usb", ATTR{idVendor}=="03eb", ATTR{idProduct}=="800c", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# HackRF One
SUBSYSTEM=="usb", ATTR{idVendor}=="1d50", ATTR{idProduct}=="6089", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# SDRplay RSP1 / RSP1A / RSP2 / RSPduo / RSPdx
SUBSYSTEM=="usb", ATTR{idVendor}=="1df7", ATTR{idProduct}=="2500", MODE="0666", GROUP="plugdev", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="1df7", ATTR{idProduct}=="3000", MODE="0666", GROUP="plugdev", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="1df7", ATTR{idProduct}=="3010", MODE="0666", GROUP="plugdev", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="1df7", ATTR{idProduct}=="3020", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# FlightAware Pro Stick / Pro Stick Plus
SUBSYSTEM=="usb", ATTR{idVendor}=="0bda", ATTR{idProduct}=="2838", MODE="0666", GROUP="plugdev", TAG+="uaccess"

# Funcube Dongle Pro / Pro+
SUBSYSTEM=="usb", ATTR{idVendor}=="04d8", ATTR{idProduct}=="fb31", MODE="0666", GROUP="plugdev", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="04d8", ATTR{idProduct}=="fb56", MODE="0666", GROUP="plugdev", TAG+="uaccess"
UDEV_EOF

	# Blacklist kernel modules that conflict with SDR usage
	display_alert "Installing SDR kernel module blacklist" "${EXTENSION}" "info"
	cat > "${SDCARD}/etc/modprobe.d/airwaves-sdr-blacklist.conf" << 'BLACKLIST_EOF'
# Airwaves OS - Prevent DVB/TV drivers from claiming SDR devices
# These kernel modules interfere with userspace SDR access

blacklist dvb_usb_rtl28xxu
blacklist dvb_usb_rtl2832u
blacklist rtl2832
blacklist rtl2830
blacklist dvb_usb_v2
blacklist r820t
blacklist rtl2832_sdr
blacklist dvb_core
BLACKLIST_EOF

	display_alert "Hardware support configured" "${EXTENSION}" "info"
}
