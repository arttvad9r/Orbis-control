# Roadmap

> Роль: **FUTURE PLAN**. Очередность работ без календарных обещаний.
> Обновлено: **2026-08-21**.
> Фактическое состояние — [`current-state.md`](current-state.md).

## Principles

1. Safety и truthful state важнее количества функций.
2. Read/write evidence независимы; model/DMI name не доказывает support.
3. `Accepted != Applied`.
4. Timeout после возможного mutation dispatch — unknown outcome, а не повод для blind retry.
5. Неподтверждённые writes остаются fail-closed в UI + Hardware1 + policy/sandbox where applicable.
6. Source inspection может доказать `IMPLEMENTED`, но не заменяет executable Cargo/Nix/Slint evidence.
7. Старые audits/plans не являются backlog source of truth; активная работа живёт здесь + GitHub Issues.

## Milestone 0 — Repository and release baseline

**Status: SOURCE CLEANUP CURRENT; EXECUTABLE CI BLOCKED**

Completed in the active integration branch:

- canonical docs hierarchy;
- one canonical `.github/workflows/ci.yml`;
- removal of obsolete/private/generated documentation artifacts;
- removal of the remaining `.github/keyboard-backlight-probe-validation-trigger` one-off artifact;
- current UI/backend status normalized into `current-state.md` and `backend-completion-status.md`;
- source-complete Autostart (#110), Diagnostics (#111) and Window/Tray lifecycle (#121) issues closed;
- effective product write-truth consumer/support-matrix audit and policy decision completed (#120 closed);
- Draft PR #129 created as the explicit integration checkpoint for the active branch.
- Read-only ASUS FA707NV baseline evidence captured on `asus-hardware-validation-20260821` at `67f9913`; no hardware mutation performed.

Remaining:

1. #106 — restore trustworthy executable GitHub Actions;
2. #114 — after #106, protect `main` with the real required check names;
3. #118 — prune obsolete `agent/*` refs when delete-ref access exists;
4. #124 — decide permanent `io.github.orbiscontrol.*` identity before stable release;
5. reconcile integration lines: `development` (active) vs Draft PR #129 (head `e8b611e`); any merge decision waits for executable validation and resolved safety blockers.

The ASUS baseline is observation evidence only. It does not promote any unvalidated
write or product capability to `Supported`.

## Milestone 1 — Runtime reliability and security

**Status: HIGHEST PRIORITY**

1. **#125 GUI root boundary** — reject interactive euid 0 before preferences, runtime or D-Bus setup. Screenshot/offscreen behavior must remain explicit and testable.
2. **#123 bounded execution** — source work already covers public probes, CLI and core worker reads. Remaining:
   - telemetry timeout ownership;
   - bounded Hardware1 mutation-status requery;
   - explicit mutation unknown-outcome/recovery semantics;
   - no blind retries.
3. **#107 Battery owner/interface liveness** — dynamic bounded non-mutating evidence across daemon restart/disappearance.
4. **#117 telemetry coverage/freshness** — useful observation vs empty/partial/field-local failures.

#112 is source-complete: explicit and periodic capability refresh share the same canonical mutation-status requery path. It remains validation-only work until executable CI returns.

## Milestone 2 — Fan read/evidence correctness

**Status: READS AVAILABLE; WRITES HARD-BLOCKED**

Before any fan write promotion:

1. #109 — aggregate `FeatureId::FanCurves` now requires both CPU and GPU read contracts; single-fan failure (including `BackendMissing`/`PermissionDenied`) suppresses the aggregate (source-complete; executable validation still required).
2. #116 — stored `FanCurveData.enabled` now reaches typed Session1/client/UI evidence and is rendered from `UiState` in the FansWindow (source-complete; executable validation still required).
3. #104 — dormant custom-write path now preserves authoritative `enabled` on write/read-back (hardwared read → pass-through → read-back confirm; source-complete, executable validation still required).
4. #105 — Factory Defaults must restore original platform profile on every success/failure path or avoid temporary profile switching.
5. executable tests on exact revision.
6. controlled hardware validation with final fan/profile state proof.

The product-policy/UI/Diagnostics write-truth audit is complete under #120; future fan work must preserve those operation-level gating rules.

No fan write should be enabled to “test” these contracts.

## Milestone 3 — Release graph and production bootstrap

**Status: OPEN**

- #115 — introduce production-native `UiState` startup with explicit Loading/Unknown/non-writable defaults and compile-time package version;
- move `orbis-test-support` out of the normal GUI release dependency graph;
- keep deterministic screenshot/mock construction in dev/test-only paths;
- verify release dependency graph and UI compilation once toolchain is available.

This also reduces the amount of production startup code that must sanitize fixture-derived state.

## Milestone 4 — Lifecycle / Desired state / Automation

**Status: FOUNDATIONS IMPLEMENTED; EXECUTION DELIBERATELY RESTRICTED**

Completed source foundations include versioned desired-state storage, lifecycle observations, Automation policy/revision/debounce/recovery/serialization and a Performance-only executor contract.

Next:

- #113 — remove deprecated legacy config/path compatibility APIs after executable compatibility validation;
- wire research-foundation modules (presets, reconciliation, transaction, readiness) only through deliberate design steps that keep config loading hardware-inert;
- reconciliation must read authoritative Observed state before any action;
- Desired/Observed/Pending remain explicit;
- unattended Automation promotion remains `false` until exact-build executable validation;
- GPU/Fan/Battery/Display/Lighting automation executors remain disabled unless separately designed and validated.

## Milestone 5 — Stable read-only supportability

**Status: MOST UI LIFECYCLE SOURCE WIRING COMPLETE**

Completed source slices:

- Autostart (#110 closed);
- Diagnostics Refresh/Export/Copy (#111 closed; Open Logs intentionally disabled);
- Start Minimized / X11-style position / Wayland fail-closed / tray-close semantics (#121 closed);
- read-only CLI including versioned JSON;
- operation-level capability write truth through UI/Diagnostics plus support-matrix evidence contract (#120 closed).

Remaining:

- #119 executable CLI/service-absent/integration validation;
- #117 telemetry truth improvements should flow into Diagnostics/UI without fake freshness;
- final packaged desktop/AppStream/tray/preferences acceptance on release revision.

## Milestone 6 — Product GPU and extended ASUS controls

**Status: BLOCKED BY CONCEPT-SPECIFIC EVIDENCE**

Eco / Standard / Ultimate / Optimized are product policies, not aliases for one raw backend enum. Raw GPU mutation remains disabled.

Panel, Keyboard and Aura typed code does not itself authorize product writes. Display Refresh has a typed request/read-back contract but no concrete compositor mutation owner. Updates has no canonical signed release feed/installer owner. Ambiguous ASUS controls remain disabled until their exact owner, constraints and confirmation semantics are proven.

## Milestone 7 — Packaging / sandbox / release acceptance

**Status: BLOCKED BY EXECUTABLE ENVIRONMENT**

- #108 source fix needs real test/clippy execution;
- #126 standalone/full hardwared sandbox needs package/VM validation;
- #124 identity decision;
- #106 executable CI;
- #114 required checks after CI recovery;
- final `cargo fmt/check/test/clippy --locked`;
- final `nix flake check`;
- packaged startup, D-Bus, polkit, desktop/AppStream and support-matrix acceptance;
- dated live evidence for device-specific claims.

## Release gate

A release candidate requires all of the following:

- intended integration branch merged into `main`;
- no known unsafe write enabled by default;
- #125 enforced;
- #123 complete enough that provider/status hangs and mutation unknown outcomes cannot silently wedge/retry hardware operations;
- relevant Cargo/Slint tests/clippy executed green;
- `nix flake check` executed green;
- package/metadata acceptance complete;
- current capability/UI/support evidence matches shipped policy;
- application identity decision recorded (#124);
- `main` protected by required executable checks (#114);
- live/device claims tied to exact revision and environment.

Until #106/tooling is restored, continue only narrow source hardening, read/evidence work, repository cleanup, documentation and static contracts. Release remains **BLOCKED**.
