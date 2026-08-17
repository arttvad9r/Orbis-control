#!/usr/bin/env bash
# deploy-dev-hardwared — standalone Orbis Hardware1 deployment.
#
# Использует НАРЯДНЫЙ пакет orbis-hardwared (без GUI/Slint deps).
# Сборка занимает ~1-2 минуты вместо ~20 минут полного orbis-control.
#
# Идемпотентен: повторный запуск обновляет binary + policies + unit.
#
# Использование:
#   sudo bash packaging/deploy-dev-hardwared.sh [--stop]
#
# --stop  остановить service перед обновлением (для аккуратного upgrade).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ─── Пути ───────────────────────────────────────────────────────────
STABLE_BIN="/usr/local/bin"
DBUS_CONF="/etc/dbus-1/system.d"
POLKIT_DIR="/etc/polkit-1/actions"
SYSTEMD_DIR="/etc/systemd/system"
SERVICE_NAME="orbis-hardwared"
SERVICE_FILE="${SYSTEMD_DIR}/${SERVICE_NAME}.service"
GC_ROOT="/nix/var/nix/gcroots/orbis-hardwared"

# ─── Preconditions ──────────────────────────────────────────────────
if [[ $EUID -ne 0 ]]; then
  echo "ERROR: запустите через sudo: sudo bash $0 $*"
  exit 1
fi

if [[ "${1:-}" == "--stop" ]]; then
  echo "→ Останавливаем ${SERVICE_NAME}…"
  systemctl stop "${SERVICE_NAME}.service" 2>/dev/null || true
  echo "✓ Service остановлен"
fi

# ─── BUILD ─────────────────────────────────────────────────────────
# Собираем НАРЯДНЫЙ пакет orbis-hardwared (только daemon, без GUI/Slint).
# Build time: ~1-2 min (clean) / ~10-20s (incremental).
echo "→ [BUILD] Собираем orbis-hardwared через nix build…"
cd "$PROJECT_ROOT"
nix build .#orbis-hardwared --max-jobs 1 --cores 4 --print-out-paths \
  > /tmp/orbis-build-path.txt 2>&1

BUILD_PATH="$(cat /tmp/orbis-build-path.txt)"
if [[ ! -d "$BUILD_PATH" ]]; then
  echo "ERROR: nix build завершился ошибкой"
  cat /tmp/orbis-build-path.txt
  exit 1
fi
echo "✓ [BUILD] Пакет: $BUILD_PATH"

# ─── INSTALL ────────────────────────────────────────────────────────
echo "→ [INSTALL] Создаём persistent GC root…"
nix-store --add-root "$GC_ROOT" -r "$BUILD_PATH" >/dev/null 2>&1
if [[ -L "$GC_ROOT" ]]; then
  echo "✓ GC root: $GC_ROOT → $(readlink "$GC_ROOT")"
else
  echo "✓ GC root создан: $GC_ROOT"
fi

echo "→ [INSTALL] Устанавливаем бинарник…"
cp -f "$BUILD_PATH/bin/orbis-hardwared" "${STABLE_BIN}/orbis-hardwared"
chmod 755 "${STABLE_BIN}/orbis-hardwared"
echo "✓ ${STABLE_BIN}/orbis-hardwared"

# ─── DBUS ───────────────────────────────────────────────────────────
echo "→ [DBUS] Устанавливаем D-Bus policy…"
cp -f "$BUILD_PATH/share/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf" \
      "${DBUS_CONF}/io.github.orbiscontrol.Hardware.conf"
chmod 644 "${DBUS_CONF}/io.github.orbiscontrol.Hardware.conf"
echo "✓ D-Bus policy: ${DBUS_CONF}/io.github.orbiscontrol.Hardware.conf"

# ─── POLKIT ─────────────────────────────────────────────────────────
echo "→ [POLKIT] Устанавливаем polkit actions…"
cp -f "$BUILD_PATH/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy" \
      "${POLKIT_DIR}/io.github.orbiscontrol.hardware.policy"
chmod 644 "${POLKIT_DIR}/io.github.orbiscontrol.hardware.policy"
echo "✓ Polkit: ${POLKIT_DIR}/io.github.orbiscontrol.hardware.policy"

# ─── SYSTEMD ────────────────────────────────────────────────────────
echo "→ [SYSTEMD] Устанавливаем systemd unit…"
cat > "${SERVICE_FILE}" << 'UNIT'
[Unit]
Description=Orbis Control hardware helper (standalone dev deployment)
X-StopOnRemoval=false
After=dbus.service
Requires=dbus.service

[Service]
Type=dbus
BusName=io.github.orbiscontrol.Hardware
ExecStart=/usr/local/bin/orbis-hardwared
Restart=on-failure
RestartSec=2s
X-RestartIfChanged=false

# Sandbox (threat-model §3.3)
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_UNIX
MemoryDenyWriteExecute=true
AmbientCapabilities=
CapabilityBoundingSet=

# /sys read-only, на write ТОЛЬКО platform_profile
ReadOnlyPaths=/sys
ReadWritePaths=-/sys/firmware/acpi/platform_profile
UNIT

chmod 644 "${SERVICE_FILE}"
echo "✓ Unit: ${SERVICE_FILE}"

# ─── START ──────────────────────────────────────────────────────────
echo "→ [START] daemon-reload + enable + restart…"
systemctl daemon-reload
systemctl enable "${SERVICE_NAME}.service"
systemctl restart "${SERVICE_NAME}.service"

# ─── Проверка ───────────────────────────────────────────────────────
echo ""
echo "=== Проверка ==="
systemctl is-active "${SERVICE_NAME}.service" && echo "✓ Service active" || echo "✗ Service NOT active"
busctl list 2>/dev/null | grep -q "io.github.orbiscontrol.Hardware" && \
  echo "✓ D-Bus name registered" || echo "✗ D-Bus name NOT found"
echo ""
echo "✓ Deploy завершён."
echo "  Бинарник: ${STABLE_BIN}/orbis-hardwared"
echo "  GC root:  ${GC_ROOT}"
echo "  Unit:     ${SERVICE_FILE}"
