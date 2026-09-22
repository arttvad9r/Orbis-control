# Orbis Control — Feature Consolidation Transfer Plan (2026-09-22)

**Status:** approved direction — base = `implementation/current-plan` (user decision, this session).
**Author:** planner. **Evidence:** `docs/LINEAGE-RECON-2026-09-22.md`, measurements below.

## 1. Why this plan exists

Three divergent lines grew from one merge base `e72bbb3`:

| line | tip | commits from base | what it carries |
|---|---|---|---|
| `implementation/current-plan` | `5084148` | 34 | power-limit pipeline (C), product GPU status (D1/D2), fan-curve production (B), LACT telemetry, UI symmetry fixes F1–F4b, G02 fix, SPEC/ACCEPTANCE/contracts, verification evidence |
| `origin/agent/finish-v01` | `e1a409c` | 100 | **RGB sliders (ValueSlider) + secondary colour + 12 Aura effects**, Arch `PKGBUILD`, clamshell inhibitor, CPU package limits provider, diagnostics pipeline, secondary windows, themes, app-mark, rewritten UI pages, screenshots |
| `origin/feature/control-surfaces-v1` | `11d03ce` | 2 | read-only power observations; superseded by finish-v01 (finish-v01 already has clamshell + `asus_armoury_power_limits`) |

`/usr/bin/orbis-control` (package `local/orbis-control 0.1.0-2`, built 2026-09-09) contains
`Красный` / `Зелёный` / `Синий` / `Вторичный цвет`, `Breathe Static`, `Rainbow Wave`, `Comet`,
`Clamshell` — strings that exist **only** on `finish-v01`. The running application **is** finish-v01.

## 2. Measured change footprint (not estimated)

```
changed vs base:  ours 231 files   theirs 142 files
ONLY OURS   : 176
ONLY THEIRS :  87
BOTH        :  55   (17 slint, 27 rust, 11 other)
```

A plain `git merge` yields **36 conflicted files — all of them "both changed"**, i.e. there is no
mechanical resolution anywhere; every conflict is semantic.

Two blanket experiments, measured in an isolated worktree (shared checkout untouched):

| resolution | result |
|---|---|
| `-X ours` | 15 compile errors **and it discards their features** in conflicted files (RGB sliders gone) |
| `-X theirs` | drops **our** power-limit D-Bus protocol (`PowerLimitsTuple` unresolved) |

**Both blanket modes are wrong.** The resolution is per-file.

## 3. Decisive finding: the seams are additive

Proven, not assumed: our `orbis-session-protocol/src/lib.rs` has `power_limit_field`,
`PowerLimitsTuple`, `power_limits()`, `gpu_mux`, `gpu_access`; theirs has a `clamshell` module and
two clamshell trait methods. The two sides touch **disjoint** symbols. Unioning them by hand
(our file + their `clamshell` block + their two trait methods) **compiles clean**
(`cargo check -p orbis-session-protocol` → `Finished`, 1 unrelated warning).

After that union the workspace check went **12 errors → 9**, all of them our power-limit wiring
(`power_limit_backend` / `power_limit_authorizer` fields, `SessionService::with_power_limits` /
`read_power_limits`, `build_session_server`, one duplicate `handle_product_gpu_status`) plus a
slint build error caused by mixing our geometry with their components. This is a **bounded,
enumerable seam set in 2 crates**, not a rewrite.

`origin/agent/finish-v01` checked out standalone builds clean: `cargo check` → `Finished`, 0 errors.

## 4. Ownership matrix (the contract for the landing worker)

| group | count | owner | rule |
|---|---|---|---|
| `ui/**` slint + components + themes + app-mark | 17 both + ~20 theirs-only | **THEIRS** | their UI is a superset rewrite (see §5); our geometry fixes do not apply to it |
| `crates/*` rust, both-changed | 27 | **SEMANTIC UNION** | ours where it is our feature (power limits, product GPU, LACT); theirs where it is their feature (clamshell, diagnostics, aura); union at the protocol seam |
| ours-only rust/tests/docs/scripts | 176 | **KEEP OURS** | power-limit pipeline, product GPU status, fan production, LACT, contracts, evidence |
| theirs-only rust/tests/packaging | 87 | **KEEP THEIRS** | PKGBUILD, clamshell, CPU package limits, diagnostics pipeline, secondary windows |
| `automation_*` (26 files, ours-only) | 26 | **KEEP OURS** | their `0c8e37b` deleted them; D-012 (locked) keeps automation in scope (parity waves P1–P6). They are fail-closed, hardware-inert scaffolding — inert, not active code |
| docs | — | **OURS WINS** | SPEC/ACCEPTANCE/DECISIONS/contracts/evidence are ours; keep their `FINISH_PLAN.md`, `docs/product.md`, `docs/research/` as additions |

## 5. Why our UI fixes are re-established, not ported

- `ui/components/nav-item.slint`: ours 100 lines with `focus-scope`; theirs 56 lines, no
  `focus-scope`, different structure. Our F4a/F4b icon-box fix targets a geometry that no longer
  exists.
- `ui/audited/sections/performance.slint`: ours 168 lines with **18** power-limit references;
  theirs 146 lines with **0**. On their performance page the limits card is absent entirely.
- Their components are new and load-bearing: `app-mark`, `metric-card`, `mode-card`,
  `request-toggle-row`, `steppers`, `fan-curve-chart`, `preview-dialog-window`, `ui/themes/*`.

Consequence: the G02 phantom-band fix has **no target** on their page; the required power-limits and
boost UI (REQ-POWER, REQ-BOOST, AC-*) must be **re-established on their layout** as UI work, with a
fresh geometry measurement — not carried over as a patch.

## 6. Stages (minimum topology)

1. **LAND** (developer / implementation, worktree) — land the merge on `current-plan`; resolve the
   55 both-changed per §4; union the protocol seam; keep automation modules. Exit: `cargo check
   --workspace --locked` and `cargo test --workspace --locked` green, single candidate commit.
2. **UI-PARITY** (ui / ui, worktree, after LAND) — on the landed UI restore and prove the required
   UI features on their layout: Aura sliders + 12 effects, power-limits card, boost vertical,
   capability/Unavailable states, both themes, 980×680 and 1200×800; geometry measured, not eyeballed.
3. **QA** (qa / qa, after UI-PARITY) — user-liveness smoke on the assembled candidate, interaction
   inventory, contract + visual/product + regression checks, independent of both writers.
4. **E** (developer / implementation, after QA) — Arch package from `packaging/arch/PKGBUILD`;
   install candidate; smoke the installed binary.
5. **R** (planner — terminal release acceptance, after E).

Every stage keeps its own worktree, its own frozen candidate and its own evidence. No verdict from
the pre-merge candidate is reused.

## 7. What is explicitly NOT covered

- `feature/control-surfaces-v1` is not merged (superseded, see §1).
- No push to origin; `main` untouched.
- Their 22 deleted `automation_*` modules are retained but remain hardware-inert; wiring automation
  to hardware is parity waves P1–P6, after v0.1.
