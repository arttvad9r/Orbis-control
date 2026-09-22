#!/bin/sh
# D-AUD-G03 verification battery — logs every probe against the committed candidate.
set -e
cd /home/artt/Orbis-control-implementation/.worktrees/t_28783b7a
LOG=.scratch-g03/verification-log.txt
S=/home/artt/.hermes/profiles/ui/cache/scratch/g03qa
F=.scratch-g03/fixed
: > $LOG
{
  echo "D-AUD-G03 verification — $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "candidate: $(git rev-parse HEAD) (branch $(git branch --show-current), pushed to origin)"
  echo "change: ui/audited/sections/about.slint — removed 'width: parent.width;' from both"
  echo "        card paragraphs (audit prescription; layout already stretches to padded width)"
  echo
  echo "== 1. post-fix card-region probe (PASS required): ink right of card border, y>=30 =="
  for t in dark light; do
    python3 .scratch-g03/probe_card_region.py $F/about-980x680-$t.png 951 "fixed-980-$t" 30
    python3 .scratch-g03/probe_card_region.py $F/about-1200x800-$t.png 1171 "fixed-1200-$t" 30
  done
  echo
  echo "== 2. baseline probe (FAIL expected — documents the defect on QA archive 2c3e3af) =="
  for t in dark light; do
    python3 .scratch-g03/probe_card_region.py $S/about-980x680-$t.png 951 "baseline-980-$t" 30 || true
    python3 .scratch-g03/probe_card_region.py $S/about-1200x800-$t.png 1171 "baseline-1200-$t" 30 || true
  done
  echo
  echo "== 3. diff scope baseline->fixed (must be text-only, no card-geometry bands) =="
  for t in dark light; do
    for sz in 980x680 1200x800; do
      printf '%s %s: ' "$sz" "$t"
      python3 .scratch-g03/image_diff.py $S/about-$sz-$t.png $F/about-$sz-$t.png | tr '\n' ' '
      echo
    done
  done
  echo
  echo "== 4. cargo check -p orbis-ui (default features) =="
  cargo check -p orbis-ui 2>&1 | tail -1
  echo
  echo "== 5. cargo test -p orbis-ui --features ui-review --lib =="
  cargo test -p orbis-ui --features ui-review --lib 2>&1 | grep 'test result'
} 2>&1 | tee -a $LOG
echo "log written: $LOG"
