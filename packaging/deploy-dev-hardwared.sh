#!/usr/bin/env bash
# deploy-dev-hardwared — standalone Arch/Linux development deployment.
#
# Builds orbis-hardwared with Cargo, installs the binary plus its systemd,
# D-Bus and polkit assets, then restarts the service. No Nix/NixOS tooling is
# required.
#
# Usage:
#   bash packaging/deploy-dev-hardwared.sh [--stop]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_DIR="/usr/local/bin"
SYSTEMD_DIR="/etc/systemd/system"
DBUS_DIR="/etc/dbus-1/system.d"
POLKIT_DIR="/usr/share/polkit-1/actions"
SERVICE_NAME="orbis-hardwared"
SERVICE_FILE="${SYSTEMD_DIR}/${SERVICE_NAME}.service"
DBUS_POLICY_SOURCE="${PROJECT_ROOT}/data/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf"
POLKIT_POLICY_SOURCE="${PROJECT_ROOT}/data/polkit-1/actions/io.github.orbiscontrol.hardware.policy"

if ! command -v cargo >/dev/null 2>&1; then
  echo "ERROR: cargo not found. Install rustup/Rust first." >&2
  exit 1
fi
if ! command -v sudo >/dev/null 2>&1; then
  echo "ERROR: sudo is required for system installation." >&2
  exit 1
fi

if [[ "${1:-}" == "--stop" ]]; then
  echo "→ Stopping ${SERVICE_NAME}…"
  sudo systemctl stop "${SERVICE_NAME}.service" 2>/dev/null || true
fi

# Build as the invoking user, not as root.
echo "→ Building orbis-hardwared with Cargo…"
cd "$PROJECT_ROOT"
cargo build --release --locked -p orbis-hardwared --bin orbis-hardwared

BINARY="${PROJECT_ROOT}/target/release/orbis-hardwared"
[[ -x "$BINARY" ]] || { echo "ERROR: built binary not found: $BINARY" >&2; exit 1; }

# Install the complete privileged-service contract together so a fresh Arch
# checkout does not depend on distro-specific packaging.
echo "→ Installing binary and service assets…"
sudo install -d -m 0755 "$BIN_DIR" "$SYSTEMD_DIR" "$DBUS_DIR" "$POLKIT_DIR"
sudo install -m 0755 "$BINARY" "${BIN_DIR}/orbis-hardwared"
sudo install -m 0644 "${SCRIPT_DIR}/orbis-hardwared.service" "$SERVICE_FILE"
sudo install -m 0644 "$DBUS_POLICY_SOURCE" "${DBUS_DIR}/io.github.orbiscontrol.Hardware.conf"
sudo install -m 0644 "$POLKIT_POLICY_SOURCE" "${POLKIT_DIR}/io.github.orbiscontrol.hardware.policy"

sudo systemctl daemon-reload
sudo systemctl enable "${SERVICE_NAME}.service"
sudo systemctl restart "${SERVICE_NAME}.service"

echo ""
echo "=== Verification ==="
sudo systemctl is-active --quiet "${SERVICE_NAME}.service"
echo "✓ Service active"

if busctl list 2>/dev/null | grep -q "io.github.orbiscontrol.Hardware"; then
  echo "✓ D-Bus name registered"
else
  echo "ERROR: D-Bus name io.github.orbiscontrol.Hardware not found" >&2
  exit 1
fi

READ_WRITE_PATHS="$(systemctl show "${SERVICE_NAME}.service" -p ReadWritePaths --value)"
for expected in \
  "/sys/firmware/acpi/platform_profile" \
  "/sys/class/leds/asus::kbd_backlight/brightness" \
  "/sys/class/leds/asus::kbd_backlight/max_brightness"; do
  if grep -Fq "$expected" <<<"$READ_WRITE_PATHS"; then
    echo "✓ Sandbox write path: $expected"
  else
    echo "ERROR: sandbox missing expected write path: $expected" >&2
    exit 1
  fi
done

echo ""
echo "✓ orbis-hardwared deployed without Nix."
echo "  Binary: $BIN_DIR/orbis-hardwared"
echo "  Unit:   $SERVICE_FILE"
echo "  D-Bus:  $DBUS_DIR/io.github.orbiscontrol.Hardware.conf"
echo "  Polkit: $POLKIT_DIR/io.github.orbiscontrol.hardware.policy"
