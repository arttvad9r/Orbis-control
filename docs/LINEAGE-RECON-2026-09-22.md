# Orbis Control — Lineage Reconnaissance (2026-09-22)

**Trigger:** user report that the running application has RGB *sliders* which our line lacks.
**Verdict:** confirmed. There are THREE divergent lines from a common base. Our line is
NOT the newest, and it is NOT the line the user's installed package came from.

## Common base

`e72bbb3` (2026-09-05, `docs: bind agents to canonical finish plan`) —
shared ancestor of `main`, `development`, `feature/control-surfaces-v1`,
`agent/finish-v01`, and our `implementation/current-plan`.

## The three lines

| line | tip | date | ahead of base | what it is |
|---|---|---|---|---|
| `origin/agent/finish-v01` | `e1a409c` | 2026-09-09 | **100** | the real product completion line; PKGBUILD, RGB sliders, clamshell, POST sound, CPU limits |
| `origin/feature/control-surfaces-v1` | `11d03ce` | 2026-09-13 | 2 | thin continuation of base; read-only power observations |
| `implementation/current-plan` (ours) | `e838430` | 2026-09-22 | 33 | SPEC/ACCEPTANCE/board-contracts + LACT + D1/D2 GPU status + UI symmetry work |

`origin/development` and `origin/main` are both still at the base `e72bbb3`.

## Proof that the installed app comes from `finish-v01`

- Installed package: `orbis-control 0.1.0-2`, built **2026-09-09 18:50**, installed 20:12.
- Its `PKGBUILD` (only PKGBUILD in the whole repo, exists **only** on `finish-v01`)
  is pinned to `_commit=4ce963c2161537b90fcecbe3df6519a61de1818c` — a `finish-v01` commit
  dated 2026-09-09.
- Binary string check on `/usr/bin/orbis-control` (UTF-8 aware, `grep -a`):

| string | in installed binary | in our `target/release/orbis-control` |
|---|---|---|
| `Красный` / `Зелёный` / `Синий` | yes | **no** |
| `Вторичный цвет` | yes | **no** |
| `Быстрый статичный RGB` | yes | no |
| `Rainbow Wave`, `Comet`, `Ripple`, `Color Cycle`, `Laser`, `Highlight`, `Flash`, `Pulse` | yes | (mostly) no |
| `Clamshell` | yes | no |

Source `4ce963c:ui/audited/sections/backlight.slint` contains all of those strings
(verified via `git show`), so the installed binary and the `finish-v01` source agree.

## Feature matrix (content-based, not filename-based)

| feature | finish-v01 | current-plan (ours) |
|---|---|---|
| RGB sliders R/G/B + secondary colour + 12 Aura effects | **yes** (7 `ValueSlider`, 21 effect labels) | **no** (`ValueSlider`=0, effects=0) |
| Arch `PKGBUILD` | **yes** (`packaging/arch/PKGBUILD`) | no (only `.gitkeep`) |
| Clamshell / lid inhibitor | **yes** (`sessiond/clamshell.rs`, 13 files) | no |
| POST / boot sound | **yes** (`firmware.rs` + SetBootSound) | no |
| CPU package limits (Armoury) | yes | yes |
| amd-pstate EPP diagnostics | **yes** (`cpu_frequency.rs`) | no |
| power-profiles-daemon delegation | yes | no (we do have `power_limits.rs`) |
| LACT telemetry | no | **yes** |
| D1/D2 product GPU status | yes | yes |
| canonical `screenshots/` (10) + UI-review CI | **yes** | no |
| `UI_AUDIT_BASELINE.md` (F1–F4b) | no | **yes** |
| board contracts / acceptance-contract.yaml | no | **yes** |
| SPEC / ACCEPTANCE / DECISIONS / IMPLEMENTATION_PLAN | no (has its own TODO.md) | **yes** |
| G02 phantom-band fix (conditional pair) | no (defect absent — different markup) | **yes** |

## Mergeability: the lines are NOT cheaply reconcilable

`git merge-tree` dry run (`current-plan` <- `finish-v01`), **non-destructive, no checkout**:

- **36 real content conflicts** (125 file-level touches).
- Conflicted: `AGENTS.md`, `crates/orbis-hardwared/src/{fans,lib,main}.rs`,
  `crates/orbis-providers/src/{asus_armoury,fan_defaults}.rs`,
  `crates/orbis-session-client/src/lib.rs`, `crates/orbis-session-protocol/src/lib.rs`,
  `crates/orbis-sessiond/src/{composition,service}.rs`,
  `crates/orbis-ui/{examples/ui_snapshot.rs,src/controller.rs,src/diagnostics_runtime.rs,src/main.rs,src/worker_runtime.rs}`,
  `data/polkit-1/actions/...policy`, `packaging/install-arch.sh`,
  `packaging/orbis-hardwared.service`, and 13 `ui/**` slint files.
- Modify/delete: `crates/orbis-ui/src/display_refresh_service.rs` deleted on `finish-v01`,
  modified by us.
- `finish-v01`'s first unique commit `0c8e37b` **deleted 22 `automation_*` modules**
  that we still carry (they came from the common base).

## Docs duplication

`SPEC.md` and `ACCEPTANCE.md` are **byte-identical** between
`feature/control-surfaces-v1` and our line (same blobs `c528ce4a…` / `5ad33c27…`);
`DECISIONS.md` and `IMPLEMENTATION_PLAN.md` diverge. `finish-v01` has none of them —
it uses `TODO.md` as its completion queue.

## Existing decision that this overturns

`DECISIONS.md` (ours, line ~120) records: *"Draft PR #130 (`agent/finish-v01`) закрыт как
устаревший — работа продолжается в ветке `implementation/current-plan`"*.
That judgement is now falsified by evidence: `finish-v01` is not the older/obsolete
line, it is the **more complete** one (100 commits, shipped package), and it carries
the RGB sliders the user is asking about.

## Other places checked (nothing else exists)

- No second checkout anywhere: only `/home/artt/Orbis-control-implementation`.
- `/home/artt/.orbis-worktrees` exists but is **empty**.
- `/home/artt/Orbis-control` (referenced by `Projects/pc-control-mcp/docs/ORBIS_AUDIT.md`)
  does **not** exist — that doc points at a path that is gone.
- `pc-control-mcp` is an unrelated MCP project; it only documents Orbis's battery
  boundary as a reference.
- One stash: `stash@{0}` = `wip: D gpu-mode read_status + RefreshProductGpuStatus vertical`
  (already superseded by our D1/D2 commits).
- No dangling commits; one dangling tree `1f93db5` (the merge-tree dry run output).
- `git fsck` clean otherwise.

## Working-tree state of our line

HEAD `e838430` = `99d3e969` (G02 conditional-pair fix) + `38adcc6`/`e838430` (evidence).
The G02 fix is real and verified: phantom band **213px -> 59px** (56px = natural card gap),
`scripts/verify task` exit 0. Untracked: `.int-smoke/`, `.qa-t0c45ec21/`.
