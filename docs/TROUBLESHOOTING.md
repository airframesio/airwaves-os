# Troubleshooting Airwaves OS

Common problems and how to diagnose them. Most diagnosis happens over SSH:

```bash
ssh airwaves@airwaves-XXXXXX.local    # or ssh airwaves@<ip>
```

(Default password `airwaves` — if you haven't changed it yet, do that first;
see [GETTING_STARTED.md](GETTING_STARTED.md).)

## Device not discoverable (`.local` doesn't resolve)

`airwaves-XXXXXX.local` relies on **mDNS** (avahi), which only works when your
computer and the device are on the same subnet and the network passes
multicast. Corporate networks, guest Wi-Fi, separate VLANs, and some routers
block it.

1. **Check the router's DHCP client table.** The device requests an address
   with a hostname starting with `airwaves-`. Use the listed IP directly:
   `http://<ip>`. This works on any network, with or without mDNS.
2. **Browse for the device** from the same subnet:
   - macOS: `dns-sd -B _http._tcp` (look for "Airwaves OS on …")
   - Linux: `avahi-browse -art | grep -i airwaves`
3. **Scan the subnet** as a last resort:
   `nmap -p 80 --open 192.168.1.0/24` (adjust to your subnet).
4. **Attach a monitor and keyboard.** The console welcome message prints the
   device's URL and IP address.
5. If `.local` resolves on one machine but not another: older Windows
   versions and some VPN clients don't handle mDNS — use the IP.

The hostname suffix is the last six hex digits of the device's primary MAC
address, so you can also match it against the MAC shown in your router.

## Web UI not loading

SSH in and check the container stack:

```bash
docker ps
```

You should see `airwaves-gateway` (port 80) and `airwaves-manager` running.
If not:

```bash
# Did first-boot initialization succeed?
systemctl status airwaves-init
journalctl -u airwaves-init -b --no-pager

# Container stack service (subsequent boots)
systemctl status airwaves-containers

# Container logs
docker logs airwaves-gateway --tail 100
docker logs airwaves-manager --tail 100
```

Common causes:

- **No internet on first boot.** If the images aren't pre-baked, `airwaves-init`
  pulls `airwaves-gateway` and `airwaves-manager` from `ghcr.io`. A failed pull
  is retried on the next boot — connect Ethernet and reboot.
- **Stack stopped.** Restart it manually:

  ```bash
  cd /etc/airwaves && sudo docker compose up -d --remove-orphans
  ```

- **Page loads but the UI shows errors** — usually the manager API. Check
  `docker logs airwaves-manager`.

## SDR not detected

Supported out of the box: RTL-SDR (Blog V3/V4, generic RTL2832U), Airspy
Mini/R2/HF+, HackRF One, SDRplay RSP series, FlightAware Pro Stick / Pro Stick
Plus, Funcube Dongle Pro / Pro+.

```bash
# Is it visible on USB at all?
lsusb
```

- **Not in `lsusb`:** bad cable/port or insufficient power. On Raspberry Pi,
  use a powered USB hub for power-hungry SDRs; try a different port.
- **In `lsusb` but apps can't open it:**
  - Permissions come from the udev rules at
    `/etc/udev/rules.d/90-airwaves-sdr.rules`. If your device's USB ID isn't
    covered (exotic clone), add a rule, then:

    ```bash
    sudo udevadm control --reload && sudo udevadm trigger
    ```

    and replug the device.
  - The DVB-T kernel drivers are blacklisted at
    `/etc/modprobe.d/airwaves-sdr-blacklist.conf` so they can't claim RTL
    dongles. Verify none loaded: `lsmod | grep -E 'rtl|dvb'` — if one did,
    reboot after confirming the blacklist file is intact.
  - Quick functional test for RTL dongles (the `rtl-sdr` tools are
    preinstalled): `rtl_test -t`. "Device or resource busy" means another
    app/container already claimed it — only one app can use a given SDR at a
    time. Stop the other app first.

## App or container won't start

```bash
docker ps -a                      # look for Exited / Restarting containers
docker logs <container-name> --tail 200
```

Also worth checking:

- **Disk full:** `df -h /`. The OS auto-expands the root filesystem on boot
  (`airwaves-growfs`); on VMs you must enlarge the virtual disk first — see
  [DEPLOYMENT.md](DEPLOYMENT.md).
- **SDR conflict:** decoders fail at startup if their SDR is missing or
  claimed by another container (see the SDR section above).
- **Docker itself:** `systemctl status docker` and `journalctl -u docker -b`.

## Update problems

Updates are managed from the web UI (**Settings → System Update**) and applied
on the host by `airwaves-update.service`. Full details in
[UPDATES.md](UPDATES.md).

- **No updates showing / check fails:** the device fetches a manifest for its
  channel from `raw.githubusercontent.com` — it needs internet access. Force a
  re-check from the UI. The channel (`stable`, `beta`, or `dev`) is stored in
  `/etc/airwaves/.versions.json`; `stable` is the default and what you want
  unless you're testing.
- **Update appears stuck:** progress is written to
  `/etc/airwaves/update/status.json` on the host and survives the manager
  restarting mid-update (the UI reconnects automatically). Watch the updater
  itself with:

  ```bash
  journalctl -u airwaves-update -f
  ```

- **Update failed:** the updater health-gates the new stack and automatically
  rolls back the compose/catalog config and re-starts the previous stack if
  the gateway/manager don't come up healthy. Container image rollback is
  best-effort; config rollback is exact. OS package and major Debian upgrades
  are **not** atomic — back up before applying a major upgrade.
- **Older device with no update UI:** devices flashed before the updater
  existed need a one-time bootstrap:

  ```bash
  curl -fsSL https://raw.githubusercontent.com/airframesio/airwaves-os/main/scripts/bootstrap-updater.sh | sudo bash
  ```

## Collecting logs for a bug report

Run these over SSH and attach the output to your report:

```bash
cat /etc/airwaves-release          # Airwaves version, build date, board
uname -a                           # kernel
docker ps -a                       # container states
docker logs airwaves-manager --tail 200
docker logs airwaves-gateway --tail 200
journalctl -u airwaves-init -u airwaves-containers -u airwaves-update -b --no-pager
lsusb                              # attached SDRs (only if hardware-related)
```

File issues at
[github.com/airframesio/airwaves-os/issues](https://github.com/airframesio/airwaves-os/issues)
with the board, image version (from `/etc/airwaves-release`), what you
expected, and what happened. For quick questions, the Discord linked from the
README is the fastest channel.
