# Airwaves OS

[![Build OS Image](https://github.com/airframesio/airwaves-os/actions/workflows/build-os-image.yml/badge.svg)](https://github.com/airframesio/airwaves-os/actions/workflows/build-os-image.yml)
[![Build Containers](https://github.com/airframesio/airwaves-os/actions/workflows/build-containers.yml/badge.svg)](https://github.com/airframesio/airwaves-os/actions/workflows/build-containers.yml)
![CodeRabbit Pull Request Reviews](https://img.shields.io/coderabbit/prs/github/airframesio/airwaves-os)
[![Contributors](https://img.shields.io/github/contributors/airframesio/airwaves-os)](https://github.com/airframesio/airwaves-os/graphs/contributors)
[![Activity](https://img.shields.io/github/commit-activity/m/airframesio/airwaves-os)](https://github.com/airframesio/airwaves-os/pulse)
[![Discord](https://img.shields.io/discord/1067697487927853077?logo=discord)](https://discord.gg/8Ksch7zE)

Radio software that just works.

Airwaves OS is a radio-focused operating system based on [Armbian](https://www.armbian.com/) that runs on embedded computers, mini PCs, and servers. It provides a fully pre-configured environment for receiving and decoding radio signals, with all services running as Docker containers.

**Website:** [airwavesos.com](https://airwavesos.com)

## Supported Hardware

### Tier 1 (actively tested)

| Board | Architecture | Release |
|-------|-------------|---------|
| Raspberry Pi 4B | arm64 | Ubuntu Noble |
| Raspberry Pi 5 | arm64 | Ubuntu Noble |
| Rock 5B | arm64 | Debian Bookworm |
| Orange Pi 5 | arm64 | Debian Bookworm |
| x86 UEFI (mini PCs, servers) | amd64 | Debian Bookworm |

### SDR Support

Out-of-the-box udev rules and driver support for:
- RTL-SDR (Blog V3/V4, generic RTL2832U)
- Airspy Mini / R2 / HF+
- HackRF One
- SDRplay RSP series
- FlightAware Pro Stick / Pro Stick Plus
- Funcube Dongle Pro / Pro+

## Architecture

```
Host OS (Armbian minimal)
├── Kernel + systemd + Docker
├── systemd-networkd + avahi (mDNS)
├── SDR udev rules + driver packages
└── Docker containers
    ├── airwaves-gateway (nginx reverse proxy, port 80)
    ├── airwaves-manager (system management API, future)
    └── [decoder/feeder apps installed via app store]
```

All user-facing services run as Docker containers. The host OS is minimal (`BUILD_MINIMAL=yes`) with only the kernel, systemd, Docker, networking, and hardware drivers.

## Repository Structure

```
airwaves-os/
├── armbian/
│   ├── build.sh                    # Build entry point (clones armbian/build at pinned tag)
│   └── userpatches/                # Armbian customization layer
│       ├── extensions/
│       │   ├── airwaves-base.sh    # Identity, users, MOTD, config
│       │   ├── airwaves-docker.sh  # Docker CE + container infrastructure
│       │   ├── airwaves-networking.sh  # systemd-networkd, avahi, mDNS
│       │   ├── airwaves-hardware.sh    # SDR udev rules, driver packages
│       │   └── airwaves-os/        # Extension data (config files, scripts, templates)
│       ├── common-airwaves.conf    # Core build configuration
│       ├── config-airwaves.conf    # Board-specific overrides
│       └── targets.yaml            # Build matrix definition
├── containers/
│   └── airwaves-gateway/           # Nginx reverse proxy container
├── catalog/                        # App catalog definitions
└── releases/                       # Release manifests
```

## Building

### Prerequisites

- Linux system (Ubuntu 24.04 recommended) or Docker
- 8GB+ RAM, 50GB+ free disk space
- Root/sudo access

### Build an image

```bash
# Build for a specific board
./armbian/build.sh airwaves BOARD=rock-5b BRANCH=current RELEASE=bookworm

# Build for Raspberry Pi 4B
./armbian/build.sh airwaves BOARD=rpi4b BRANCH=current RELEASE=noble

# Build for x86 (mini PC / server)
./armbian/build.sh airwaves BOARD=uefi-x86 BRANCH=current RELEASE=bookworm
```

The build script clones `armbian/build` at a pinned tag and applies our userpatches automatically.

### Build containers

```bash
cd containers/airwaves-gateway
docker buildx build --platform linux/amd64,linux/arm64 -t airwaves-gateway .
```

## First Boot

1. Flash the image to an SD card or USB drive
2. Boot the device
3. The system automatically:
   - Generates a unique hostname (`airwaves-XXXXXX` from MAC address)
   - Loads pre-baked container images from disk
   - Starts the gateway container on port 80
   - Advertises itself via mDNS
4. Access the web interface at `http://airwaves-XXXXXX.local`

Default credentials: `airwaves` / `airwaves`

## Development

This project uses the Armbian build framework's [userpatches system](https://docs.armbian.com/Developer-Guide_User-Configurations/) for OS customization and Docker for all application services.

Key docs:
- [Armbian Build Documentation](https://docs.armbian.com/Developer-Guide_Build-Preparation/)
- [Armbian Extensions](https://docs.armbian.com/Developer-Guide_Extensions/)

## License

See [LICENSE](LICENSE) for details.
