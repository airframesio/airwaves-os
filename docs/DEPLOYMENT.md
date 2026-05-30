# Deploying Airwaves OS

## Disk size (important for VMs)

Airwaves OS runs everything in containers and updates in place, so it needs
headroom for image pulls and OS package upgrades.

- **Minimum: 16 GB.** Recommended: 32 GB+, especially if you'll run several
  decoder/feeder apps.
- The published OS image is small (~5 GB). On a physical disk or SD card,
  Airwaves OS **auto-expands** the root filesystem to fill the medium on boot.

### Proxmox / VMware note
`qm importdisk` (Proxmox) and raw-image imports create the VM disk at the
**image's size** (~5 GB) — that is *not* enough. After importing:

1. Enlarge the virtual disk to ≥16 GB:
   - **Proxmox:** *VM → Hardware → Hard Disk → Disk Action → Resize* (e.g. +16G).
   - **VMware Fusion:** *Settings → Hard Disk → expand*.
2. **Reboot.** Airwaves OS's `airwaves-growfs` service grows the root partition
   and filesystem to fill the new size automatically — no manual steps.

`airwaves-growfs` runs on every boot and is idempotent (a no-op once the
partition already fills the disk), so you can resize again later the same way.

### Manual grow (if needed)
If you can't reboot, expand live (root is `sda3` on x86 UEFI images — confirm
with `lsblk` / `findmnt /`):

```bash
sudo growpart /dev/sda 3      # install cloud-guest-utils if missing
sudo resize2fs /dev/sda3
df -h /
```

## Booting in a VM (UEFI)

The x86 image boots via UEFI/GRUB. The VM must use **OVMF (UEFI)** firmware, not
SeaBIOS, and needs an EFI disk.

Proxmox example:
```bash
qm create 200 --name airwaves --bios ovmf --machine q35 \
  --efidisk0 local-lvm:1,format=raw,efitype=4m,pre-enrolled-keys=0 \
  --memory 2048 --cores 2 --cpu host --net0 virtio,bridge=vmbr0 --ostype l26
qm importdisk 200 Airwaves_OS_*.img local-lvm
qm set 200 --scsihw virtio-scsi-single --scsi0 local-lvm:vm-200-disk-1
qm set 200 --boot order=scsi0
# Then enlarge the disk (see above) before/after first boot.
```

## First boot

Fully unattended:
- The `airwaves` user (password `airwaves`) is pre-created; no setup wizard runs.
- `airwaves-init` generates a device ID, sets the hostname from the MAC, and
  starts the container stack (pulling gateway + manager from GHCR if needed).
- Open `http://<ip>` or `http://airwaves-XXXXXX.local`.

See [UPDATES.md](UPDATES.md) for keeping the system current.
