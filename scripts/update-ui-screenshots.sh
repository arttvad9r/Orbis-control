#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mkdir -p screenshots

count=0
for view in main fans extra; do
  for theme in light dark; do
    file="screenshots/$view-$theme.png"
    echo "→ $view ($theme) -> $file"
    cargo run --quiet -p orbis-ui --example ui_snapshot -- "$view" "$file" "$theme"
    count=$((count + 1))
  done
done

echo "✓ Refreshed $count Orbis UI screenshots"
