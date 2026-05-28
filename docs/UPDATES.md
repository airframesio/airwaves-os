# Airwaves OS System Updater

Airwaves OS updates in layers, all surfaced in the control app under
**Settings → System Update** (`/updates`):

| Layer | What it is | How it updates |
|---|---|---|
| **System Manager** | `ghcr.io/airframesio/airwaves-manager` (Rust API) | pull image, recreate container |
| **Control Panel** | `ghcr.io/airframesio/airwaves-gateway` (nginx + the React control app) | pull image, recreate container |
| **System Configuration** | `/etc/airwaves/docker-compose.yml` | download from manifest (sha256-verified), atomic replace |
| **App Catalog** | `/etc/airwaves/catalog.json` | download from manifest (sha256-verified), atomic replace |
| **OS packages / kernel** | Debian/apt packages | `apt-get upgrade` (reboot-flagged if kernel changed) |
| **Major OS upgrade** | e.g. Bookworm → Trixie | in-place `apt full-upgrade` after repointing apt sources, then reboot |

## Severity

Each update carries a severity, shown as a badge and used to drive the global
banner:

- **nice-to-have** — optional, quality-of-life.
- **recommended** — preferred; apply soon.
- **required** — necessary (security/compat). Surfaced as a red banner across
  the whole app until applied.

## Release manifest

The device checks a JSON manifest for the configured channel:

```
https://raw.githubusercontent.com/airframesio/airwaves-os/main/releases/<channel>.json
```

(Default channel `stable` → `releases/stable.json`.) Schema (`schema: 1`):

```jsonc
{
  "schema": 1,
  "channel": "stable",
  "os_version": "1.0.0",
  "codename": "Sideband",
  "released": "2026-05-28",
  "severity": "recommended",          // rollup = max of components
  "min_os_version": "1.0.0",
  "summary": "…",
  "notes_url": "…",
  "components": {
    "manager":  { "version": "1.0.0", "severity": "recommended", "image": "ghcr.io/…", "tag": "latest" },
    "gateway":  { "version": "1.0.0", "control_app_version": "1.0.0", "severity": "recommended", "image": "ghcr.io/…", "tag": "latest" },
    "compose":  { "version": "1", "severity": "required", "url": "…/docker-compose.yml.template", "sha256": "…" },
    "catalog":  { "version": "1", "severity": "nice-to-have", "url": "…/catalog.json", "sha256": "…" }
  },
  "os": {
    "major_upgrade": null,            // or { "from": "bookworm", "to": "trixie", "severity": "recommended", "guide_url": "…" }
    "reboot_expected": false
  }
}
```

The manager overrides the source with env vars when needed:
`AIRWAVES_MANIFEST_URL` (full URL) or `AIRWAVES_MANIFEST_BASE` (base dir).

## How version detection works

- **manager** — `env!("CARGO_PKG_VERSION")` baked into the binary.
- **control app** — the gateway serves `/version.json` (stamped at image build
  from the control-app `package.json` via the `CONTROL_APP_VERSION` build-arg);
  the manager fetches `http://airwaves-gateway/version.json`.
- **compose / catalog** — integer revisions tracked in `/etc/airwaves/.versions.json`.
- **OS** — `/etc/os-release` (codename) + `/etc/airwaves-release` (Airwaves
  version); upgradable package count via `apt-get -s upgrade`.

`semver` is used to compare versions; non-semver strings fall back to string
inequality. `unknown`/empty never counts as an update.

## How applying works (the host updater)

The manager runs *inside* the compose stack, so it cannot recreate itself.
Instead all host mutations go through one audited path:

1. The manager writes a job spec to `/etc/airwaves/update/request.json` and
   triggers `systemctl start airwaves-update.service` (via `nsenter` into the
   host PID namespace — the manager already runs with `pid: host` +
   `HOST_VIA_NSENTER=1`).
2. `/opt/airwaves/scripts/airwaves-update` runs as **host root**, outside the
   container, writing incremental progress to `/etc/airwaves/update/status.json`.
3. Because the status file lives on the host bind-mount, it **survives the
   manager being recreated** mid-update. The control app polls
   `GET /api/v1/system/update/progress` and reconnects automatically.

Steps: snapshot config + image digests → download/verify (sha256) compose &
catalog → `docker compose pull` → `docker compose up -d --remove-orphans` →
**health-gate** (wait for manager + gateway healthy) → on failure **roll back**
config and re-`up` the previous stack → apt upgrade / major upgrade → set
`reboot_required`.

Dry-run: set `AIRWAVES_UPDATE_DRYRUN=1` to log actions without mutating.

## API (manager)

| Method | Path | Purpose |
|---|---|---|
| GET  | `/api/v1/system/update/status`   | cached check result |
| POST | `/api/v1/system/update/check`    | force a fresh manifest fetch |
| POST | `/api/v1/system/update/apply`    | `{ "components": ["manager","gateway","compose","catalog","os_packages","os_major"] }` or `["all"]` |
| GET  | `/api/v1/system/update/progress` | host updater progress |

A background task re-checks every 12h and broadcasts a `UpdateAvailable`
WebSocket event when a recommended/required update appears.

## Publishing a release

1. Bump component versions (manager `Cargo.toml`, control-app `package.json`,
   `/etc/airwaves-release`, and the `.versions.json` seed in
   `airwaves-base.sh` for compose/catalog as needed).
2. Build & push the container images (CI: **Build Containers**).
3. Recompute `sha256` for the compose template + catalog and update
   `releases/stable.json` (and archive a copy as `releases/<version>.json`).
4. Commit/push. `stable.json` points at `main` (rolling latest); versioned
   manifests should pin their `url`s to the release tag once tags are cut
   (see ROADMAP).

## Bootstrapping existing devices

Devices flashed before the updater existed don't have the host script or the
`pid: host` compose yet. Run once, as root, on the device:

```bash
curl -fsSL https://raw.githubusercontent.com/airframesio/airwaves-os/main/scripts/bootstrap-updater.sh | sudo bash
```

This installs the updater + unit, refreshes `docker-compose.yml`, pulls images,
and brings the stack up. After that, updates are fully in-app.

## Limitations

- Container image rollback is best-effort (compose pins `:latest`); config-file
  rollback is exact. Atomic, rollback-safe OS updates are tracked for A/B
  partitions — see [ROADMAP.md](ROADMAP.md).
- Major in-place Debian upgrades are powerful but inherently risky without A/B
  partitions; back up first.
