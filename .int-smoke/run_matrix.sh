#!/usr/bin/env bash
# Render the smoke matrix for one ui_snapshot binary.
# Usage: run_matrix.sh <path-to-ui_snapshot-binary> <output-dir>
set -u
BIN="$1"
OUT="$2"
mkdir -p "$OUT"
SECTIONS="dashboard performance power cooling graphics backlight display system settings about"
fail=0
for sec in $SECTIONS; do
  for geo in 980x680 1200x800; do
    w=${geo%%x*}; h=${geo##*x}
    "$BIN" "$sec" "$OUT/$sec-$geo-dark.png" dark "$w" "$h" normal || fail=1
  done
done
for sec in cooling performance about; do
  "$BIN" "$sec" "$OUT/$sec-980x680-light.png" light 980 680 normal || fail=1
done
# Functional-state scenario probes (merged runtime honesty):
"$BIN" cooling "$OUT/cooling-980x680-dark-error.png" dark 980 680 error || fail=1
"$BIN" graphics "$OUT/graphics-980x680-dark-pending.png" dark 980 680 pending || fail=1
"$BIN" performance "$OUT/performance-980x680-dark-unsupported.png" dark 980 680 unsupported || fail=1
echo "MATRIX_FAIL=$fail"
exit $fail
