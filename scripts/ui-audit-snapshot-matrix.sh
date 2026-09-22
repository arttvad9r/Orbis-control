#!/bin/sh
# Baseline snapshot matrix for the geometry/symmetry audit (task t_5fc90d28).
# Sections SC-VISUAL-980 names + the remaining audited pages, both window
# sizes and both themes. No source changes; screenshots only.
# Screenshots land in the scratch dir (gitignored workspace output).
set -e
cd /home/artt/Orbis-control-implementation/.worktrees/t_5fc90d28
OUT="${1:-/home/artt/.hermes/profiles/ui/cache/scratch/audit-baseline}"
mkdir -p "$OUT"

run() { # section w h theme [state]
    sec="$1"; w="$2"; h="$3"; th="$4"; st="${5:-normal}"
    name="${sec}-${w}x${h}-${th}"
    if [ "$st" != "normal" ]; then name="${name}-${st}"; fi
    cargo run -q -p orbis-ui --example ui_snapshot -- "$sec" "$OUT/$name.png" "$th" "$w" "$h" "$st" >/dev/null 2>&1
    echo "$name"
}

for sec in dashboard performance cooling graphics power backlight display system settings about dialog; do
    for wh in "980 680" "1200 800"; do
        for th in dark light; do
            run "$sec" $wh "$th"
        done
    done
done

# State variants on the four SC-VISUAL-980 pages (dark, both sizes).
for sec in dashboard performance cooling graphics; do
    for st in dirty pending error unsupported readonly; do
        run "$sec" 980 680 dark "$st"
        run "$sec" 1200 800 dark "$st"
    done
done
echo "DONE"
