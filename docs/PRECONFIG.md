# Airwaves OS — config partition (AWCFG) & unattended install

Every Airwaves OS image (and every installed system) carries a small FAT32
partition labelled **AWCFG** holding `airwaves-install.json`. You can edit it on
any computer (Windows/macOS/Linux). It is the offline, headless provisioning and
recovery surface. The manager API / web UI remains the primary way to configure a
running device; AWCFG is for when there's no screen or no network yet.

## When the config is read

`airwaves-preconfig` (a systemd oneshot that runs before the app stack) reads
AWCFG and applies it:

1. **First setup** — automatically, on the very first boot of a freshly flashed
   or freshly installed system.
2. **Later, on a running system** — only if a file named **`apply.conf`** is also
   present on AWCFG. It applies once, then deletes `apply.conf`.

On a normal boot of an already-set-up device with no `apply.conf`, AWCFG is
ignored.

### Headless Wi-Fi recovery (the common case)

A flashed device whose Wi-Fi didn't connect, with no screen attached:

1. Power off, remove the SD/USB, mount AWCFG on another computer.
2. Edit the `wifi` block in `airwaves-install.json`.
3. Create an empty file named `apply.conf` next to it.
4. Re-insert, power on. It reconnects, and `apply.conf` is consumed.

## `airwaves-install.json`

```jsonc
{
  "hostname": "airwaves-tower-1",          // RFC1123; blank = derive from MAC
  "wifi":    { "ssid": "MyNet", "psk": "secret" },  // blank ssid = skip
  "channel": "stable",                     // stable|beta|dev; blank = unchanged
  "timezone": "America/Chicago",           // blank = unchanged
  "ssh_keys": ["ssh-ed25519 AAAA... me"],  // added to root + airwaves

  // --- unattended install (DESTRUCTIVE; OFF by default) ---
  "auto_install": false,        // install with no menu (live USB / removable)
  "confirm_destructive": false, // REQUIRED with auto_install/self_install
  "target": "auto",             // "auto" (exactly one internal disk) | "largest" | "/dev/nvme0n1"
  "ab": false,                  // A/B (blue/green) layout; needs >= 12 GiB
  "self_install": false,        // flashed SD -> install to internal eMMC/NVMe (ARM)
  "post_install": "poweroff"    // "poweroff" (USB) or "reboot" (SD self-install)
}
```

Fields map onto the running system: `hostname` -> `config.json` `.device.hostname`
+ `hostnamectl`; `wifi` -> a NetworkManager connection; `channel` ->
`.versions.json`; `timezone` -> `timedatectl`; `ssh_keys` -> `authorized_keys`.

## Unattended install — safety model

Installing erases a disk, so the gate is strict:

- **Dual key:** nothing is erased unless **both** `auto_install` (or
  `self_install`) **and** `confirm_destructive` are `true`.
- **Target validation:** the target is resolved against the list of *internal,
  non-removable* disks, and the **source/boot disk is never a candidate**.
  `"auto"` requires *exactly one* candidate; anything ambiguous, missing, or
  invalid **falls back to the interactive installer and erases nothing**.
- **Cancelable:** the console shows a countdown before any write; cancel returns
  to the menu.
- **No loop:** the trigger lives in tmpfs; a successful SD->eMMC self-install
  marks the source so re-booting it won't reinstall. The installed target ships
  with the destructive flags forced off.

`auto_install` is for a removable live USB; `self_install` is for an ARM board
booted from a flashed SD card that should move itself onto internal storage.

## Build / flashing notes

- The AWCFG partition is the **last** partition and **survives `dd`-flash**. Use
  `dd`, Balena Etcher (>= 1.18), or Raspberry Pi Imager (>= 1.8); older imagers
  that truncate trailing partitions will drop it.
- The Wi-Fi PSK is stored in plain text on a FAT partition (no encryption). Keep
  the card physically secure, or clear `psk` after the device has connected.
