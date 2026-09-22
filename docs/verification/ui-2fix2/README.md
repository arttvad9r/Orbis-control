# t_95d506c5 — D-AUD-G02 fix: evidence index

Candidate commit: 99d3e969c7ebe21c51ed2357c653c264bfbb5ef3
Parent: b3fff38c226a1672ece490dd5c32b2a1c4a48e86 (planner docs-only planning-log commit;
excluded from this candidate per the planner note — this commit's own diff has 0 planning-log lines)

Diff: crates/orbis-ui/examples/ui_snapshot.rs (+42), ui/audited/sections/performance.slint (9→+5/-4),
ui/audited/sections/cooling.slint (10, 5 lines changed). F1/F3/F4a/F4b files untouched.

## check: slint-conditional-pair
performance.slint:132-164 — `visible: root.ui-state.power-limits-ready` replaced by
`if (root.ui-state.power-limits-ready) : SectionCard {…}` and
`if (!root.ui-state.power-limits-ready) : SectionCard {…}`.
evidence: target/ui-2fix2/checks_t95d506c5.txt, target/ui-2fix2/body_t95d506c5.json
(source diff: `git diff b3fff38 99d3e969 -- ui/audited/sections/performance.slint`)

## check: harness-nonready-state
ui_snapshot.rs:158-203 — `clear_power_limits` helper + `limits-notready`/`limits-loading` scenarios.
evidence: target/ui-2fix2/before/perf-notready-980x680-dark.png (rendered artifact)

## check: band-gone-offscreen
probe: target/ui-2fix2/probe-before.txt vs target/ui-2fix2/probe-after.txt
  interior x=230..959 ink 0.000% (pre) -> 31.037% (post)
evidence: target/ui-2fix2/before/perf-notready-980x680-dark.png,
          target/ui-2fix2/after/perf-notready-980x680-dark.png

## check: band-gone-live
live run of target/release/orbis-control at 980x680 (scale 1.0, window at 470,177),
page «Производительность», real startup (log: "power-limit refresh failed; observed snapshot is stale").
probe: target/ui-2fix2/probe-gap-live-before-after.txt
  gap between profile card and limits card 173px (pre) -> 15px (post)
  interior ink 0.000% (pre) -> 31.582% (post)
evidence: /home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots/c5ca-performance-reselect.png (pre),
          target/ui-2fix2/live/live-performance-start-980x680.png (post),
          target/ui-2fix2/live/probe-live-after.txt,
          target/ui-2fix2/live/full-start.png (full 1920x1080 desktop capture)

## check: regression-perf
Ready state: six LimitRow rows inside the card, card x=[258,951] => 28px gutter both sides.
evidence: target/ui-2fix2/after/perf-ready-980x680-dark.png,
          target/ui-2fix2/after/perf-ready-980x680-light.png,
          target/ui-2fix2/after/perf-ready-1200x800-dark.png,
          target/ui-2fix2/after/perf-ready-1200x800-light.png,
          target/ui-2fix2/probe-edges.txt, target/ui-2fix2/probe-cardbody.txt,
          target/ui-2fix2/probe-limitrows.txt, target/ui-2fix2/probe-rows.txt
F1/F3/F4b vs the 84-shot audit baseline, same probe script:
  F1 dashboard card left-inset 14px -> 28px, match=True (target/ui-2fix2/probe-f134b.txt vs
     target/ui-2fix2/probe-f134b-baseline.txt)
  F3 about deepest ink x=963 -> 951
  F4b nav icon column spread 66px/9 distinct -> 15px/5 distinct
evidence: target/ui-2fix2/probe-f134b.txt, target/ui-2fix2/probe-f134b-baseline.txt,
          target/ui-2fix2/regress/dashboard-980x680-dark.png,
          target/ui-2fix2/regress/about-980x680-dark.png,
          target/ui-2fix2/regress/settings-980x680-dark.png,
          target/ui-2fix2/regress/cooling-1200x800-dark.png

## check: tests
scripts/verify task -> "verification passed", VERIFY_TASK_EXIT=0
evidence: target/ui-2fix2/verify-task.log
cargo test --workspace --locked -> CARGO_TEST_EXIT=0
evidence: target/ui-2fix2/cargo-test.log

## Not verified / honest limitations
- cooling.slint `selection-change-pending` (Остаться/Отбросить) branch could NOT be driven live:
  this machine refuses fan-curve writes ("Кривая недоступна" / fan_curve_error), so the pending
  state never latches in the live app. The change is verified as markup (conditional pair, same
  class as the audited fix) plus the pending=false render showing a clean right-aligned action row
  (target/ui-2fix2/live/live-cooling-pending-980x680.png). The live pending interaction itself is
  NOT verified.
- b3fff38 (planner's docs-only commit) is an ancestor of the candidate but is not part of this
  card's change; the candidate diff is b3fff38..99d3e969.
