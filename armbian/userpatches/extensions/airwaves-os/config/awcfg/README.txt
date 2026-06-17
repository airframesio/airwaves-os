========================================================================
 Airwaves OS - configuration partition (AWCFG)
========================================================================

This small FAT32 partition lets you pre-configure or recover an Airwaves OS
device WITHOUT a screen or keyboard. You can edit it on any computer
(Windows, macOS, Linux) before or after flashing the card.

Edit the file:  airwaves-install.json

------------------------------------------------------------------------
WHEN IS IT USED?
------------------------------------------------------------------------
1) FIRST setup. On the very first boot of a freshly flashed device, Airwaves
   OS reads airwaves-install.json automatically and applies it.

2) LATER, on a device that is already set up. The file is IGNORED on normal
   boots. To make Airwaves OS apply it again, also create an empty file named:

        apply.conf

   On the next boot the device reads airwaves-install.json, applies it, and
   deletes apply.conf (so it only applies once).

   This is the easy way to fix Wi-Fi on a headless device: power it off, pull
   the card, edit the "wifi" section below, create apply.conf, put the card
   back, and power on. It reconnects with the new Wi-Fi.

------------------------------------------------------------------------
FIELDS (all optional; leave blank to skip)
------------------------------------------------------------------------
  hostname   Device name on the network (letters, digits, hyphens).
  wifi.ssid  Wi-Fi network name.
  wifi.psk   Wi-Fi password (leave blank for an open network).
  channel    Update channel: stable | beta | dev.
  timezone   e.g. "America/Chicago".
  ssh_keys   List of SSH public keys to authorize for remote login.

  The following control UNATTENDED INSTALL to an internal disk and are OFF
  by default. They ERASE the target disk, so BOTH of the first two must be
  set to true before anything is erased.

  NOTE (current release): unattended install via these fields is NOT active
  yet and setting them has no effect. Use the on-screen installer for now.
  They are documented here because they will be enabled in a coming update:

  auto_install        true to install automatically (no menu).
  confirm_destructive true to acknowledge the target disk will be ERASED.
  target              "auto", "largest", or a device like "/dev/nvme0n1".
  ab                  true for A/B (blue/green) layout (needs >= 12 GiB).
  self_install        true to let a card flashed to SD self-install to the
                      device's internal eMMC/NVMe (ARM boards).
  post_install        "poweroff" or "reboot" after a successful install.

------------------------------------------------------------------------
SECURITY NOTE
------------------------------------------------------------------------
This partition is NOT encrypted. The Wi-Fi password is stored in plain text
and can be read by anyone who has the card. Keep the card physically secure,
or clear the "psk" field after the device has connected.
