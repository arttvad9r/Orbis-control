#!/usr/bin/env bash
# deploy-dev-hardwared — standalone Orbis Hardware1 deployment.
#
# Обновляет ТОЛЬКО бинарник и systemd unit.
# D-Bus policy и polkit actions регистрируются через NixOS один раз
# (nixosModules.orbis-hardwared-policies), НЕ через этот скрипт.
#
# Идемпотентен: повторный запуск обновляет binary + unit.
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

# ─── DIRECTORY PREP ──────────────────────────────────────────────────
echo "→ [DIR] Создаём целевые директории…"
install -d -m 0755 "${STABLE_BIN}"
install -d -m 0755 "${SYSTEMD_DIR}"
install -d -m 0755 "$(dirname "$GC_ROOT")"
echo "✓ Директории готовы"

# ─── GC ROOT ────────────────────────────────────────────────────────
echo "→ [GC] Создаём persistent GC root…"
nix-store --add-root "$GC_ROOT" -r "$BUILD_PATH" >/dev/null 2>&1
if [[ -L "$GC_ROOT" ]]; then
  echo "✓ GC root: $GC_ROOT → $(readlink "$GC_ROOT")"
else
  echo "✓ GC root создан: $GC_ROOT"
fi

# ─── INSTALL BINARY ────────────────────────────────────────────────
echo "→ [INSTALL] Устанавливаем бинарник…"
install -m 0755 "$BUILD_PATH/bin/orbis-hardwared" "${STABLE_BIN}/orbis-hardwared"
echo "✓ ${STABLE_BIN}/orbis-hardwared"

# ─── SYSTEMD ────────────────────────────────────────────────────────
echo "→ [SYSTEMD] Устанавливаем systemd unit…"
install -m 0644 "${SCRIPT_DIR}/orbis-hardwared.service" "${SERVICE_FILE}"
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
echo ""
echo "  D-Bus/polkit registration: через NixOS (orbis-hardwared-policies)"
