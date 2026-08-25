# Roadmap

> Роль: **FUTURE PLAN**. Очередность работ без календарных обещаний.
> Обновлено: **2026-08-24**.
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
- Draft PR #129 was created as the explicit integration checkpoint; closed 2026-08-25 as superseded — its head `e8b611e` is a verified direct ancestor of `development`.
- Read-only ASUS FA707NV baseline evidence captured on `asus-hardware-validation-20260821` at `67f9913`; no hardware mutation performed.

Remaining:

1. resolved 2026-08-25: #106 closed by single-owner policy; GitHub Actions is optional/manual and local flake checks are canonical;
2. resolved 2026-08-25: #114 closed as not required by project policy; `main` uses explicit review and local checks;
3. resolved 2026-08-25: obsolete refs pruned (#118) — the superseded PR-head branch is deleted after verifying its tip is an ancestor of `development`; only `main`/`development` remain;
4. resolved 2026-08-25: #124 identity decision recorded in ADR 0013; `io.github.orbiscontrol.Orbis` is permanent and hosting-independent;
5. resolved 2026-08-25: integration lines reconciled — Draft PR #129 closed as superseded; `development` is the single integration line, and any merge into `main` waits for local executable validation and resolved safety blockers.

The ASUS baseline is observation evidence only. It does not promote any unvalidated
write or product capability to `Supported`.

## Milestone 1 — Runtime reliability and security

**Status: HIGHEST PRIORITY**

1. **#125 GUI root boundary** — reject interactive euid 0 before preferences, runtime or D-Bus setup. Screenshot/offscreen behavior must remain explicit and testable.
2. **#123 bounded execution — CLOSED (2026-08-24)**: telemetry owns provider identity/deadline, Hardware1 mutation-status requery is bounded (`HARDWARE1_STATUS_DEADLINE`), mutation unknown outcomes classify as `Unconfirmed` with no blind retries; full local flake evidence is green.
3. **#107 Battery owner/interface liveness — CLOSED (2026-08-25)**: dynamic bounded non-mutating owner probe, fresh uncached threshold read, interface-drift classification and live FA707NV confirmation are complete.
4. **#117 telemetry coverage/freshness — CLOSED / SOURCE-COMPLETE (2026-08-24)**: UI `telemetry_fresh` means a recent useful observation; empty/partial/field-local evidence and Diagnostics gap export are implemented and pinned by tests.

#112 is source-complete and carries executed evidence: explicit and periodic capability refresh share the same canonical mutation-status requery path, and a private-P2P integration test proves a real BackendMissing→Supported→Unknown transition through explicit refresh (2026-08-24).

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

**Status: SOURCE COMPLETE (#115); executable claim recorded by the local full flake workflow**

- [x] #115 — production-native `UiState` startup with explicit Loading/Unknown/non-writable defaults and compile-time package version (`production_initial`);
- [x] `orbis-test-support` moved out of the normal GUI release dependency graph (dev-dependency + optional `ui-review` feature only; proven by `cargo tree -e normal`);
- [x] deterministic screenshot/mock construction kept in dev/test-only paths (`--screenshot` requires `--features ui-review`);
- [x] final release dependency graph and UI compilation claim recorded from the next full executable check run (2026-08-24, revision `a752be6`: workspace `fmt/check/test/clippy --locked` green, release tree clean of `orbis-test-support`, `cargo check -p orbis-ui --features ui-review --locked` green; evidence in [`current-state.md`](current-state.md)).

This also reduces the amount of production startup code that must sanitize fixture-derived state.

## Milestone 4 — Lifecycle / Desired state / Automation

**Status: FOUNDATIONS IMPLEMENTED; EXECUTION DELIBERATELY RESTRICTED**

Completed source foundations include versioned desired-state storage, lifecycle observations, Automation policy/revision/debounce/recovery/serialization and a Performance-only executor contract.

Next:

- #113 closed (2026-08-24): deprecated legacy config/path compatibility symbols removed after confirming zero consumers and executing the full workspace `--locked` suite green;
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

- #119 closed: executable local workspace validation plus an executable service-absent integration test recorded (`ebba8ea`);
- #117 telemetry truth improvements delivered: freshness honors snapshot quality and field-local gap evidence reaches Diagnostics/UI without fake freshness (2026-08-24);
- final packaged desktop/AppStream/tray/preferences acceptance on release revision.

## Milestone 6 — Product GPU and extended ASUS controls

**Status: BLOCKED BY CONCEPT-SPECIFIC EVIDENCE**

Eco / Standard / Ultimate / Optimized are product policies, not aliases for one raw backend enum. Raw GPU mutation remains disabled.

Panel, Keyboard and Aura typed code does not itself authorize product writes. Display Refresh has a typed request/read-back contract but no concrete compositor mutation owner. Updates has no canonical signed release feed/installer owner. Ambiguous ASUS controls remain disabled until their exact owner, constraints and confirmation semantics are proven.

## Milestone 7 — Packaging / sandbox / release acceptance

**Status: LOCAL EXECUTABLE EVIDENCE GREEN; RELEASE POLICY GATES REMAIN**

- #126 standalone/full hardwared sandbox — resolved with package/VM/live evidence;
- ADR 0013 permanent identity decision — resolved;
- hosted GitHub Actions optional/manual by single-owner policy;
- `main` required checks intentionally not used;
- final `cargo fmt/check/test/clippy --locked` — executed green locally;
- final `nix flake check` — executed green locally;
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
- application identity decision recorded in ADR 0013;
- local canonical checks and explicit owner review;
- live/device claims tied to exact revision and environment.

Release remains gated by packaging acceptance, independent hardware promotion gates and live/device evidence; hosted Actions recovery is not required by project policy.
