# A/B (blue/green) OS updates — design & implementation plan

Status: **approved, implementation in progress**. This is the build spec for
atomic, rollback-safe full-OS upgrades, per `docs/ROADMAP.md` and issue #35.

## Decision: native dual-root A/B (no RAUC/Mender)

A custom, in-repo A/B scheme that reuses the existing installer
(`airwaves-install`) and updater (`airwaves-update`) machinery rather than
adopting an external framework. Rationale: the installer already has clean
per-platform `partition_*`/`bootloader_*` functions, the updater already has the
`request.json`/`status.json` + `download()`+sha256 + `wait_healthy()` +
rollback machinery, and the release manifest already serves tag-pinned,
sha256-verified assets from GitHub. RAUC/Mender would be net-new dependencies
whose bootloader integration is weakest exactly where we need it most (Pi, and
Armbian's binman-built rk3588 u-boot). We own every line; debuggable on a serial
console.

## Partition layout (GPT, all platforms)

Two **equal** root slots (so either can receive any future image) + one shared
`aw-data`. Slot size = clamp(35% of disk, 4 GiB, 12 GiB); data = remainder.

- **x86 UEFI**: `p1 ESP(FAT32,512M,AIRWAVES, shared, holds GRUB+grubenv)` · `p2 rootA(ext4,aw-root-a)` · `p3 rootB(ext4,aw-root-b)` · `p4 data(ext4,aw-data)`
- **Rockchip rk3588**: raw 0–16 MiB reserved for u-boot (unchanged) · `p1 rootA(16M→,aw-root-a)` · `p2 rootB(aw-root-b)` · `p3 data(aw-data)`
- **Raspberry Pi bcm2711**: **dual firmware** so a bad kernel can't brick both slots · `p1 fwA(FAT32,256M,AW-FW-A)` · `p2 rootA(aw-root-a)` · `p3 fwB(FAT32,256M,AW-FW-B)` · `p4 rootB(aw-root-b)` · `p5 data(aw-data)`

All installer layouts (single-root and A/B) also append a small **`AWCFG`**
FAT32 config partition (~100 MiB) as the **last** partition — the offline
provisioning / headless-recovery surface. It is inert on a running system unless
first-setup or an `apply.conf` trigger is present. See `docs/PRECONFIG.md`.

## Data model

`aw-data` holds everything that must survive a slot swap; each root is otherwise
self-contained. Per-slot fstab (identical in both slots):
```
UUID=<this slot>  /                ext4  defaults,noatime,errors=remount-ro  0 1
UUID=<aw-data>    /etc/airwaves    ext4  defaults                            0 2
UUID=<aw-data>    /var/lib/docker  ext4  defaults                            0 2   (subdir/bind of aw-data)
+ ESP (x86) / firmware (Pi) as today
```
Slot-LOCAL (not shared): `/var/log`, `/tmp`, the OS rootfs. Honest gap: `aw-data`
is a single point of failure for config+docker and is not itself A/B'd —
mitigated by the existing timestamped config backups (retains 10) + a TUI export;
later: btrfs subvolumes for config snapshots.

## Slot switching — `/opt/airwaves/scripts/airwaves-slot-manager`

One script abstracts the bootloader specifics behind verbs: `current`, `other`,
`set-try <slot>`, `commit <slot>`, `rollback`.
- **x86 (GRUB grubenv on shared ESP)**: two real menuentries `--id=aw-root-a/b`
  (each `search --label` + `root=UUID=`), `set fallback` to the other.
  `set-try B` → `grub-editenv set ab_try=aw-root-b boot_success=0`; `commit B` →
  `set saved_entry=aw-root-b boot_success=1; unset ab_try`.
- **Rockchip (u-boot env + boot.scr)**: `boot.scr`/`armbianEnv.txt` reads
  `aw_slot`; `bootcount`+`bootlimit=3`+`altbootcmd` flips slot pre-Linux.
  `set-try`/`commit` via `fw_setenv` (libubootenv). Falls back to rewriting
  `armbianEnv.txt` on the target root (today's mechanism) where `fw_setenv` is
  unavailable.
- **Raspberry Pi (tryboot + dual firmware)**: `autoboot.txt` selects the active
  firmware partition; `tryboot_a_b` boots the alternate on a try with native
  GPU-firmware fallback.

## Rollback — two layers

1. **Pre-Linux / firmware** (kernel panic, unbootable): x86 GRUB `fallback`;
   Rockchip u-boot `bootcount`/`altbootcmd`; Pi tryboot auto-fallback.
2. **Health-gate commit** (boots but unhealthy): new `airwaves-slot-confirm.service`
   (after `airwaves-containers.service`) reuses `wait_healthy()` — commit on
   healthy-try, reboot-to-revert on unhealthy-try; writes `/etc/airwaves/ab/last-upgrade.json`.

## Artifact delivery (reuse existing manifest pipeline)

Per-platform rootfs tarball `airwaves-os-<platform>-<version>.rootfs.tar.zst`
(+sha256), published as a tag-pinned GitHub Release asset. Manifest gains an
additive `os.ab_image {version,platform,url,sha256,size_mb}`. Manager `apply()`
learns an `os_ab` component that flows url/sha256/version into `request.json`
(mirroring `compose_url`/`compose_sha256`). Updater `download()` fetches to
`aw-data`; the new os-upgrade phase extracts into the inactive slot. Containers
are unchanged (pulled/cache-reused on the new slot, as today).

## Decisions (final)

1. **Rockchip rollback**: add `libubootenv` (`fw_setenv`) for a real pre-Linux
   bootcount, **rk3588 only**; graceful fallback to `armbianEnv.txt` rewrite.
2. **Min disk for A/B**: **≥ 12 GiB** → A/B; smaller → existing single-root + in-place updater.
3. **Health gate**: container health only (reuse `wait_healthy()`) for MVP; add network, then SDR probe later.
4. **Upgrade semantics**: **keep BOTH** — A/B image path for full-OS/major upgrades, in-place apt for minor package patches. (Single-root devices keep in-place only.)
5. **Reboot after staging**: **prompt** (operator picks the moment); opt-in auto/scheduled later.

## Phased plan (MVP = x86 first)

0. **CI**: tar the already-built rootfs per platform/channel → `.rootfs.tar.zst` (+sha256), publish as a tag-pinned release asset.
1. **Installer A/B layout + slot-manager skeleton**: `--ab` path in `airwaves-install` (`partition_*_ab`, per-slot fstab from `aw-data`, snapshot rootA→rootB so both boot; Pi dual firmware); bootloader A/B variants; `airwaves-slot-manager` verbs. Single-root stays the <12 GiB fallback.
2. **Health-gate commit/rollback**: `airwaves-slot-confirm.service` (reuse `wait_healthy()`), systemd watchdog, Rockchip `libubootenv`.
3. **Updater `os_ab` phase**: `download()`+extract to inactive slot, write per-slot boot config, `slot-manager set-try`, `REBOOT_REQUIRED`.
4. **Manager + manifest**: `os.ab_image` + `AbImage`; `apply()` emits `os_ab_*`; platform-select asset; expose active/previous slot + `/system/update/rollback`.
5. **Console TUI integration**: route Upgrade through `os_ab` on A/B devices (minor patches still via apt); add Repair→roll-back-to-previous-slot; show active/previous slot + last rollback reason.
6. **Hardware soak**: x86 (VM+box), Rock 5B, Orange Pi 5, Pi 4/5 — normal upgrade→commit, forced-unhealthy→auto-revert, power-loss during extract/health window, repeated A/B/A; then add network/SDR health probes.

## Top risks

Rockchip u-boot env writability (validate `fw_setenv` early; fall back to
armbianEnv rewrite). Pi tryboot firmware-version dependence (dual firmware
removes brick-both; check min firmware). `aw-data` SPoF (backups + later btrfs).
Config/OS schema skew across slots (keep config back-compat within a slot pair).
Power loss mid-extract/commit (extract to inactive only; verify sha256 before;
atomic boot-var writes; never commit until health passes). Tarball size/cost
(zstd + GitHub Releases + prune old assets).
