#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mkdir -p screenshots

sections=(
  "dashboard:01-dashboard.png"
  "performance:02-performance.png"
  "power:03-power.png"
  "cooling:04-cooling-fan-editor.png"
  "graphics:05-graphics.png"
  "backlight:06-backlight.png"
  "display:07-display.png"
  "system:08-system-diagnostics.png"
  "settings:09-settings.png"
  "about:10-about.png"
)

for entry in "${sections[@]}"; do
  section="${entry%%:*}"
  file="${entry#*:}"
  echo "→ $section -> screenshots/$file"
  cargo run --quiet -p orbis-ui --example ui_snapshot -- "$section" "screenshots/$file" dark
done

echo "✓ Refreshed ${#sections[@]} Orbis UI screenshots"
