# Airwaves OS

Radio-focused operating system based on Armbian. Tagline: "Radio software that just works."

## Architecture

```
Host OS (Armbian minimal, BUILD_MINIMAL=yes)
├── Kernel + systemd + Docker
├── systemd-networkd + avahi (mDNS: airwaves-XXXXXX.local)
├── SDR udev rules (RTL-SDR, Airspy, HackRF, SDRplay, Funcube)
└── Docker containers
    ├── airwaves-gateway  (nginx, port 80, serves control app, proxies /api/ + /ws/)
    ├── airwaves-manager  (Rust/axum, port 8080, 24 REST endpoints + WebSocket)
    └── [decoder/feeder apps from catalog]
```

## Repository Structure

```
armbian/
  build.sh                     # Clones armbian/build at pinned tag, runs compile
  userpatches/
    common-airwaves.conf       # Core build config (BUILD_MINIMAL=yes, VENDOR=Airwaves_OS)
    config-airwaves.conf       # Board-specific overrides
    targets.yaml               # Tier 1 boards: RPi 4B/5, Rock 5B, OPi5, x86 UEFI
    extensions/
      airwaves-base.sh         # Users, MOTD, config dirs, init service
      airwaves-docker.sh       # Docker CE, compose, container lifecycle
      airwaves-networking.sh   # systemd-networkd, avahi/mDNS
      airwaves-hardware.sh     # SDR udev rules, kernel module blacklist
      airwaves-os/             # Extension data: config files, scripts, templates
containers/
  airwaves-gateway/            # Nginx reverse proxy + control app static files
  airwaves-manager/            # Rust API (axum + bollard + sysinfo + reqwest)
catalog/                       # App catalog definitions
releases/                      # Release manifests
```

## Manager API (containers/airwaves-manager)

Rust project with hexagonal architecture:
- `src/domain/` - Data models (ContainerInfo, SystemInfo, SdrDevice, FeedConfig, etc.)
- `src/ports/` - Trait interfaces (DockerPort, SystemPort, HardwarePort, ConfigPort)
- `src/adapters/` - Implementations (DockerAdapter via bollard, SystemAdapter via sysinfo)
- `src/handlers/` - HTTP handlers (system, containers, hardware, network, config, apps, feeds, tracking, fleet, exec)
- `src/ws/` - WebSocket event types
- `src/main.rs` - Router, state, background tasks (stats broadcaster, Docker event watcher)

24 endpoints total. Config stored at `/etc/airwaves/config.json`. Catalog at `/etc/airwaves/catalog.json`.

## Conventions

- **Naming**: `airwaves` prefix everywhere. Config: `/etc/airwaves/`. Containers: `airwaves-*`.
- **Releases**: Trixie (primary) + Bookworm (legacy) + Noble (RPi boards requiring flash-kernel)
- **Tier 1 boards**: RPi 4B, Rock 5B, Orange Pi 5, x86 UEFI
- **Armbian build tag**: pinned in `armbian/build.sh` (currently v26.2.1)
- **All user software runs in containers** - host OS is minimal

## Development

```bash
# Start the manager locally
make dev-up          # Builds and runs manager container at localhost:8080

# Start the control app (separate repo: airframesio/airwaves-os-control)
cd ../airwaves-os-control && npm run dev   # Vite proxies /api/v1/* to manager

# Check Rust compilation
make check

# Build OS image (requires Linux)
make build-x86       # x86 UEFI
make build-rpi4      # Raspberry Pi 4B
```

## Control App (airframesio/airwaves-os-control)

React + Vite + Wouter + shadcn/ui + TanStack Query. NOT Next.js.
- API client: `client/src/lib/api.ts`
- TanStack Query hooks: `client/src/hooks/useAirwavesApi.ts`
- WebSocket hook: `client/src/hooks/useManagerEvents.ts`
- API detection: `client/src/hooks/useApiStatus.ts`
- All pages fall back to mock data when API unavailable
