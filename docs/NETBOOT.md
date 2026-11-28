# Network install (netboot)

Airwaves OS can be installed over the network from Airframes Desktop. Desktop
runs a small boot server (proxyDHCP, TFTP, HTTP); a device that boots from the
network gets the **Airwaves OS network installer**, a kernel plus initramfs
that asks Desktop what to install, streams the image onto the device's own
disk, writes the device settings to the AWCFG partition and reboots into
Airwaves OS. On first boot the manager enrolls with Desktop, which signs the
device in to the Desktop user's Airframes account and registers its station.

This file is the contract between the installer and manager (this repo) and
Desktop (`airframesio/desktop-app`, `docs/netboot.md`). Change both together.

- [Supported devices](#supported-devices)
- [Release assets](#release-assets)
- [Boot chain](#boot-chain)
- [Installer HTTP API](#installer-http-api)
- [AWCFG written by the installer](#awcfg-written-by-the-installer)
- [First-boot enrollment](#first-boot-enrollment)
- [Signatures](#signatures)
- [Raspberry Pi: turning on network boot](#raspberry-pi-turning-on-network-boot)
- [Testing without hardware](#testing-without-hardware)

## Supported devices

| Device | Installer asset | Boot | Notes |
|---|---|---|---|
| x86_64 PCs and mini PCs, UEFI | `netinstall-x86_64.tar` | UEFI PXE (arch 7/9) to iPXE, then HTTP | Secure Boot must be off for now |
| Raspberry Pi 4, 400, CM4 | `netinstall-rpi-arm64.tar` | EEPROM bootloader, TFTP | Network boot must be turned on once ([below](#raspberry-pi-turning-on-network-boot)) |

Not supported: Raspberry Pi 5 / 500 / CM5 (the Airwaves OS Pi image runs the
Armbian bcm2711 kernel, which does not support the Pi 5's BCM2712 and RP1), Pi 3
and older, legacy BIOS PCs, and Rockchip/Amlogic boards (no network boot on a
blank board; flash an SD card instead).

## Release assets

Each release carries, next to the OS images:

| Asset | Contents |
|---|---|
| `netinstall-x86_64.tar` | `netinstall-x86_64/`: `vmlinuz`, `initramfs.img`, `ipxe.efi` (iPXE `snponly.efi`), `boot.ipxe` (template), `manifest.json` |
| `netinstall-rpi-arm64.tar` | `netinstall-rpi-arm64/`: `tftp/` (the whole Pi TFTP root: `start4.elf`, `fixup4.dat`, `bcm2711-*.dtb`, `overlays/`, `config.txt`, `cmdline.txt` (template), `kernel8.img`, `initramfs.img`), `manifest.json` |
| `SHA256SUMS` | SHA-256 of every asset in the release (images, netinstall tars, container tars) |
| `SHA256SUMS.minisig` | minisign (Ed25519) signature of `SHA256SUMS` ([Signatures](#signatures)) |

`manifest.json`:

```json
{
  "schema": 1,
  "arch": "x86_64",
  "board": "uefi-x86",
  "version": "1.0.39",
  "kernel": "6.18.35-current-x86",
  "protocol": 1,
  "files": { "kernel": "vmlinuz", "initramfs": "initramfs.img", "bootloader": "ipxe.efi" },
  "cmdline": "console=tty0 console=ttyS0,115200 quiet"
}
```

`protocol` is the version of the HTTP API below. Desktop must refuse an
installer whose `protocol` it does not know.

## Boot chain

All boot parameters reach the installer on the kernel command line:

| Parameter | Meaning |
|---|---|
| `awnet.url=<base>` | Base URL of Desktop's netboot API, e.g. `http://192.168.1.20:47811/netboot/v1` |
| `awnet.token=<token>` | This device's token (URL-safe, 16 to 128 chars). Sent as `Authorization: Bearer <token>` on every call. Desktop issues one per approved device |
| `awnet.debug=1` | Optional. Opens a shell on tty2 and keeps going on errors instead of rebooting |

The installer gets its address with DHCP on every wired interface that has
link and uses the first one that reaches `awnet.url`.

### x86_64 (UEFI PXE)

1. Firmware PXE request (option 93 = 7 or 9). Desktop's proxyDHCP answers
   with the boot file `ipxe.efi` over TFTP.
2. iPXE runs DHCP again. Its request carries user class `iPXE` (option 77).
   Desktop answers it with an HTTP boot file instead:
   `http://<desktop>:<port>/netboot/v1/boot.ipxe?token=<token>`, so iPXE does
   not loop back into itself.
3. `boot.ipxe` is generated per device from the template in the tar
   (`@URL@`, `@TOKEN@`, `@CMDLINE@` replaced). It fetches `vmlinuz` and
   `initramfs.img` over HTTP and boots them with the parameters above.

### Raspberry Pi 4 (EEPROM bootloader)

1. The bootloader's DHCP request carries vendor class
   `PXEClient:Arch:00000:UNDI:002001`. Desktop's proxyDHCP answers with its
   own address as TFTP server and option 43 `Raspberry Pi Boot`.
2. The bootloader reads `<serial>/start4.elf` (and every later file) under a
   directory named by the Pi's 8-hex-digit serial number, then falls back to
   the TFTP root. Desktop serves the `tftp/` folder at both, and generates
   `cmdline.txt` per device from the template (same placeholders as
   `boot.ipxe`).
3. The firmware loads `kernel8.img` and `initramfs.img` (both over TFTP, about
   40 MB together, so blksize/windowsize support matters).

## Installer HTTP API

Base URL is `awnet.url`. Every request carries `Authorization: Bearer <token>`
and `User-Agent: airwaves-netinstall/<version>`. Bodies are JSON. A `401` or
`403` stops the installer (shown on screen). Network errors and `5xx` are
retried with backoff for up to 10 minutes.

### `POST /hello`

Sent once the network is up, then again whenever the previous answer was
`wait`.

```json
{
  "protocol": 1,
  "installer_version": "1.0.39",
  "arch": "x86_64",
  "boot": "uefi",
  "model": "Intel(R) Client Systems NUC8i5BEH",
  "vendor": "Intel(R) Client Systems",
  "product": "NUC8i5BEH",
  "serial": "G6BE123456",
  "rpi_serial": null,
  "memory_bytes": 8222191616,
  "interfaces": [ { "name": "eth0", "mac": "1c:69:7a:01:02:03", "ip": "192.168.1.57", "link": true } ],
  "disks": [
    {
      "path": "/dev/nvme0n1",
      "size_bytes": 256060514304,
      "model": "Samsung SSD 970 EVO Plus 250GB",
      "serial": "S4EUNX0N123456",
      "transport": "nvme",
      "removable": false,
      "rotational": false,
      "partitions": 3,
      "has_airwaves": false
    }
  ]
}
```

- `boot`: `uefi` (x86) or `rpi` (Pi bootloader).
- `rpi_serial`: the Pi's serial as the bootloader sends it in TFTP paths
  (lowercase hex, last 8 digits), or null.
- `model`: DMI `sys_vendor` + `product_name` on x86, the device tree `model`
  on ARM (`Raspberry Pi 4 Model B Rev 1.4`).
- `disks`: every block device that can hold an install (no loop, ram, zram
  or optical; at least 4 GiB). `has_airwaves` is true when a partition is
  labeled `AWCFG` or `armbi_root`.

Answer (`200`), one of:

```json
{ "action": "wait", "retry_after": 5, "message": "Waiting for you to approve this device in Airframes Desktop" }
```

```json
{ "action": "abort", "message": "This device was not approved", "then": "poweroff" }
```

```json
{
  "action": "install",
  "image": {
    "name": "Airwaves_OS_1.0.39_Uefi-x86_trixie_current_6.18.35_minimal.img.xz",
    "url": "http://192.168.1.20:47811/netboot/v1/image/Airwaves_OS_1.0.39_Uefi-x86_trixie_current_6.18.35_minimal.img.xz",
    "sha256": "<64 hex>",
    "size_bytes": 1368392160
  },
  "sums": {
    "url": "http://192.168.1.20:47811/netboot/v1/release/1.0.39/SHA256SUMS",
    "sig_url": "http://192.168.1.20:47811/netboot/v1/release/1.0.39/SHA256SUMS.minisig"
  },
  "target": { "path": "/dev/nvme0n1", "serial": "S4EUNX0N123456", "size_bytes": 256060514304 },
  "erase_confirmed": true,
  "countdown_seconds": 10,
  "awcfg": { "schema": 1, "hostname": "airwaves-hangar", "...": "see below" },
  "post_install": "reboot"
}
```

- `message` is shown on the device's screen.
- `retry_after` is in seconds (1 to 60, default 5).
- `then` for `abort`: `poweroff` (default), `reboot` or `shell` (only honored
  with `awnet.debug=1`).
- `image.url`, `sums.url`, `sums.sig_url` may be absolute or relative to the
  base URL. They are fetched with the same bearer token.
- `image.name` must be listed in `SHA256SUMS` with the same `sha256`.
  `size_bytes` is the compressed size and drives the progress bar.
- `target` must match a disk from `/hello` by `path`, and by `serial` and
  `size_bytes` when the installer reported them, so a renumbered disk is never
  written. The installer also refuses a target smaller than the decompressed
  image.
- `erase_confirmed` must be `true`. It records that the user chose this disk
  in Desktop knowing it will be erased.
- `countdown_seconds` (0 to 60, default 10): shown on screen before writing.
  Pressing a key on the device's console during the countdown cancels.
- `awcfg`: the whole `airwaves-install.json` to write
  ([AWCFG](#awcfg-written-by-the-installer)). Desktop's `/hello` answer is
  the only place the Wi-Fi password travels, over the LAN, once.
- `post_install`: `reboot` (default) or `poweroff`.

### `POST /progress`

Sent at each stage change and about every 2 seconds while writing. `202` or
`200` with any body; errors are ignored (the install never waits on it).

```json
{ "stage": "writing", "percent": 42, "bytes_done": 574724710, "bytes_total": 1368392160, "message": "" }
```

Stages, in order: `booted`, `waiting`, `verifying_manifest`, `countdown`,
`writing`, `verifying`, `configuring`, `rebooting`. On failure: `failed`
with `message` (and `error`, a short code: `manifest_signature`,
`manifest_missing_image`, `image_checksum`, `target_mismatch`,
`target_too_small`, `download`, `write`, `awcfg`, `canceled`).

`percent` is of the compressed download while `writing`, and 100 after.

## AWCFG written by the installer

The installer writes `awcfg` from the plan verbatim as
`airwaves-install.json` on the new disk's AWCFG partition, after checking it
is a JSON object. Every field of the [existing format](../armbian/userpatches/extensions/airwaves-os/config/awcfg/README.txt)
applies on first boot (hostname, Wi-Fi, channel, timezone, SSH keys). The
destructive fields (`auto_install`, `self_install`) must be false: the
installer has already installed. Two blocks are added for network installs:

```json
{
  "schema": 1,
  "hostname": "airwaves-hangar",
  "wifi": { "ssid": "", "psk": "" },
  "channel": "stable",
  "timezone": "America/Los_Angeles",
  "ssh_keys": [],
  "enroll": {
    "url": "http://192.168.1.20:47811/netboot/v1/enroll",
    "code": "<one-time enrollment code, 16 to 128 URL-safe chars>",
    "expires_at": "2026-12-05T08:00:00Z"
  },
  "station": {
    "ident": "KE-HANGAR",
    "latitude": 38.6766,
    "longitude": -121.0158,
    "altitude_m": 260,
    "name": "Hangar",
    "xng": {
      "feed_airframes": true,
      "modes": "",
      "airband_channels": "",
      "atc_feed": true,
      "atc_station_name": "",
      "transcribe": false
    }
  }
}
```

- `station.ident`: the xng station ID (`A-Z`, `0-9`, `_`, `-`, up to 40;
  xng adds `-ACARS`, `-VDL2`, ... per mode).
- `station.latitude/longitude/altitude_m`: written to the device settings
  (`config.json` `station`), used by xng and the station record.
- `station.name`: the device's display name (`config.json` `device.name`).
- `station.xng`: settings for the xng app, the same as the Feed Airframes
  page (`XNG_FEED_AIRFRAMES`, `XNG_MODES`, `XNG_AIRBAND_CHANNELS`,
  `XNG_ATC_FEED`, `XNG_ATC_STATION_NAME`, `XNG_TRANSCRIBE`). Omit `xng` or
  leave fields empty for the defaults (every SDR in the mode that suits it).
  Without an SDR plugged in, xng is not started; the station is still
  registered and the Feed Airframes page starts xng later.

On first boot `airwaves-preconfig` applies the usual fields, then moves
`enroll` and `station` into `/etc/airwaves/netinstall.json` (mode 600) and
blanks them in `airwaves-install.json`, so the enrollment code does not stay
on the FAT partition. It also clears `wifi.psk` from the AWCFG copy once the
Wi-Fi profile is saved, for network installs only (`enroll` present).

## First-boot enrollment

The manager reads `/etc/airwaves/netinstall.json` at startup. If the device is
not already signed in:

1. `POST <enroll.url>` (no bearer token; the code is the credential):

   ```json
   { "code": "<enroll.code>", "hostname": "airwaves-hangar", "model": "Raspberry Pi 4 Model B Rev 1.4", "os_version": "1.0.39", "mac": "dc:a6:32:01:02:03" }
   ```

   Desktop starts a device sign-in for client `airframes-feeder`, scope
   `atc:feed` (`POST /auth/device/code`), approves the user code with the
   signed-in Desktop user, and answers:

   ```json
   { "device_code": "<device_code>", "interval": 5, "expires_in": 600 }
   ```

   Only the `device_code` crosses the LAN. `410` means the code was used or
   expired and `404` that Desktop does not know it: the manager stops
   enrolling and the device's dashboard shows the normal sign-in. Network
   errors are retried every 30 s until `enroll.expires_at`.

2. The manager polls `POST /auth/token` on the Airframes API with that
   `device_code`, exactly like the dashboard sign-in, and stores xng's
   credentials (`atc.json`).

3. With the access token from that answer it registers the station:
   `POST /feeder/station` `{ "ident", "latitude", "longitude" }`.

4. It applies the station location and name, and installs xng with the
   `station.xng` settings if xng is not installed yet and an SDR is free.

5. It reports back (best effort, no retries beyond 3):
   `POST <enroll.url>/status`
   `{ "code": "...", "stage": "signed_in" | "station_registered" | "done" | "failed", "station_id": 237411012, "ident": "KE-HANGAR", "username": "kevin", "message": "" }`.

Then `netinstall.json` is deleted, except after `failed`, when it is kept
with `"done": true` so the manager does not try again on every restart.

## Signatures

Releases ship `SHA256SUMS` and `SHA256SUMS.minisig`, signed with
[minisign](https://jedisct1.github.io/minisign/) by the release workflow
(secret `MINISIGN_SECRET_KEY`, the contents of the `.key` file, and
`MINISIGN_PASSWORD`). The public key is [`netinstall/minisign.pub`](../netinstall/minisign.pub),
built into the installer, and must be built into Desktop too.

Desktop verifies the signature before caching an image or installer. The
installer checks it again, using OpenSSL (Ed25519 over the BLAKE2b-512 of the
file, the default prehashed minisign format, plus the trusted comment), before
it writes anything, and checks the image's SHA-256 while streaming it. Because
the image is streamed, the SHA-256 is only known once it is written: on a
mismatch the installer erases the start and end of the disk so it can never
boot a partial or altered image, and reports `image_checksum`.

The netinstall kernel and initramfs themselves are not verified by the device
(nothing is, on a Pi or on x86 with Secure Boot off): anyone on the same LAN
could answer the boot request first. That is how every PXE setup works.

## Raspberry Pi: turning on network boot

A Pi 4 tries the SD card and then USB, never the network, until its
bootloader setting is changed once. On a Pi running Raspberry Pi OS or
Airwaves OS:

```sh
sudo rpi-eeprom-config --edit
# set:
BOOT_ORDER=0xf241
```

`0xf241` reads right to left: SD card (1), then USB (4), then network (2),
then start over (f). A Pi with an SD card or USB disk that boots keeps
booting it; a Pi with neither tries the network. To install onto a USB disk
or SSD, leave the SD slot empty.

Without a running system, Raspberry Pi Imager can write a bootloader card:
choose Misc utility images, Bootloader (Pi 4 family), **Network Boot**,
write it to a spare SD card, boot the Pi from it once (green light blinks
steadily when done), then remove the card. Note that Imager's network boot
card sets the order to network first; change it to `0xf241` afterwards from
Airwaves OS (`rpi-eeprom-config --edit`) so a reinstall needs an empty SD
slot.

## Testing without hardware

`netinstall/test/` boots the x86 installer in QEMU with OVMF against a small
mock of Desktop's API (`mock-desktop.py`), installs a real release image onto
a virtual disk, and checks the AWCFG it wrote:

```sh
sudo netinstall/build.sh --rootfs /mnt/uefi-rootfs --arch x86_64 --out out/
netinstall/test/qemu-x86.sh out/netinstall-x86_64.tar Airwaves_OS_..._Uefi-x86_....img.xz
```
