# QA verdict evidence — t_0c45ec21

candidate (card pin): a21139e8f4591237c38075d54350e5bf7afc8974
  = HEAD, code-identical to the merge commit 5f0d3da (diff a21139e8 vs 5f0d3da = docs/ only)
  = tree hash e4df44785ab02ba86bf3b372230dd0d737fc31cc
verdict set: c5ca-*.png (27 files) — post-merge, tree 5f0d3da
control set: p980-*.png (9 files) — pre-merge, tree 5ca36ab  (calibration only)

## Verdict: FAIL

Blocking defect: D-AUD-G02 (planned as fix "F2") is NOT fixed in the candidate
source and is LIVE-REPRODUCED in the verdict set.

## F2 / D-AUD-G02 — Performance page phantom ~172px band  [FAIL]

Audit definition (UI_AUDIT_BASELINE.md:26-40, fix row :106):
  the "Лимиты мощности и температуры" card uses `visible: ...power-limits-ready`;
  Slint `visible:false` hides without collapsing, so the card keeps its full
  height and the real `if (!power-limits-ready)` fallback renders BELOW the hole.
  Required fix: replace the `visible:` card + trailing `if` pair with a single
  conditional pair (`if (ready) : SectionCard` / `if (!ready) : SectionCard`).

Source at the candidate — construct still present, unfixed:
  ui/audited/sections/performance.slint:133  `visible: root.ui-state.power-limits-ready;`
  ui/audited/sections/performance.slint:154  `if (!root.ui-state.power-limits-ready) : SectionCard {`

  Same construct at every revision: 860bc07:122/143, 2c3e3af:126/147,
  f45f55d:126/147, 5ca36ab:129/150, 5f0d3da:133/154, a21139e8:133/154.
  `git log --all -S"if (root.ui-state.power-limits-ready"` -> never committed.
  The commit that CLAIMS F2 (2c3e3af) never touched this construct; its F2 change
  was `crates/orbis-ui/examples/ui_snapshot.rs` demo-data only
  (`state.power_limits_ready = true`), i.e. a harness fixture change, not the
  required UI fix.

Live reproduction in the verdict set:
  c5ca-performance-reselect.png (Performance page, sidebar row "Производительность"
  selected, fill coverage 0.72 = REF merged/performance)
    ink bands  = [(17,26),(61,80),(89,101),(120,298),(471,525)]
    gap 172px between the profile card (ends y=298) and the fallback card (y=471..525)
    ink in the band y=302..466 x=222..959 = 1.351% (only the card's left border
    column x=220..229) -> the band body is empty page background
    the y=471..525 card is 55px = padding+title only = the FALLBACK card
    (pre-fix baseline: y=469..522 = 54px, identical structure)
    post-fix reference (merged, READY): no gap; limits card y=312..627 = 316px
  Same 172px gap measured on the pre-merge control p980-performance.png and the
  pre-fix baseline render -> the live candidate reproduces the defect state, not
  the fixed state.

Why it is live and not just a fixture artifact: the app runs against the real
backend; hardwared is not running, so power-limits-ready is false in production
(this is exactly the "production startup state" the audit named). Evidence:
  logs/status-before-reselect.json -> gpu.power.state = "unavailable"
  ("org.freedesktop.DBus.Error.ServiceUnknown: The name is not activatable").
  In that state the page always shows the phantom band + fallback card.

Honesty side-effect (SC-VISUAL-STATES / SC-UNSUPPORTED): in the merged render the
declared-Unavailable scenario does NOT change the limits card at all —
  limits-card region y=312..627 x=222..959: differing px = 0 of 233208 (0.000%)
  normal vs unsupported -> identical. The harness never clears power_limits_ready
  (`apply_scenario` "unsupported" does not touch it), so the state that the audit
  says must show the fallback card cannot be produced by the harness at all.

## F1 / D-AUD-G01 — dashboard gutter  [PASS]

Source fixed at the candidate: dashboard.slint:77-78
  `width: root.width - 2 * Grid.page-gutter;` / `x: Grid.page-gutter;`
  (pre-fix 860bc07:74-75 was `root.width - 28px` / `x: 14px`).
Grid.page-gutter = 28px (common.slint). Live: c5ca-dashboard-early /
c5ca-dashboard-top-again / c5ca-perf content column x=(218,975), identical to the
candidate-tree reference merged/dashboard.

## F3 / D-AUD-G03 — About paragraph overflow  [PASS]

Source fixed at the candidate: about.slint `width: parent.width` occurrences
  860bc07: 2,  2c3e3af: 2,  f45f55d: 0,  5f0d3da: 0,  a21139e8 = HEAD: 0.
  `wrap: word-wrap` retained on both paragraphs (about.slint:64,70).
Live c5ca-about matches the candidate-tree about render.

## F4a / D-AUD-G04 — nav icon vertical centring  [PASS]

Image-derived row bands (no hard-coded offsets), icon centre vs label centre:
  LIVE c5ca-about / c5ca-power / c5ca-system: mean -1.25px, |max| 2.00px
  POST-FIX merged/dashboard (candidate tree): mean -1.10px
  PRE-FIX baseline/dashboard: -15.00px
  CONTROL p980-settings (pre-merge): -15.00px
-> fix present in the live set.

## F4b / D-AUD-G04 — constant nav icon column  [PASS]

Nav icon left edge per row, pooled:
  c5ca-* (verdict set, 27 files, 266 rows): distinct [25,26,27]  spread = 2
  merged/* (candidate tree, 10 files, 100 rows): distinct [25,26,27]  spread = 2
  p980-* (control, pre-merge, 90 rows): distinct [30,54,57,62,63,69,70,77]  spread = 47
Matches the planner's independently measured expectation (lefts≈25..27, spread=2;
control spread 47..67).

## Liveness  [PASS]

27 live captures 07:17-07:34 after the merge; every page render is decodable,
correctly sized and non-blank; sidebar selection and page headers match the
candidate-tree references for dashboard/performance/about/power/settings/system.
Rapid repetition (c5ca-after-rapid-nav.png), back-navigation
(c5ca-dashboard-top-again.png, c5ca-settings-toggled-back.png) and state
toggling (dirty/discarded/toggled) all render consistent sidebar geometry.
Note: `c5ca-perf.png` is MISLABELLED — it is the Dashboard page (row "Главная"
selected, fill 0.99); the genuine Performance capture is
c5ca-performance-reselect.png.

## Honesty / read-only discipline  [PASS]

status-before-reselect.json vs status-after-reselect.json are byte-identical once
observation timestamps are stripped (only secs/nanos differ); no hardware
mutation observed. Performance and charge-limit read back unchanged
(balanced / enabled=false, configured=effective=100).

## INCONCLUSIVE (declared, not guessed)

Mutation path (SC-POWER-APPLY, SC-BOOST, SC-FAN-*, SC-GPU-MODE writes) is NOT
covered by this evidence set: hardwared is not running, so gpu.power is
unavailable and no privileged write or read-back was exercised. This card's
verdict set contains no mutation evidence; that path stays unverified here.
