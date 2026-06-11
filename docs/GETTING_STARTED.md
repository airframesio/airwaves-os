# Getting Started with Airwaves OS

From zero to decoding radio signals: download an image, verify it, flash it,
boot it, and install your first app.

## 1. Choose and download an image

Images are published on the
[GitHub Releases page](https://github.com/airframesio/airwaves-os/releases)
as compressed `.img.xz` files, one per board:

| Your hardware | Image to download | Base OS |
|---|---|---|
| Raspberry Pi 4B **or** Raspberry Pi 5 | `rpi4b` | Ubuntu Noble |
| Rock 5B | `rock-5b` | Debian |
| Orange Pi 5 | `orangepi5` | Debian |
| Mini PC / server / VM (x86_64, UEFI) | `uefi-x86` | Debian |

> **Raspberry Pi 5 owners:** there is no separate Pi 5 image. Armbian's
> `rpi4b` board target covers all 64-bit Raspberry Pi models (3 through 5),
> so the `rpi4b` image is the right one.

Running in a VM (Proxmox, VMware)? Read [DEPLOYMENT.md](DEPLOYMENT.md) first —
the x86 image needs UEFI firmware and the virtual disk must be enlarged to
≥16 GB.

## 2. Verify the checksum

Every image is published with a matching `.sha256` file. Download both into
the same directory, then:

```bash
# Linux
sha256sum -c Airwaves_OS_<...>.img.xz.sha256

# macOS
shasum -a 256 -c Airwaves_OS_<...>.img.xz.sha256
```

```powershell
# Windows (PowerShell) - compare the output to the hash in the .sha256 file
Get-FileHash .\Airwaves_OS_<...>.img.xz -Algorithm SHA256
```

You should see `OK`. If verification fails, the download is corrupt or
tampered with — re-download before flashing.

## 3. Flash the image

Use a **16 GB or larger** SD card / USB drive / SSD (32 GB+ recommended if
you'll run several apps).

- **balenaEtcher** (easiest, all platforms): select the `.img.xz` directly —
  no need to decompress — choose your drive, and flash.
- **dd** (Linux/macOS):

  ```bash
  xz -d Airwaves_OS_<...>.img.xz
  sudo dd if=Airwaves_OS_<...>.img of=/dev/sdX bs=4M status=progress conv=fsync
  ```

  **Double-check `/dev/sdX`** (`lsblk` / `diskutil list`) — dd will happily
  overwrite the wrong disk.

## 4. First boot

Insert the card/drive, connect **Ethernet** (recommended for first boot), plug
in your SDR, and power on. First boot is fully unattended — there is no setup
wizard. Behind the scenes the system:

1. Expands the root filesystem to fill your card/disk (`airwaves-growfs`).
2. Generates a unique device ID and sets the hostname to `airwaves-XXXXXX`
   (the last six hex digits of the primary MAC address).
3. Loads the pre-baked gateway and manager container images, or pulls them
   from `ghcr.io` if they aren't baked into the image.
4. Starts the container stack and begins advertising itself via mDNS.

Expect the first boot to take a few minutes — longer on a slow internet
connection if container images need to be pulled. If a pull fails (e.g. no
network yet), it is retried on the next boot. Subsequent boots are much
faster.

## 5. Find your device

- **mDNS (most home networks):** open `http://airwaves-XXXXXX.local`. If you
  don't know the suffix, browse for it:
  - macOS: `dns-sd -B _http._tcp` (the device advertises "Airwaves OS on …")
  - Linux: `avahi-browse -art | grep -i airwaves`
- **Router DHCP table (networks without mDNS):** open your router's admin
  page and look at the DHCP client list for a hostname starting with
  `airwaves-`. Use its IP address directly: `http://<ip>`.
- **Attached monitor:** log in on the console — the welcome message prints the
  exact URL and IP address.

## 6. Log in and change the default password

The system ships with a pre-created user:

- **Username:** `airwaves`
- **Password:** `airwaves`

> ⚠️ **Change the default password immediately.** Anyone on your network can
> SSH into the device with these well-known credentials. After your first
> login, run:
>
> ```bash
> ssh airwaves@airwaves-XXXXXX.local
> passwd                # change the airwaves user's password
> sudo passwd root      # the root password is also 'airwaves' by default
> ```

## 7. Open the web UI

Browse to `http://airwaves-XXXXXX.local` (or `http://<ip>`). The control app
runs on port 80 and gives you system status, hardware (detected SDRs),
container management, app catalog, and system updates.

## 8. Install your first app

All decoder/feeder apps run as Docker containers and are installed from the
built-in catalog (e.g. **ADS-B Ultrafeeder**, **acarsdec**, **dumpvdl2**,
**AIS-Catcher**, **rtl_433**, **ACARS Hub**). With a supported SDR plugged in
(RTL-SDR, Airspy, HackRF, SDRplay, FlightAware sticks, Funcube — see the
README for the full list):

1. Open the **Apps** section in the web UI.
2. Pick an app — decoders that need an SDR will let you select which device
   to use (leave blank to auto-select the first one).
3. Fill in any required settings (e.g. latitude/longitude for ADS-B) and
   install. The container is pulled and started automatically.

Note that only one app can use a given SDR at a time.

## Next steps

- Keep the system current: [UPDATES.md](UPDATES.md) (updates are applied from
  the web UI under **Settings → System Update**).
- Running in a VM or short on disk: [DEPLOYMENT.md](DEPLOYMENT.md).
- Something not working: [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
