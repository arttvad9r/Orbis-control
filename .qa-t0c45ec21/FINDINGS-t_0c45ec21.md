# t_0c45ec21 — Orbis QA-verdict: verification-target contradiction (analysis only)

Read-only QA analysis. No candidate mutation, no re-render, no re-build, no app launch
(card scope forbids all of these; only existing artifacts were inspected).

## Card as written

- title: `Orbis QA-verdict: записать вердикт общей QA по собранным артефактам (5ca36ab)`
- `candidate.git_sha` (in body): `fb894f295222d225c387e9dd5cfeba2328290425`
- objective/scope: "38 live-скриншотов кандидата **5ca36ab** (prefix c5ca-* и 980-набор p980-*)";
  "verify на 5ca36ab уже PASS exit 0"
- required checks (all `required: true`, `verification.required: true`):
  - `liveness-verdict` — вердикт по live-скриншотам c5ca-*/p980-*
  - `honesty-verdict` — вердикт по diff-пробам (schema v3)
  - `visual-verdict` — «Скриншоты 980-набора соответствуют зафиксированной симметрии (F1-F4b PASS из UI-QA2verdict)»
- scope detail: «Геометрическая симметрия уже PASS в t_4035d84d — **не дублировать пиксельные замеры**, только сверка соответствия»

## Fact 1 — `5ca36ab` does not contain the F4a/F4b nav fix

`git show 5ca36ab:ui/components/nav-item.slint` has no absolute-x geometry. It still uses
`HorizontalLayout { padding-left: 12px; spacing: 11px; alignment: center; }` — the pre-fix
construct. The constant-column fix (`root.row-inset`, fixed 18×18 icon box, `x: root.icon-size
+ root.spacing-icon-label`) is absent at `5ca36ab`.

Presence of the fix by revision (`git show <rev>:ui/components/nav-item.slint | grep -n 'x:'`):

    5ca36ab   -> (no x: geometry at all)             PRE-fix
    5f0d3da   -> x: root.row-inset / x: 0 / ...      POST-fix (arrives with the merge)
    fb894f2   -> same as 5f0d3da                     POST-fix

`5f0d3da` is the merge `merge: integrate verified UI symmetry line (wt/ui-symmetry line tip
f45f55d) into functional line`, committer 2026-09-22 07:14:01 +0300.
`5ca36ab` committer 2026-09-22 07:03:25 +0300 (earlier, docs-only card).

## Fact 2 — the two QA screenshot sets are two DIFFERENT product trees

Nav icon-column spread measured per 980-wide frame (leftmost glyph pixel per nav row, over the
10 sidebar rows; wide selection/hover fills excluded). The fix pins every icon to one constant
column, so spread ≈ 0–3 px = aligned/post-fix, spread ≈ 44–47 px = zigzag/pre-fix.

    c5ca-*.png  (28 files)  spread = 2  distinct icon columns = 3   -> ALIGNED   (post-fix)
    p980-*.png  ( 9 files)  spread = 47 distinct icon columns = 8   -> ZIGZAG    (pre-fix)
    win-02.png              spread = 47                             -> ZIGZAG    (pre-fix live)
    win-01.png              is a Hermes board screenshot, not the product

Reference renders in-repo:

    .int-smoke/merged/*980x680*.png           spread = 2  -> ALIGNED (post-fix, 5f0d3da)
    docs/ui-audit-baseline/*980x680*.png      spread = 44 -> ZIGZAG  (pre-fix baseline)

`p980-*` was captured 07:05–07:06 (before the 07:14 merge); `c5ca-*` 07:17–07:34 (after it).
Pixel comparison `p980-settings.png` vs `docs/ui-audit-baseline/settings-980x680-dark.png`
=> 28,889 differing pixels on a 980×680 canvas (~4.3%): a different UI state, not a clean match.
The pre-fix zigzag UI is visually confirmed in `p980-settings.png` / `p980-performance.png`.

Consequence: the "980-набор" that `visual-verdict` is defined over is the **pre-fix** UI, so
its screenshots cannot "соответствовать зафиксированной симметрии (F1-F4b PASS)" —
the F4b property is exactly what they contradict.

## Fact 3 — the card's own instruction removes the only means to discharge `visual-verdict`

The card says not to duplicate pixel measurements because the geometry verdict already PASSed in
`t_4035d84d`. But the expected observable of `visual-verdict` is a *correspondence* claim, and
correspondence can only be shown by comparing the 980-set against the locked symmetry — i.e. by
the very measurement the card forbids. With the ban in force, the check is unsatisfiable as
written, and its true answer for `p980-*` is the opposite of the text the card pre-supplies.

## Fact 4 — candidate identity is triply inconsistent

    card `candidate.git_sha`            fb894f295222d225c387e9dd5cfeba2328290425 (docs-only, HEAD)
    scope-named check candidate         5ca36ab   (PASSed scripts/verify task; but PRE-fix tree)
    artifact candidate                  c5ca-*  -> 5f0d3da tree (POST-fix)
                                        p980-*  -> pre-fix tree (no single named revision)

None of these three agree. `fb894f2` (committer 07:52:21) is docs-only: it changes only
`docs/planning-log.md` and `docs/verification/int-5f0d3da/*` on top of `5f0d3da`, so it can
neither validate nor invalidate the UI artifacts.

The board's own planner log records the governing rule for exactly this situation:
"merging can silently drop a fix ... **merge-invariant acceptance evidence must be produced by
the planner, not inherited from the branch that was merged**", and it names `5f0d3da` as the
accepted v0.1 candidate identity. The card names a different candidate.

## Fact 5 — `honesty-verdict` evidence is thinner than the check text implies

`logs/status-diff-gpu-click.txt` shows only `secs_since_epoch`/`nanos_since_epoch` changes
(1790050857 -> 1790050855). `status-before-reselect.json` vs `status-after-reselect.json` differ
only in the two `observed_at` timestamps. `gpu.power` is `unavailable`
("The name is not activatable" — hardwared not running), so the probed path could not exercise a
mutation at all. The probes do not by themselves support the check's claim that "кнопки не
мутуют железо" beyond the trivial fact that nothing changed.

## Why this is stopped rather than turned into a verdict

`visual-verdict` has no satisfiable true answer against the artifacts as scoped, and the
candidate to certify is ambiguous between three values, two of which the planner has explicitly
reserved as his own acceptance call. Deciding which candidate the artifacts certify, or
declaring a pre-fix artifact set as evidence of a post-fix symmetry PASS, is a coverage/integrity
judgement outside a QA verifier's authority. Per contract this is a `BLOCKED` / `needs_input`
handoff, not a self-invented criterion and not a silently narrowed coverage.

## Environment facts

    workspace               /home/artt/Orbis-control-implementation
    workspace HEAD          fb894f295222d225c387e9dd5cfeba2328290425 (branch implementation/current-plan)
    tracked-file changes    none (read-only analysis)
    acceptance-contract.yaml present at repo root:
      delivery_target COMPLETE_SMALL_PRODUCT, placeholder_policy FORBIDDEN, locale ru-RU
    evidence root           /home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/

## Reproduction commands

    git show 5ca36ab:ui/components/nav-item.slint | grep -n 'x:'
    git show 5f0d3da:ui/components/nav-item.slint | grep -n 'x:'
    python3 .qa-t0c45ec21/tree_match.py
    python3 .qa-t0c45ec21/nav_geometry.py
