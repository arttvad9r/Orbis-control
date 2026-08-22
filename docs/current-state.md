# Current State

> Роль: **CURRENT STATUS**.
> Обновлено: **2026-08-21**.
> Source snapshot: development branch (consolidates `asus-hardware-validation-20260821` / PR #129 head `e8b611e`).
>
> `main` остаётся последней консолидированной baseline до отдельной интеграции этой ветки. Claims используют [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md): `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Активная интеграционная ветка содержит существенно более новый UI/runtime/backend слой, чем `main`. Source-level production вертикали существуют для Battery, Performance, независимых GPU read primitives, telemetry, fan reads, preferences/autostart/window lifecycle, diagnostics и read-only CLI.

Privileged mutation architecture остаётся узкой: original caller → typed `Hardware1` → capability-specific polkit → bounded backend/read-back. `sessiond` — user-session/read boundary и не является privileged deputy.

Production product path намеренно включает только доказанные writes. Performance и условно Battery — live mutation owners; raw GPU, Fan, Panel, Keyboard и Aura остаются product/policy blocked. Automation execution promotion=false, Display modeset не имеет concrete owner, Updates не имеет canonical signed feed/installer owner.

Главный release blocker — #106: GitHub Actions не предоставляет trustworthy executable validation. Локальный Rust/Cargo toolchain (flake devShell) доступен: на текущей ревизии `cargo fmt/check/test/clippy --locked` выполнены успешно, `python3 scripts/verify-static` проходит после `317f22d`, а `nix flake check --no-build` подтверждает только evaluation. Поэтому изменения текущей ветки можно называть `IMPLEMENTED`/`TESTED` по source и локальному executable evidence точной ревизии, но это не `PACKAGED`, не `LIVE-VALIDATED` и не восстановление hosted CI.

## Repository status

- Active integration branch: `development` (pushed to `origin/development`; consolidates the former `asus-hardware-validation-20260821` line).
- Draft integration PR: #129 (last known state: Draft; live status requires GitHub verification). Its head remains `e8b611e`; `development` now supersedes it as the integration line, and no merge decision has been made.
- `main` — previous consolidated baseline; required checks не включены из-за #106/#114.
- Единственный canonical workflow — `.github/workflows/ci.yml`.
- Удалён оставшийся одноразовый `.github/keyboard-backlight-probe-validation-trigger` artifact.
- Canonical docs hierarchy определена в `docs/README.md`; historical plans/audits отделены через `history.md`.
- Remote `agent/*` refs ещё требуют cleanup #118; текущий connector не предоставляет delete-ref operation.
- Read-only ASUS FA707NV evidence is recorded in [`hardware-evidence/asus-fa707nv-baseline.md`](hardware-evidence/asus-fa707nv-baseline.md).

## Current areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | CLEANED / IMPLEMENTED | One-off validation artifact удалён; canonical workflow/document hierarchy сохранены; интеграция консолидирована в ветку `development` (PR #129 head `e8b611e` сохранён как исходная история). |
| Rust/build contract | IMPLEMENTED | Workspace MSRV/toolchain contract — Rust 1.87. |
| Executable CI | BLOCKED — #106 | Actions failure/no-run occurs before trustworthy repository steps; не интерпретируется как Cargo/Nix result. |
| Main protection | DEFERRED — #114 | Required checks включать только после реально исполняемого CI. |
| Remote branches | CLEANUP PENDING — #118 | Старые `agent/*` refs остаются. |
| Application identity | DECISION OPEN — #124 | `io.github.orbiscontrol.*` permanence/ownership нужно решить до stable release. |
| Core/domain | IMPLEMENTED | Typed capability/state/action/lifecycle models, `Accepted != Applied`, Desired/Observed/Pending foundations. |
| Preferences/config | HARDENED / COMPAT CLEANUP OPEN — #113 | Checked XDG paths, inert defaults, atomic/durable stores; deprecated legacy compatibility symbols ещё существуют. |
| Autostart | IMPLEMENTED SOURCE — #110 closed | Owned XDG desktop entry read/write/read-back wired to Preferences. |
| Window/tray lifecycle | IMPLEMENTED SOURCE — #121 closed | Start Minimized, X11-style position restore/save, Wayland fail-closed behavior, StatusNotifier tray gating and explicit Quit lifecycle. |
| Diagnostics | IMPLEMENTED SOURCE — #111 closed | Runtime initialization, capability generation replacement, Refresh, privacy-bounded JSON export and Copy Summary wired; Open Logs intentionally disabled. |
| Battery read | IMPLEMENTED / CONFLICT-AWARE | Session1 exposes UPower, ASUS backend and sysfs threshold evidence; disagreements are `Conflict`, not `Supported`. |
| Battery mutation | HISTORICAL LIVE-VALIDATED / HARDENING OPEN | Controlled Hardware1 path; dynamic asusd owner/interface liveness remains #107. |
| Performance read/write | HISTORICAL LIVE-VALIDATED / CURRENT SOURCE IMPLEMENTED | Session1 read + Hardware1/polkit write + read-back. |
| GPU primitives | HISTORICAL LIVE-VALIDATED READS | Power, physical MUX and access policy remain separate read concepts. |
| GPU product/raw mutation | BLOCKED | Product modes are policy, not raw backend enum. Production raw mutation disabled. |
| Capability registry | IMPLEMENTED / RESILIENCE COVERED | Whole-swap immutable generations; explicit and periodic refresh share canonical mutation-status requery (#112 source-complete); backend loss/timeout and recovery publish capability-local availability transitions. UI/Diagnostics/support-policy source audit #120 is complete; Battery owner evidence remains #107. |
| Provider execution | PARTIAL HARDENING — #123 | Canonical `bounded_provider_call`; public probes, CLI and main worker read refreshes use provider deadlines. Telemetry/status requery and mutation unknown-outcome boundary remain. |
| CLI | IMPLEMENTED READ-ONLY / VALIDATION OPEN — #119 | `status` + versioned `status --json` (schema 2 includes battery threshold evidence); typed states; no mutation commands. |
| Telemetry | IMPLEMENTED / EVIDENCE GAP — #117 | Partial metrics are supported, but empty/useful/field-local failure coverage is not fully modeled. |
| Fan reads | IMPLEMENTED / EVIDENCE GAPS — #109/#116 | Per-fan Session1 read exists; aggregate capability still CPU-centric and stored `enabled` is not carried end-to-end. |
| Fan writes/reset | HARD-BLOCKED — #104/#105 | UI + polkit + production Hardware1 composition prevent the dormant unsafe write path. Successful factory-reset semantics are hardened in source: `ApplyResult::Accepted` (never `Applied`), Timeout/Dbus after possible dispatch → unknown-outcome, serialized through the worker FIFO with custom fan writes. Orbis-side #105 contract enforced by source test: the fan mutation/reset backend never switches the platform profile itself (any temporary switch stays inside asusd). |
| Panel write | HARD-BLOCKED | Typed API may exist; production backend status remains Unsupported/default-deny. |
| Keyboard write | HARD-BLOCKED | Current production Hardware1 reports Unsupported; root sandbox does not expose keyboard write path. |
| Aura write | HARD-BLOCKED | Static RGB typed writer is not promoted into product/unattended execution. |
| Automation | IMPLEMENTED SHADOW/EXECUTOR CONTRACT, EXECUTION BLOCKED | Worker owns policy/lifecycle/debounce/recovery/serialization; resume performs read-only provider/capability refresh and never restores desired hardware state. Performance executor compiled but promotion=false. Other executors disabled. |
| Display Refresh | IMPLEMENTED CONTRACT / NO MUTATION OWNER | Typed target/request/read-back design exists; no concrete compositor configuration owner is enabled. |
| Updates | FAIL-CLOSED | Installation owner detection + typed blockers exist; no invented release feed/downloader/installer. |
| Release dependency graph | OPEN — #115 | GUI still has normal `orbis-test-support` dependency because production/offscreen startup share fixture-derived UiState bootstrap. |
| GUI root boundary | IMPLEMENTED SOURCE — #125 | Interactive launch rejects euid 0 before preferences/runtime/bus setup; screenshot/offscreen paths remain allowed. |
| Hardwared sandbox | STRUCTURALLY MINIMIZED / VALIDATION OPEN — #126 | Intended direct sysfs write surface is `platform_profile` only; executable package/VM proof awaits #106/tooling. |
| ASUS FA707NV live read baseline | OBSERVED / READ-ONLY | `platform_profile`, asusd/asusctl profile, UPower, DRM/sysfs GPU, hwmon, thermal and power_supply reads were observed on FA707NV; Session1/Hardware1/hardwared and supergfxd were unavailable. This does not promote write support. |
| ASUS FA707NV platform profile mutation | LIVE-VALIDATED (revision-scoped) | Controlled Hardware1 apply/read-back/restore was validated on-device; see [`hardware-evidence/fa707nv-platform-profile-validation.md`](hardware-evidence/fa707nv-platform-profile-validation.md). Evidence is scoped to that revision and environment; it does not extend to later changes or other capabilities. |
| Research foundations (policy/preset/reconciliation/fan-policy/transaction/readiness/system-telemetry) | FOUNDATION | Merged modules are exported from `orbis-core`/`orbis-config`/`orbis-providers` public APIs but have no runtime consumers: worker, UI, sessiond and hardwared do not call them. Loading config remains hardware-inert. See "Research-foundation status" below. |

## Bounded execution status (#123)

Implemented in source:

- `orbis_providers::bounded_provider_call(provider, operation, future)` owns provider deadlines;
- no retry is performed by the generic timeout primitive;
- public capability probes use bounded adapters;
- Performance probe gives `profiles()` and `current_profile()` separate deadlines;
- probe timeout becomes local `TemporarilyUnavailable`, not false `Unsupported`;
- `orbisctl` reuses the same primitive;
- worker Battery, Performance, Fan and GPU authoritative refresh reads are bounded;
- GPU power/MUX/access reads execute independently with `tokio::join!`;
- explicit and periodic capability refresh use the same canonical refresh helper.

Still open:

1. telemetry now exposes canonical provider identity/deadline through `TelemetryServiceRuntime` (`provider_id`/`snapshot_timeout`), and snapshot reads remain bounded via `provider.timeout()`; executable validation of the exact revision remains;
2. Hardware1 mutation-status requery is bounded (`HARDWARE1_STATUS_DEADLINE`, timeout → `Unknown`);
3. mutation timeout after possible dispatch is now classified across interactive Performance/Battery, the Automation Performance executor and the (gated) Fan Factory Reset path as `CommandError::Unconfirmed`/recovery: never success, never retried, rollback not auto-triggered; the outcome is obtained only through a subsequent authoritative read-back that confirms or refutes the desired state. Successful factory reset returns `ApplyResult::Accepted` (not `Applied`) because an independent default-evidence source does not exist;
4. `orbisctl validate` bus connects and status query are bounded (`VALIDATE_BUS_CONNECT_DEADLINE`/`VALIDATE_STATUS_DEADLINE`); the interactive confirmation and the confirmed mutation itself remain intentionally outside generic timeout (unknown-outcome contract);
5. hosted CI execution remains blocked (#106).

## Effective product-policy truth (#120 complete)

The current source contract is deliberately conservative:

- Performance/Battery/Fan UI writability derives from `operations.write.status`;
- Diagnostics carries and renders read/write statuses independently;
- Panel/Keyboard request paths require explicit `ProductWriteStatus::Supported`;
- support-matrix schema requires separate read/write evidence and the repository currently contains only `empty`/`unknown` examples, not optimistic model claims;
- current deliberately disabled/unvalidated product writes remain effective `Unsupported` with a reason rather than a generic `DisabledByPolicy` status that could imply already-proven hardware support.

These invariants are protected by the static backend completion contract. Executable validation still belongs to #106.

## Fan truth status (#109/#116)

The old claim that a requested CPU read requires both CPU and GPU curves is no longer true in this branch: Session1 profile-specific reads target a concrete `(profile, fan)` and strict-decode the returned fan.

Remaining fan evidence defects:

- registry still publishes one aggregate `FeatureId::FanCurves` from a CPU probe, so CPU support can still overstate UI-visible GPU support (#109);
- stored `FanCurveData.enabled` is still lost before the final Session1/client/UI observation (#116);
- custom write preservation of `enabled` and factory-reset profile restoration remain mandatory future-write blockers (#104/#105).

No fan writes should be enabled while these remain unresolved.

## Production boundaries

### Reads

```text
authoritative backend
→ Session1 / typed read provider
→ bounded application/worker read
→ GUI / CLI / diagnostics
```

A read failure is local evidence. Missing/denied/unavailable/unknown are not interchangeable and must not produce fake hardware defaults.

### Enabled mutations

```text
original GUI/application caller
→ Hardware1 system bus
→ capability-specific polkit
→ typed Performance or Battery backend
→ authoritative read-back / explicit result
```

Current product block remains defense-in-depth: UI gating + Hardware1 disabled backend + polkit/default sandbox where applicable.

## Work still possible while GitHub Actions remain blocked (#106)

Local executable Rust checks are available in the flake devShell and were executed green on this revision (`cargo fmt/check/test/clippy --locked`; `python3 scripts/verify-static` PASS after `317f22d`). This is revision-scoped `TESTED` evidence only. It does not restore trustworthy hosted CI (#106), does not prove packaging/VM acceptance, and does not create live hardware evidence for this branch.

Safe source work may continue when it does not widen unvalidated hardware writes:

- docs/source-of-truth cleanup;
- stale issue/repository artifact cleanup;
- static contracts;
- read-only/evidence improvements;
- fail-closed runtime hardening whose source semantics are locally reviewable;
- preparation of tests that will execute later.

Do not promote fan/GPU/Panel/Keyboard/Aura writes, unattended Automation, Display modeset or self-update on static or local-test evidence alone.

## Research-foundation status (FOUNDATION, unwired)

The consolidated `development` branch includes domain modules merged from the research-mechanics line. They are honest foundations, not production features:

- policy/desired-state presets (`orbis-config::policy_desired`, `orbis-core` preset/preset-bundle);
- reconciliation decision/scheduling primitives;
- software fan-policy primitives (EMA, hysteresis, PWM rate limiting, interpolation);
- mutation transaction phases and audit model;
- readiness model with bounded probe helpers;
- system telemetry (Linux memory/PSI/zram/zswap parsers);
- telemetry history/export/statistics primitives;
- thermal-control pure computations;
- hardware-validation evidence state machine.

All of them are exported from crate public APIs but have **no runtime consumers**: the worker, UI composition, sessiond and hardwared do not call them. No preset import, policy selection or reconciliation pass can dispatch a hardware action today; configuration loading remains hardware-inert. Wiring any of these into production requires its own design step and must preserve Desired/Observed/Pending separation, `Accepted != Applied`, and the existing fail-closed product gates.

## Active blockers / next work

1. #106 executable CI/tooling recovery.
2. #125 GUI root guard.
3. #123 remaining telemetry/status/mutation unknown-outcome timeout design.
4. #107 dynamic Battery mutation owner/interface liveness.
5. #117 telemetry coverage/freshness semantics.
6. #109 aggregate CPU/GPU fan capability truth.
7. #116 fan stored-enabled read evidence.
8. #115 production-native UiState + release dependency graph cleanup.
9. #113 removal of deprecated legacy config/path API after executable compatibility validation.
10. #119 CLI integration/executable validation.
11. #126 package/VM sandbox validation.
12. #124 application identity decision.
13. #118 old remote branch cleanup when delete-ref access exists.
14. #114 required checks after #106.

## Historical live evidence retained

Revision-scoped evidence exists for controlled Battery and Performance mutations, independent GPU primitive reads and Hardware1/original-caller authorization on documented hardware. It is historical evidence, not a universal ASUS support guarantee and not proof for the current unexecuted branch.

## References

- [`architecture.md`](architecture.md)
- [`verification.md`](verification.md)
- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`backend-completion-status.md`](backend-completion-status.md)
- [`beta-acceptance-checklist.md`](beta-acceptance-checklist.md)
- [`roadmap.md`](roadmap.md)
- [`history.md`](history.md)

Update this file whenever active production behavior, capability evidence or release blockers change. Source inspection proves at most `IMPLEMENTED`; stronger claims require executed evidence.
