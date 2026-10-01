#!/usr/bin/env bash
# Render the README screenshots: every window in light and dark theme at 2x,
# plus one composed overview per theme (needs ImageMagick 7 `magick`).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OUT=screenshots
SCALE="${ORBIS_SNAPSHOT_SCALE:-2}"
mkdir -p "$OUT"

cargo build --quiet -p orbis-ui --example ui_snapshot
for view in main fans extra; do
  for theme in light dark; do
    ORBIS_SNAPSHOT_SCALE="$SCALE" target/debug/examples/ui_snapshot \
      "$view" "$OUT/$view-$theme.png" "$theme"
  done
done

if ! command -v magick >/dev/null 2>&1; then
  echo "✓ Window screenshots refreshed (magick not found: overview skipped)"
  exit 0
fi

# Window frame: a title bar like the KDE Clay decoration, rounded corners and
# a soft shadow; the windows sit in a row like on screen (Extra, Fans, main).
TITLE_FONT=$(fc-match -f '%{file}' 'Noto Sans:bold')

frame() { # <in> <out> <title> <theme>
  local in=$1 out=$2 title=$3 theme=$4 bar fg w
  if [[ $theme == light ]]; then bar='#E8E6DC'; fg='#141413'; else bar='#30302E'; fg='#F0EEE6'; fi
  w=$(magick identify -format '%w' "$in")
  # Title bar 64 px (32 at 1x): title on the left; minimise, maximise and
  # close drawn as plain strokes on the right.
  local x=$((w - 40))
  magick \( -size "${w}x64" "xc:$bar" -fill "$fg" -font "$TITLE_FONT" -pointsize 24 \
            -gravity west -annotate +28+0 "$title" -gravity northwest \
            -stroke "$fg" -strokewidth 3 -fill none \
            -draw "line $((x - 10)),22 $((x + 10)),42 line $((x + 10)),22 $((x - 10)),42" \
            -draw "rectangle $((x - 68)),23 $((x - 50)),41" \
            -draw "line $((x - 128)),32 $((x - 108)),32" \) \
         "$in" -append "$out"
  local fw fh
  read -r fw fh <<<"$(magick identify -format '%w %h' "$out")"
  # Rounded corners (12 px at 1x), then a soft drop shadow.
  magick "$out" \( -size "${fw}x${fh}" xc:none -fill white \
            -draw "roundrectangle 0,0 $((fw - 1)),$((fh - 1)) 24,24" \) \
         -compose DstIn -composite "$out"
  magick "$out" \( +clone -background '#00000055' -shadow 60x24+0+16 \) +swap \
         -background none -compose Over -layers merge +repage "$out"
}

for theme in light dark; do
  tmp=$(mktemp -d)
  frame "$OUT/extra-$theme.png" "$tmp/extra.png" 'Дополнительно' "$theme"
  frame "$OUT/fans-$theme.png" "$tmp/fans.png" 'Вентиляторы и мощность' "$theme"
  frame "$OUT/main-$theme.png" "$tmp/main.png" 'Orbis Control' "$theme"
  if [[ $theme == light ]]; then from='#F7F5EF'; to='#E3DED0'; else from='#2B2A28'; to='#171615'; fi
  magick "$tmp/extra.png" "$tmp/fans.png" "$tmp/main.png" -background none -gravity south \
         +smush 8 "$tmp/row.png"
  magick "$tmp/row.png" -bordercolor none -border 56x48 "$tmp/row.png"
  read -r rw rh <<<"$(magick identify -format '%w %h' "$tmp/row.png")"
  magick -size "${rw}x${rh}" "gradient:$from-$to" "$tmp/row.png" -compose Over -composite \
         -resize 50% -strip "$OUT/overview-$theme.png"
  rm -rf "$tmp"
done

echo "✓ Refreshed Orbis UI screenshots in $OUT/"
