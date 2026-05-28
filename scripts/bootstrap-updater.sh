#!/usr/bin/env bash
#
# Airwaves OS updater bootstrap.
#
# One-time installer for devices that predate the in-app system updater. It
# installs the host updater script + systemd unit, seeds the version markers,
# refreshes docker-compose.yml to the latest, and brings the stack up. After
# running this once, all future updates can be done from the control app
# (Settings → System Update).
#
# Usage (on the device, as root):
#   curl -fsSL https://raw.githubusercontent.com/airframesio/airwaves-os/main/scripts/bootstrap-updater.sh | sudo bash
#
set -euo pipefail

RAW="https://raw.githubusercontent.com/airframesio/airwaves-os/main"
EXT="${RAW}/armbian/userpatches/extensions/airwaves-os"
AIRWAVES_DIR="/etc/airwaves"
SCRIPTS_DIR="/opt/airwaves/scripts"

echo "==> Airwaves OS updater bootstrap"

if [ "$(id -u)" -ne 0 ]; then
    echo "Please run as root (e.g. with sudo)." >&2
    exit 1
fi

echo "==> Installing host updater script"
install -d "${SCRIPTS_DIR}"
curl -fsSL "${EXT}/scripts/airwaves-update" -o "${SCRIPTS_DIR}/airwaves-update"
chmod +x "${SCRIPTS_DIR}/airwaves-update"

echo "==> Installing systemd unit"
curl -fsSL "${EXT}/config/templates/systemd-airwaves-update.service" \
    -o /etc/systemd/system/airwaves-update.service
systemctl daemon-reload

echo "==> Seeding updater state"
install -d "${AIRWAVES_DIR}/update"
if [ ! -f "${AIRWAVES_DIR}/.versions.json" ]; then
    echo '{"compose": 0, "catalog": 0, "channel": "stable"}' > "${AIRWAVES_DIR}/.versions.json"
fi

echo "==> Refreshing docker-compose.yml to latest"
cp -f "${AIRWAVES_DIR}/docker-compose.yml" "${AIRWAVES_DIR}/docker-compose.yml.pre-bootstrap" 2>/dev/null || true
curl -fsSL "${EXT}/config/templates/docker-compose.yml.template" \
    -o "${AIRWAVES_DIR}/docker-compose.yml"

echo "==> Pulling images and recreating the stack"
cd "${AIRWAVES_DIR}"
docker compose pull
docker compose up -d --remove-orphans

echo "==> Done. Future updates are available in the control app under Settings → System Update."
