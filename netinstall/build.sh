#!/usr/bin/env bash
#
# Build the Airwaves OS network installer (kernel + initramfs) from a built
# Airwaves OS image's root filesystem, so the installer runs the same kernel,
# drivers and tools as the OS it installs.
#
#   netinstall/build.sh --rootfs <dir> --arch x86_64|arm64 --out <dir> \
#       [--version 1.0.39] [--ipxe /usr/lib/ipxe/snponly.efi] [--pubkey test.pub]
#
# <dir> is the image's mounted rootfs (read only is fine). Output:
#   x86_64: <out>/netinstall-x86_64.tar
#   arm64:  <out>/netinstall-rpi-arm64.tar   (Raspberry Pi 4 family)
#
# Works across architectures (an arm64 rootfs on an x86 runner): libraries are
# found by reading ELF headers, nothing from the rootfs is executed.
# See docs/NETBOOT.md for the layout and the boot chain.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOTFS="" ARCH="" OUT="" VERSION="" IPXE="/usr/lib/ipxe/snponly.efi" PUBKEY="$HERE/minisign.pub"
while [ $# -gt 0 ]; do
    case "$1" in
        --rootfs) ROOTFS="$2"; shift 2 ;;
        --arch) ARCH="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        --ipxe) IPXE="$2"; shift 2 ;;
        --pubkey) PUBKEY="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[ -d "$ROOTFS" ] && [ -n "$ARCH" ] && [ -n "$OUT" ] || {
    echo "usage: $0 --rootfs <dir> --arch x86_64|arm64 --out <dir> [--version V] [--ipxe file]" >&2; exit 2; }
for t in readelf depmod cpio xz jq; do
    command -v "$t" >/dev/null || { echo "missing tool: $t" >&2; exit 2; }
done
if [ -z "$VERSION" ]; then
    VERSION="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' \
        "$HERE/../containers/airwaves-manager/Cargo.toml" | head -1)"
fi

case "$ARCH" in
    x86_64) TRIPLET=x86_64-linux-gnu; NAME=netinstall-x86_64; BOARD=uefi-x86 ;;
    arm64)  TRIPLET=aarch64-linux-gnu; NAME=netinstall-rpi-arm64; BOARD=rpi4b ;;
    *) echo "arch must be x86_64 or arm64" >&2; exit 2 ;;
esac

KVER="$(ls "$ROOTFS/lib/modules" | sort -V | tail -1)"
[ -n "$KVER" ] || { echo "no kernel modules in $ROOTFS/lib/modules" >&2; exit 1; }
echo "==> ${NAME} ${VERSION}, kernel ${KVER}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
IR="$WORK/initramfs"
mkdir -p "$IR"/{etc,proc,sys,dev,run,tmp,mnt,root,usr/bin,usr/sbin,usr/share/airwaves}
ln -s usr/bin "$IR/bin"; ln -s usr/sbin "$IR/sbin"
ln -s usr/lib "$IR/lib"
mkdir -p "$IR/usr/lib"
[ "$ARCH" = x86_64 ] && { ln -s usr/lib64 "$IR/lib64"; mkdir -p "$IR/usr/lib64"; }

LIBDIRS=( "/usr/lib/$TRIPLET" "/lib/$TRIPLET" "/usr/lib" "/lib" "/usr/lib64" "/lib64" )

# Path of a file inside the rootfs, following symlinks within it.
in_root() {
    local p="$1" n=0 t
    while [ -L "$ROOTFS$p" ] && [ $n -lt 20 ]; do
        t="$(readlink "$ROOTFS$p")"
        case "$t" in /*) p="$t" ;; *) p="$(dirname "$p")/$t" ;; esac
        n=$((n + 1))
    done
    p="$(realpath -m "$p")"
    [ -e "$ROOTFS$p" ] && echo "$p"
}

declare -A COPIED=()
# Copy one rootfs file (by its path in the image) to the same path in the
# initramfs, as a real file, then its shared libraries.
copy_elf() {
    local path="$1" real dest interp lib
    [ -n "${COPIED[$path]:-}" ] && return 0
    COPIED[$path]=1
    real="$(in_root "$path")" || { echo "missing in rootfs: $path" >&2; return 1; }
    dest="$IR/${path#/}"
    mkdir -p "$(dirname "$dest")"
    cp -L "$ROOTFS$real" "$dest"
    readelf -h "$dest" >/dev/null 2>&1 || return 0
    interp="$(readelf -l "$dest" 2>/dev/null | sed -n 's/.*Requesting program interpreter: \(.*\)]/\1/p')"
    [ -n "$interp" ] && copy_elf "$interp"
    for lib in $(readelf -d "$dest" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
        copy_lib "$lib"
    done
}
copy_lib() {
    local lib="$1" d
    for d in "${LIBDIRS[@]}"; do
        if [ -e "$ROOTFS$d/$lib" ]; then copy_elf "$d/$lib"; return 0; fi
    done
    echo "warning: library $lib not found in rootfs" >&2
}
find_bin() {
    local b="$1" d
    for d in /usr/bin /usr/sbin /bin /sbin; do
        [ -e "$ROOTFS$d/$b" ] && { echo "$d/$b"; return 0; }
    done
    return 1
}

# ---- programs ---------------------------------------------------------------
BINS=( busybox curl jq pv xz openssl kmod sfdisk blkid partprobe wipefs lsblk blockdev )
[ "$ARCH" = x86_64 ] && BINS+=( efibootmgr )
for b in "${BINS[@]}"; do
    p="$(find_bin "$b")" || { echo "required program $b not in rootfs" >&2; exit 1; }
    copy_elf "$p"
    [ "$p" = "/usr/bin/$b" ] || ln -sf "$p" "$IR/usr/bin/$b"
done
# busybox applets the installer uses (the real tools above take precedence).
for a in sh ash cat cp mv rm ln ls mkdir rmdir mount umount sleep dd sync \
         sed grep awk tr cut head tail wc sort uniq printf echo test [ basename dirname \
         readlink realpath stat date uname reboot poweroff ip ifconfig udhcpc \
         sha256sum mkfifo tee chvt openvt setsid getty clear dmesg mdev \
         switch_root kill ps od hexdump base64 seq find xargs env touch chmod \
         cmp true false expr timeout watch less vi wget nc ping mktemp; do
    [ -e "$IR/usr/bin/$a" ] || ln -sf busybox "$IR/usr/bin/$a"
done
for t in modprobe insmod lsmod depmod rmmod modinfo; do ln -sf kmod "$IR/usr/bin/$t"; done
# OpenSSL needs no config for what we use; keep it from looking for one.
mkdir -p "$IR/usr/lib/ssl"; : > "$IR/usr/lib/ssl/openssl.cnf"

# ---- kernel modules ---------------------------------------------------------
# Storage, wired network and USB drivers, plus everything they depend on
# (from the rootfs's modules.dep). Built-in drivers need nothing.
MODSRC="$ROOTFS/lib/modules/$KVER"
MODDST="$IR/usr/lib/modules/$KVER"
mkdir -p "$MODDST"
DIRS=( kernel/drivers/net/ethernet kernel/drivers/net/usb kernel/drivers/net/phy
       kernel/drivers/net/mdio kernel/drivers/net/pcs kernel/drivers/nvme kernel/drivers/ata
       kernel/drivers/mmc kernel/drivers/usb/host kernel/drivers/usb/storage kernel/drivers/usb/core
       kernel/drivers/usb/common kernel/drivers/scsi/sd_mod.ko kernel/drivers/scsi/virtio_scsi.ko
       kernel/drivers/block/virtio_blk.ko kernel/drivers/net/virtio_net.ko kernel/drivers/virtio
       kernel/drivers/hid kernel/drivers/input/keyboard kernel/drivers/gpu/drm/tiny/simpledrm.ko
       kernel/drivers/firmware/efi kernel/fs/nls kernel/fs/fat kernel/fs/efivarfs
       kernel/drivers/pci/controller kernel/drivers/phy )
# Skip high-end NIC families that need firmware blobs or never appear on a
# feeder box (keeps the initramfs small for TFTP).
SKIP_RE='/(mellanox|chelsio|qlogic|netronome|pensando|fungible|huawei|cavium|emulex|brocade|sfc|myricom|neterion|amazon|google|microsoft|meta|intel/(ice|i40e|iavf|idpf|fm10k|ixgbe|ixgbevf))/'
: > "$WORK/mods"
for d in "${DIRS[@]}"; do
    if [ -f "$MODSRC/$d" ]; then echo "$d" >> "$WORK/mods"
    elif [ -d "$MODSRC/$d" ]; then find "$MODSRC/$d" -name '*.ko*' -printf "$d/%P\n" >> "$WORK/mods"
    fi
done
grep -Ev "$SKIP_RE" "$WORK/mods" | sort -u > "$WORK/mods.sel"
# Transitive dependencies.
declare -A WANT=()
add_mod() {
    local m="$1" deps dep
    [ -n "${WANT[$m]:-}" ] && return 0
    WANT[$m]=1
    deps="$(grep -m1 "^${m}:" "$MODSRC/modules.dep" | cut -d: -f2-)"
    for dep in $deps; do add_mod "$dep"; done
}
while read -r m; do
    [ -n "$m" ] && [ -f "$MODSRC/$m" ] && add_mod "$m"
done < "$WORK/mods.sel"
for m in "${!WANT[@]}"; do
    mkdir -p "$MODDST/$(dirname "$m")"
    cp "$MODSRC/$m" "$MODDST/$m"
done
cp "$MODSRC"/modules.{order,builtin,builtin.modinfo} "$MODDST/" 2>/dev/null || true
depmod -b "$IR/usr" "$KVER" 2>/dev/null || depmod -b "$IR" "$KVER"
echo "    ${#WANT[@]} kernel modules"

# NIC firmware that common feeder boxes need (Realtek, Intel I225/I226 has none).
if [ -d "$ROOTFS/lib/firmware/rtl_nic" ]; then
    mkdir -p "$IR/usr/lib/firmware"
    cp -r "$ROOTFS/lib/firmware/rtl_nic" "$IR/usr/lib/firmware/"
fi

# ---- installer ----------------------------------------------------------------
install -m 0755 "$HERE/init" "$IR/init"
install -m 0755 "$HERE/udhcpc.script" "$IR/usr/share/airwaves/udhcpc.script"
install -m 0755 "$HERE/minisign-verify" "$IR/usr/bin/minisign-verify"
# The release signing key (docs/NETBOOT.md "Signatures"). --pubkey is for tests.
if [ -s "$PUBKEY" ]; then
    install -m 0644 "$PUBKEY" "$IR/usr/share/airwaves/minisign.pub"
else
    echo "warning: $PUBKEY missing: this installer will refuse every image" >&2
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::warning::netinstall/minisign.pub is missing; the network installer will refuse every image"
    fi
fi
cat > "$IR/etc/airwaves-netinstall" <<EOF
version=$VERSION
arch=$ARCH
board=$BOARD
kernel=$KVER
protocol=1
EOF
echo "root:x:0:0:root:/root:/bin/sh" > "$IR/etc/passwd"
echo "root:x:0:" > "$IR/etc/group"

# ---- pack -------------------------------------------------------------------------
PKG="$WORK/$NAME"
mkdir -p "$PKG"
(cd "$IR" && find . -print0 | LC_ALL=C sort -z | cpio --null -o -H newc --quiet -R 0:0) \
    | xz -9 --check=crc32 -T1 > "$WORK/initramfs.img"   # the kernel only unpacks CRC32 xz
CMDLINE="quiet"
case "$ARCH" in
    x86_64)
        cp "$ROOTFS/boot/vmlinuz-$KVER" "$PKG/vmlinuz"
        cp "$WORK/initramfs.img" "$PKG/initramfs.img"
        [ -f "$IPXE" ] || { echo "iPXE EFI binary not found at $IPXE (apt install ipxe)" >&2; exit 1; }
        cp "$IPXE" "$PKG/ipxe.efi"
        install -m 0644 "$HERE/boot.ipxe" "$PKG/boot.ipxe"
        # The last console= is /dev/console, where the installer talks: the screen.
        CMDLINE="console=ttyS0,115200 console=tty0 quiet"
        FILES='{"kernel":"vmlinuz","initramfs":"initramfs.img","bootloader":"ipxe.efi","script":"boot.ipxe"}'
        ;;
    arm64)
        T="$PKG/tftp"; mkdir -p "$T/overlays"
        FW="$ROOTFS/usr/lib/linux-firmware-raspi"
        for f in start4.elf fixup4.dat; do cp "$FW/$f" "$T/$f"; done
        DTB="$ROOTFS/boot/dtb-$KVER"; [ -d "$DTB" ] || DTB="$ROOTFS/boot/dtb"
        cp "$DTB"/broadcom/bcm2711-*.dtb "$T/"
        for o in vc4-kms-v3d dwc2 disable-wifi disable-bt; do
            [ -f "$DTB/overlays/$o.dtbo" ] && cp "$DTB/overlays/$o.dtbo" "$T/overlays/"
        done
        cp "$DTB/overlays/overlay_map.dtb" "$T/overlays/" 2>/dev/null || true
        # The firmware decompresses a gzip kernel itself.
        cp "$ROOTFS/boot/vmlinuz-$KVER" "$T/kernel8.img"
        cp "$WORK/initramfs.img" "$T/initramfs.img"
        install -m 0644 "$HERE/rpi/config.txt" "$T/config.txt"
        install -m 0644 "$HERE/rpi/cmdline.txt" "$T/cmdline.txt"
        CMDLINE="console=serial0,115200 console=tty1 quiet"
        FILES='{"kernel":"tftp/kernel8.img","initramfs":"tftp/initramfs.img","tftp_root":"tftp","cmdline":"tftp/cmdline.txt"}'
        ;;
esac
jq -n --arg arch "$ARCH" --arg board "$BOARD" --arg version "$VERSION" --arg kernel "$KVER" \
      --arg cmdline "$CMDLINE" --argjson files "$FILES" \
      '{schema:1, arch:$arch, board:$board, version:$version, kernel:$kernel, protocol:1, files:$files, cmdline:$cmdline}' \
      > "$PKG/manifest.json"
mkdir -p "$OUT"
tar -C "$WORK" --owner=0 --group=0 --numeric-owner -cf "$OUT/$NAME.tar" "$NAME"
echo "==> $OUT/$NAME.tar ($(du -h "$OUT/$NAME.tar" | cut -f1)); initramfs $(du -h "$WORK/initramfs.img" | cut -f1)"
