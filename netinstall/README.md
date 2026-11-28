# Network installer

Kernel + initramfs that Airframes Desktop network-boots to install Airwaves OS
onto a device's own disk. Built from a release image's rootfs by `build.sh`
(see `build-image-simple.yml`, step "Build network installer"); `init` is the
installer itself; `test/` boots it in QEMU against a mock Desktop.

The boot chain, the HTTP API with Desktop, signatures and testing are in
[docs/NETBOOT.md](../docs/NETBOOT.md).
