#!/bin/sh
# Render the six About variants (2 themes x 2 sizes) with the fixed layout.
set -e
cd /home/artt/Orbis-control-implementation/.worktrees/t_28783b7a
mkdir -p .scratch-g03/fixed
for t in dark light; do
  for sz in 980x680 1200x800; do
    w=${sz%%x*}
    h=${sz##*x}
    cargo run -q -p orbis-ui --example ui_snapshot --features ui-review -- \
      about ".scratch-g03/fixed/about-$sz-$t.png" "$t" "$w" "$h" normal
  done
done
ls -la .scratch-g03/fixed/
