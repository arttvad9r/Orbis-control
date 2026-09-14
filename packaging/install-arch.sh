#!/usr/bin/env bash
# Install the current Orbis Control checkout on Arch Linux without Nix.
#
# This is a developer/local installer, not a pacman-owned package. It builds the
# workspace with Cargo, installs package-owned binaries under /usr/bin, installs the
# privileged service contract, and enables the session/system services.
#
# Usage:
#   bash packaging/install-arch.sh
#   DESTDIR=/tmp/orbis-root bash packaging/install-arch.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DESTDIR="${DESTDIR:-}"
BIN_DIR="${DESTDIR}/usr/bin"
SYSTEM_UNIT_DIR="${DESTDIR}/etc/systemd/system"
if [[ -n "${INSTALL_USER_UNIT_DIR:-}" ]]; then
  USER_UNIT_DIR="$INSTALL_USER_UNIT_DIR"
elif [[ -n "$DESTDIR" ]]; then
  USER_UNIT_DIR="${DESTDIR}/usr/lib/systemd/user"
else
  USER_UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
fi
DBUS_DIR="${DESTDIR}/etc/dbus-1/system.d"
POLKIT_DIR="${DESTDIR}/usr/share/polkit-1/actions"
APPLICATIONS_DIR="${DESTDIR}/usr/local/share/applications"
METAINFO_DIR="${DESTDIR}/usr/local/share/metainfo"
ICON_DIR="${DESTDIR}/usr/local/share/icons/hicolor/scalable/apps"

if [[ -n "$DESTDIR" ]]; then
  INSTALL=(install)
else
  INSTALL=(sudo install)
fi

for tool in cargo install; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "ERROR: required tool not found: $tool" >&2
    exit 1
  fi
done
if [[ -z "$DESTDIR" ]]; then
  for tool in sudo systemctl; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "ERROR: required tool not found: $tool" >&2
      exit 1
    fi
  done
fi

cd "$PROJECT_ROOT"
echo "→ Building release workspace with Cargo…"
cargo build --workspace --release --locked

for binary in orbis-control orbisctl orbis-sessiond orbis-hardwared; do
  if [[ ! -x "target/release/$binary" ]]; then
    echo "ERROR: expected binary was not built: target/release/$binary" >&2
    exit 1
  fi
done

echo "→ Installing binaries and integration assets…"
"${INSTALL[@]}" -d -m 0755 \
  "$BIN_DIR" \
  "$SYSTEM_UNIT_DIR" \
  "$DBUS_DIR" \
  "$POLKIT_DIR" \
  "$APPLICATIONS_DIR" \
  "$METAINFO_DIR" \
  "$ICON_DIR"
install -d -m 0755 "$USER_UNIT_DIR"

"${INSTALL[@]}" -m 0755 target/release/orbis-control "$BIN_DIR/orbis-control"
"${INSTALL[@]}" -m 0755 target/release/orbisctl "$BIN_DIR/orbisctl"
"${INSTALL[@]}" -m 0755 target/release/orbis-sessiond "$BIN_DIR/orbis-sessiond"
"${INSTALL[@]}" -m 0755 target/release/orbis-hardwared "$BIN_DIR/orbis-hardwared"

"${INSTALL[@]}" -m 0644 \
  packaging/orbis-hardwared.service \
  "$SYSTEM_UNIT_DIR/orbis-hardwared.service"
install -m 0644 \
  data/systemd/user/orbis-sessiond.service \
  "$USER_UNIT_DIR/orbis-sessiond.service"
"${INSTALL[@]}" -m 0644 \
  data/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf \
  "$DBUS_DIR/io.github.orbiscontrol.Hardware.conf"
"${INSTALL[@]}" -m 0644 \
  data/polkit-1/actions/io.github.orbiscontrol.hardware.policy \
  "$POLKIT_DIR/io.github.orbiscontrol.hardware.policy"
"${INSTALL[@]}" -m 0644 \
  data/applications/io.github.orbiscontrol.Orbis.desktop \
  "$APPLICATIONS_DIR/io.github.orbiscontrol.Orbis.desktop"
"${INSTALL[@]}" -m 0644 \
  data/metainfo/io.github.orbiscontrol.Orbis.metainfo.xml \
  "$METAINFO_DIR/io.github.orbiscontrol.Orbis.metainfo.xml"
"${INSTALL[@]}" -m 0644 \
  data/icons/io.github.orbiscontrol.Orbis.svg \
  "$ICON_DIR/io.github.orbiscontrol.Orbis.svg"

if [[ -n "$DESTDIR" ]]; then
  echo "✓ Staging install completed; service activation skipped (DESTDIR=$DESTDIR)."
  exit 0
fi

# System helper: available immediately after installation.
sudo systemctl daemon-reload
sudo systemctl enable --now orbis-hardwared.service

# Session daemon: per-user unit owned by the invoking user.
systemctl --user daemon-reload
systemctl --user enable --now orbis-sessiond.service

echo ""
echo "=== Installed ==="
printf '  %s\n' \
  "$BIN_DIR/orbis-control" \
  "$BIN_DIR/orbisctl" \
  "$BIN_DIR/orbis-sessiond" \
  "$BIN_DIR/orbis-hardwared" \
  "$USER_UNIT_DIR/orbis-sessiond.service"

echo ""
echo "=== Service state ==="
sudo systemctl --no-pager --full status orbis-hardwared.service || true
systemctl --user --no-pager --full status orbis-sessiond.service || true

echo ""
echo "✓ Orbis Control installed from the current checkout without Nix."
