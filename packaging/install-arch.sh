#!/usr/bin/env bash
# Install the current Orbis Control checkout on Arch Linux without Nix.
#
# This is a developer/local installer, not a pacman-owned package. It builds the
# workspace with Cargo, installs package-owned binaries, installs the
# privileged service contract, and enables the session/system services.
#
# Usage:
#   bash packaging/install-arch.sh
#   bash packaging/install-arch.sh --uninstall
#   DESTDIR=/tmp/orbis-root bash packaging/install-arch.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DESTDIR="${DESTDIR:-}"
BIN_DIR="${DESTDIR}/usr/local/bin"
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
LOCAL_STATE_DIR="${DESTDIR}/usr/local/share/orbis-control"
LOCAL_MARKER="$LOCAL_STATE_DIR/local-install"

if [[ -n "$DESTDIR" ]]; then
  # Staging install: write into DESTDIR without touching the running system.
  ROOT_CMD=()
else
  ROOT_CMD=(pkexec)
fi

if [[ "${1:-}" == "--uninstall" ]]; then
  if [[ -n "$DESTDIR" ]]; then
    echo "ERROR: --uninstall removes a live install and does not support DESTDIR staging" >&2
    exit 1
  fi
  for tool in pkexec systemctl; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "ERROR: required tool not found: $tool" >&2
      exit 1
    fi
  done
  if ! "${ROOT_CMD[@]}" test -f "$LOCAL_MARKER"; then
    echo "ERROR: local Orbis install marker not found; refusing to remove shared assets" >&2
    exit 1
  fi
  "${ROOT_CMD[@]}" systemctl disable --now orbis-hardwared.service 2>/dev/null || true
  systemctl --user disable --now orbis-sessiond.service 2>/dev/null || true
  "${ROOT_CMD[@]}" rm -f \
    "$BIN_DIR/orbis-control" "$BIN_DIR/orbisctl" \
    "$BIN_DIR/orbis-sessiond" "$BIN_DIR/orbis-hardwared" \
    "$SYSTEM_UNIT_DIR/orbis-hardwared.service" \
    "$DBUS_DIR/io.github.orbiscontrol.Hardware.conf" \
    "$POLKIT_DIR/io.github.orbiscontrol.hardware.policy" \
    "$APPLICATIONS_DIR/io.github.orbiscontrol.Orbis.desktop" \
    "$METAINFO_DIR/io.github.orbiscontrol.Orbis.metainfo.xml" \
    "$ICON_DIR/io.github.orbiscontrol.Orbis.svg"
  rm -f "$USER_UNIT_DIR/orbis-sessiond.service"
  "${ROOT_CMD[@]}" systemctl daemon-reload
  systemctl --user daemon-reload
  "${ROOT_CMD[@]}" rm -f "$LOCAL_MARKER"
  echo "✓ Local Orbis Control installation removed"
  exit 0
fi

for tool in cargo install; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "ERROR: required tool not found: $tool" >&2
    exit 1
  fi
done
if [[ -z "$DESTDIR" ]]; then
  for tool in pkexec systemctl; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "ERROR: required tool not found: $tool" >&2
      exit 1
    fi
  done
fi

cd "$PROJECT_ROOT"
echo "→ Building release workspace with Cargo…"
cargo build --workspace --release --locked --bins

UNIT_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$UNIT_TMPDIR"' EXIT
sed "s#/usr/bin/#${BIN_DIR}/#g" \
  packaging/orbis-hardwared.service >"$UNIT_TMPDIR/orbis-hardwared.service"
sed "s#/usr/bin/#${BIN_DIR}/#g" \
  data/systemd/user/orbis-sessiond.service >"$UNIT_TMPDIR/orbis-sessiond.service"

for binary in orbis-control orbisctl orbis-sessiond orbis-hardwared; do
  if [[ ! -x "target/release/$binary" ]]; then
    echo "ERROR: expected binary was not built: target/release/$binary" >&2
    exit 1
  fi
done

echo "→ Installing binaries and integration assets…"
"${ROOT_CMD[@]}" install -d -m 0755 \
  "$BIN_DIR" \
  "$SYSTEM_UNIT_DIR" \
  "$DBUS_DIR" \
  "$POLKIT_DIR" \
  "$APPLICATIONS_DIR" \
  "$METAINFO_DIR" \
  "$ICON_DIR" \
  "$LOCAL_STATE_DIR"
install -d -m 0755 "$USER_UNIT_DIR"

"${ROOT_CMD[@]}" install -m 0755 \
  "$PROJECT_ROOT/target/release/orbis-control" "$BIN_DIR/orbis-control"
"${ROOT_CMD[@]}" install -m 0755 \
  "$PROJECT_ROOT/target/release/orbisctl" "$BIN_DIR/orbisctl"
"${ROOT_CMD[@]}" install -m 0755 \
  "$PROJECT_ROOT/target/release/orbis-sessiond" "$BIN_DIR/orbis-sessiond"
"${ROOT_CMD[@]}" install -m 0755 \
  "$PROJECT_ROOT/target/release/orbis-hardwared" "$BIN_DIR/orbis-hardwared"

"${ROOT_CMD[@]}" install -m 0644 \
  "$UNIT_TMPDIR/orbis-hardwared.service" \
  "$SYSTEM_UNIT_DIR/orbis-hardwared.service"
install -m 0644 \
  "$UNIT_TMPDIR/orbis-sessiond.service" \
  "$USER_UNIT_DIR/orbis-sessiond.service"
"${ROOT_CMD[@]}" install -m 0644 \
  "$PROJECT_ROOT/data/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf" \
  "$DBUS_DIR/io.github.orbiscontrol.Hardware.conf"
"${ROOT_CMD[@]}" install -m 0644 \
  "$PROJECT_ROOT/data/polkit-1/actions/io.github.orbiscontrol.hardware.policy" \
  "$POLKIT_DIR/io.github.orbiscontrol.hardware.policy"
"${ROOT_CMD[@]}" install -m 0644 \
  "$PROJECT_ROOT/data/applications/io.github.orbiscontrol.Orbis.desktop" \
  "$APPLICATIONS_DIR/io.github.orbiscontrol.Orbis.desktop"
"${ROOT_CMD[@]}" install -m 0644 \
  "$PROJECT_ROOT/data/metainfo/io.github.orbiscontrol.Orbis.metainfo.xml" \
  "$METAINFO_DIR/io.github.orbiscontrol.Orbis.metainfo.xml"
"${ROOT_CMD[@]}" install -m 0644 \
  "$PROJECT_ROOT/data/icons/hicolor/scalable/apps/io.github.orbiscontrol.Orbis.svg" \
  "$ICON_DIR/io.github.orbiscontrol.Orbis.svg"
"${ROOT_CMD[@]}" install -m 0644 /dev/null "$LOCAL_MARKER"

if [[ -n "$DESTDIR" ]]; then
  echo "✓ Staging install completed; service activation skipped (DESTDIR=$DESTDIR)."
  exit 0
fi

# System helper: available immediately after installation.
"${ROOT_CMD[@]}" systemctl daemon-reload
"${ROOT_CMD[@]}" systemctl reload dbus.service
"${ROOT_CMD[@]}" systemctl enable --now orbis-hardwared.service
"${ROOT_CMD[@]}" systemctl restart orbis-hardwared.service

# Session daemon: per-user unit owned by the invoking user.
systemctl --user daemon-reload
systemctl --user enable --now orbis-sessiond.service
systemctl --user restart orbis-sessiond.service

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
"${ROOT_CMD[@]}" systemctl --no-pager --full status orbis-hardwared.service || true
systemctl --user --no-pager --full status orbis-sessiond.service || true

echo ""
echo "✓ Orbis Control installed from the current checkout without Nix."
