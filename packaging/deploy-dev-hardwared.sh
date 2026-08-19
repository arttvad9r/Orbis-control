#!/usr/bin/env bash
# deploy-dev-hardwared — standalone Orbis Hardware1 deployment.
#
# Обновляет ТОЛЬКО бинарник и regular-file systemd unit.
# D-Bus policy и polkit actions регистрируются через NixOS один раз
# (nixosModules.orbis-hardwared-policies), НЕ через этот скрипт.
#
# Если /etc/systemd/system/orbis-hardwared.service является symlink (обычный
# NixOS-owned unit), скрипт отказывается его перезаписывать. Для standalone
# lifecycle сначала отключите full services.orbis-control и используйте только
# policy-only module.
#
# Идемпотентен: повторный запуск обновляет binary + unit, которые ранее создал
# именно standalone deploy.
#
# Использование:
#   sudo bash packaging/deploy-dev-hardwared.sh [--stop]
#
# --stop  остановить standalone service перед обновлением.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

STABLE_BIN="/usr/local/bin"
SYSTEMD_DIR="/etc/systemd/system"
SERVICE_NAME="orbis-hardwared"
SERVICE_FILE="${SYSTEMD_DIR}/${SERVICE_NAME}.service"
GC_ROOT="/nix/var/nix/gcroots/orbis-hardwared"
POLKIT_POLICY="/etc/polkit-1/actions/io.github.orbiscontrol.hardware.policy"

if [[ $EUID -ne 0 ]]; then
  echo "ERROR: запустите через sudo: sudo bash $0 $*"
  exit 1
fi

if [[ -L "$SERVICE_FILE" ]]; then
  echo "ERROR: standalone deploy отказан: $SERVICE_FILE — symlink → $(readlink "$SERVICE_FILE")" >&2
  echo "       Это похоже на NixOS-owned unit. Не перезаписывайте его dev-script'ом." >&2
  echo "       Отключите full services.orbis-control и включите только nixosModules.orbis-hardwared-policies." >&2
  exit 2
fi

if [[ ! -r "$POLKIT_POLICY" ]]; then
  echo "ERROR: не найден зарегистрированный Hardware1 polkit policy: $POLKIT_POLICY" >&2
  echo "       Сначала включите nixosModules.orbis-hardwared-policies." >&2
  exit 2
fi

if [[ "${1:-}" == "--stop" ]]; then
  echo "→ Останавливаем ${SERVICE_NAME}…"
  systemctl stop "${SERVICE_NAME}.service" 2>/dev/null || true
  echo "✓ Service остановлен"
fi

# ─── BUILD ─────────────────────────────────────────────────────────
echo "→ [BUILD] Собираем orbis-hardwared через nix build…"
cd "$PROJECT_ROOT"

BUILD_OUTPUT_FILE="$(mktemp /tmp/orbis-build-path.XXXXXX)"
trap 'rm -f "$BUILD_OUTPUT_FILE"' EXIT

if ! nix build .#orbis-hardwared --max-jobs 1 --cores 4 --no-link --print-out-paths \
    > "$BUILD_OUTPUT_FILE"; then
  echo "ERROR: nix build завершился с ненулевым exit code"
  exit 1
fi

mapfile -t BUILD_PATHS < <(sed '/^[[:space:]]*$/d' "$BUILD_OUTPUT_FILE")
if (( ${#BUILD_PATHS[@]} != 1 )); then
  echo "ERROR: nix build --print-out-paths вернул ${#BUILD_PATHS[@]} путей (ожидается 1)"
  if (( ${#BUILD_PATHS[@]} > 0 )); then
    printf '  %s\n' "${BUILD_PATHS[@]}"
  fi
  exit 1
fi

BUILD_PATH="${BUILD_PATHS[0]}"
if [[ ! -d "$BUILD_PATH" ]]; then
  echo "ERROR: output path не является существующей директорией: $BUILD_PATH"
  exit 1
fi
echo "✓ [BUILD] Пакет: $BUILD_PATH"

# ─── DIRECTORY PREP ─────────────────────────────────────────────────
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

# ─── INSTALL BINARY ─────────────────────────────────────────────────
echo "→ [INSTALL] Устанавливаем бинарник…"
install -m 0755 "$BUILD_PATH/bin/orbis-hardwared" "${STABLE_BIN}/orbis-hardwared"
echo "✓ ${STABLE_BIN}/orbis-hardwared"

# ─── SYSTEMD ────────────────────────────────────────────────────────
echo "→ [SYSTEMD] Устанавливаем standalone systemd unit…"
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

READ_WRITE_PATHS="$(systemctl show "${SERVICE_NAME}.service" -p ReadWritePaths --value)"
PERFORMANCE_PATH="/sys/firmware/acpi/platform_profile"
KEYBOARD_PATH="/sys/class/leds/asus::kbd_backlight/brightness"

if grep -Fq "$PERFORMANCE_PATH" <<<"$READ_WRITE_PATHS"; then
  echo "✓ Sandbox write path: $PERFORMANCE_PATH"
else
  echo "✗ Sandbox missing expected write path: $PERFORMANCE_PATH" >&2
  exit 1
fi

if grep -Fq "$KEYBOARD_PATH" <<<"$READ_WRITE_PATHS"; then
  echo "✗ Sandbox unexpectedly exposes blocked keyboard write path: $KEYBOARD_PATH" >&2
  exit 1
else
  echo "✓ Blocked keyboard write path absent"
fi

echo ""
echo "✓ Deploy завершён."
echo "  Бинарник: ${STABLE_BIN}/orbis-hardwared"
echo "  GC root:  ${GC_ROOT}"
echo "  Unit:     ${SERVICE_FILE}"
echo ""
echo "  D-Bus/polkit registration: через NixOS (orbis-hardwared-policies)"
