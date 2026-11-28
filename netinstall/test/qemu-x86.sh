#!/usr/bin/env bash
#
# Boot the x86_64 network installer in QEMU (UEFI, OVMF) against the mock
# Desktop and check what it wrote to the virtual disk.
#
#   netinstall/test/qemu-x86.sh --tar out/netinstall-x86_64.tar --key test.key \
#       [--image Airwaves_OS_..._Uefi-x86_....img.xz] [--case ok|wrong-sha|wrong-target|tampered]
#
# --key is the minisign secret key (no password) matching the minisign.pub the
# installer was built with. Without --image a small stand-in image (GPT with
# an ESP and an AWCFG partition) is made. Needs qemu-system-x86, ovmf,
# minisign, gdisk, dosfstools, mtools, python3.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TAR="" KEY="" IMAGE="" CASE=ok DISK_GB=8 MEM=2048
while [ $# -gt 0 ]; do
    case "$1" in
        --tar) TAR="$2"; shift 2 ;;
        --key) KEY="$2"; shift 2 ;;
        --image) IMAGE="$2"; shift 2 ;;
        --case) CASE="$2"; shift 2 ;;
        --disk-gb) DISK_GB="$2"; shift 2 ;;
        *) echo "unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -f "$TAR" ] && [ -f "$KEY" ] || { echo "usage: $0 --tar <netinstall-x86_64.tar> --key <minisign key> [--image img.xz] [--case ok|wrong-sha|wrong-target|tampered]" >&2; exit 2; }

W="$(mktemp -d)"
echo "work dir: $W"
tar -C "$W" -xf "$TAR"
S="$W/serve"; mkdir -p "$S"
cp "$W/netinstall-x86_64/vmlinuz" "$W/netinstall-x86_64/initramfs.img" "$S/"

if [ -n "$IMAGE" ]; then
    NAME="$(basename "$IMAGE")"
    ln -s "$(realpath "$IMAGE")" "$S/$NAME"
else
    NAME="Airwaves_OS_0.0.0-test_Uefi-x86_trixie_current_minimal.img.xz"
    RAW="$W/test.img"
    truncate -s 96M "$RAW"
    sgdisk -o "$RAW" >/dev/null
    sgdisk -n 1:0:+32M -t 1:EF00 -c 1:efi -n 2:0:+16M -t 2:8300 -c 2:rootfs \
        -n 3:0:+32M -t 3:0700 -c 3:AWCFG "$RAW" >/dev/null
    off() { sgdisk -i "$1" "$RAW" | sed -n 's/^First sector: \([0-9]*\).*/\1/p'; }
    len() { sgdisk -i "$1" "$RAW" | sed -n 's/^Partition size: \([0-9]*\).*/\1/p'; }
    for n in 1 3; do
        truncate -s $(( $(len $n) * 512 )) "$W/p$n"
        mkfs.vfat -n "$([ $n = 1 ] && echo EFI || echo AWCFG)" "$W/p$n" >/dev/null
    done
    echo "placeholder" > "$W/BOOTX64.EFI"
    mmd -i "$W/p1" ::/EFI ::/EFI/BOOT && mcopy -i "$W/p1" "$W/BOOTX64.EFI" ::/EFI/BOOT/
    echo '{"schema":1,"hostname":""}' > "$W/airwaves-install.json"
    mcopy -i "$W/p3" "$W/airwaves-install.json" ::/
    for n in 1 3; do dd if="$W/p$n" of="$RAW" bs=512 seek="$(off $n)" conv=notrunc status=none; done
    xz -T0 -c "$RAW" > "$S/$NAME"
fi

(cd "$S" && sha256sum "$NAME" vmlinuz initramfs.img > SHA256SUMS)
minisign -S -s "$KEY" -m "$S/SHA256SUMS" -t "Airwaves OS test release" >/dev/null

MOCKARGS=( --dir "$S" --image "$NAME" --log "$W/events.jsonl" --waits 1 )
[ "$CASE" = wrong-sha ] && MOCKARGS+=( --wrong-sha )
[ "$CASE" = wrong-target ] && MOCKARGS+=( --wrong-target )
if [ "$CASE" = tampered ]; then
    # Signed checksum and plan agree, but the bytes served are different.
    MOCKARGS+=( --sha "$(sha256sum "$S/$NAME" | cut -d' ' -f1)" )
    xz -dc "$S/$NAME" > "$W/t.img"; printf 'tampered' | dd of="$W/t.img" bs=1 seek=4096 conv=notrunc status=none
    rm -f "$S/$NAME"; xz -T0 -c "$W/t.img" > "$S/$NAME"
fi
python3 "$HERE/mock-desktop.py" "${MOCKARGS[@]}" 2>"$W/mock.log" &
MOCK=$!
trap 'kill $MOCK 2>/dev/null || true' EXIT
sleep 1

DISK="$W/disk.img"
truncate -s "${DISK_GB}G" "$DISK"
# Give the disk an old partition table so we can see it get replaced.
sgdisk -o "$DISK" >/dev/null && sgdisk -n 1:0:+100M -c 1:olddata "$DISK" >/dev/null
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$W/vars.fd"
ACCEL=tcg; [ -w /dev/kvm ] && ACCEL=kvm

echo "booting the installer (case: $CASE, accel: $ACCEL)..."
timeout 3600 qemu-system-x86_64 -machine q35,accel=$ACCEL -m "$MEM" -smp 2 \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
    -drive if=pflash,format=raw,file="$W/vars.fd" \
    -kernel "$S/vmlinuz" -initrd "$S/initramfs.img" \
    -append "console=ttyS0,115200 panic=5 awnet.url=http://10.0.2.2:47811/netboot/v1 awnet.token=test-token-0123456789" \
    -drive file="$DISK",format=raw,if=none,id=d0 -device virtio-blk-pci,drive=d0,serial=QEMUDISK0001 \
    -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
    -nographic -no-reboot -serial mon:stdio </dev/null | tee "$W/console.log" | sed 's/^/  | /'

echo
echo "== progress stages reported =="
jq -r 'select(.kind=="progress") | .stage + (if .error then " (" + .error + ")" else "" end)' "$W/events.jsonl" | uniq
echo "== disk after install =="
sgdisk -p "$DISK" | tail -n +8

result=0
check() { if eval "$2"; then echo "PASS: $1"; else echo "FAIL: $1"; result=1; fi; }
first_hello="$(jq -c 'select(.kind=="hello") | .body' "$W/events.jsonl" | head -1)"
check "hello reports the virtio disk with its serial" \
    "echo '$first_hello' | jq -e '.disks[] | select(.path==\"/dev/vda\" and .serial==\"QEMUDISK0001\")' >/dev/null"
check "hello reports UEFI x86_64" "echo '$first_hello' | jq -e '.boot==\"uefi\" and .arch==\"x86_64\"' >/dev/null"

if [ "$CASE" = ok ]; then
    check "stages in order" "[ \"\$(jq -r 'select(.kind==\"progress\") | .stage' \"$W/events.jsonl\" | uniq | tr '\n' ' ')\" = 'booted waiting verifying_manifest countdown writing verifying configuring rebooting ' ]"
    start=$(sgdisk -i 3 "$DISK" | sed -n 's/^First sector: \([0-9]*\).*/\1/p')
    if [ -n "$IMAGE" ]; then start=$(sgdisk -p "$DISK" | awk '$NF=="AWCFG" {print $2}'); fi
    count=$(sgdisk -p "$DISK" | awk '$NF=="AWCFG" {print $3 - $2 + 1}')
    dd if="$DISK" of="$W/awcfg.vfat" bs=512 skip="$start" count="$count" status=none
    mtype -i "$W/awcfg.vfat" ::/airwaves-install.json > "$W/awcfg.json"
    echo "== AWCFG airwaves-install.json =="; cat "$W/awcfg.json"
    check "AWCFG has the plan's hostname" "jq -e '.hostname==\"airwaves-qemu\"' '$W/awcfg.json' >/dev/null"
    check "AWCFG has the enroll and station blocks" "jq -e '.enroll.code and .station.ident==\"XX-QEMU\"' '$W/awcfg.json' >/dev/null"
    check "destructive flags forced off" "jq -e '.auto_install==false and .self_install==false' '$W/awcfg.json' >/dev/null"
    check "old partition is gone" "! sgdisk -p '$DISK' | grep -q olddata"
    check "UEFI boot entry added" "grep -q 'boot entry added' '$W/console.log'"
else
    check "install refused with the right error" \
        "jq -e 'select(.kind==\"progress\" and .stage==\"failed\")' '$W/events.jsonl' >/dev/null"
    case "$CASE" in
        wrong-target) err=target_mismatch ;; wrong-sha) err=image_checksum ;; tampered) err=image_checksum ;;
    esac
    check "error code is $err" "jq -e --arg e $err 'select(.kind==\"progress\" and .stage==\"failed\" and .error==\$e)' '$W/events.jsonl' >/dev/null"
    if [ "$CASE" = tampered ]; then
        check "disk cleared after the streamed checksum mismatch" "! sgdisk -p '$DISK' 2>/dev/null | grep -q -e AWCFG -e olddata"
    else
        check "disk untouched (old partition still there)" "sgdisk -p '$DISK' | grep -q olddata"
    fi
fi
echo "logs in $W"
exit $result
