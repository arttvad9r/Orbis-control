# Backend Completion Status

> Status date: 2026-08-20.
> Source snapshot: `chatgpt/ui-refresh-ghelper-20260820`.
>
> Concise companion to [`current-state.md`](current-state.md) and [`architecture.md`](architecture.md). Source-level `IMPLEMENTED` does not imply executable validation.

## Production-connected source slices

- **Performance** — worker → application/provider mutation → authoritative read-back.
- **Battery charge limit** — Session1 read + Hardware1/application mutation + fresh read-back.
- **GPU primitives** — independent power / physical MUX / access-policy reads.
- **Telemetry** — read-only sysfs snapshot polling; coverage/freshness semantics still need #117.
- **Fan reads** — profile/fan-specific Session1 reads plus active-curve reads; fan writes remain hard-blocked.
- **Theme** — runtime + persistence.
- **Autostart** — owned XDG desktop-entry read/write/read-back (#110 source-complete/closed).
- **Start Minimized** — persisted and applied at launch.
- **Remember Window Position** — supported on X11-style positioning sessions, explicitly unavailable on Wayland.
- **Close/tray** — StatusNotifier host gating; HideToTray only with a live host; Quit terminates the Slint event loop (#121 closed).
- **Diagnostics** — runtime initialization, capability-generation replacement, Refresh, privacy-bounded JSON Export and Copy Summary (#111 closed). Open Logs remains disabled.
- **CLI** — `orbisctl status` plus versioned `status --json`; read-only only.
- **Display Quick Control** — authoritative read-only observation; no concrete mutation owner.
- **Keyboard/Panel product controls** — typed Hardware1 status/request/read-back surfaces exist but current production backend status remains Unsupported.
- **Aura** — observation/config-level typed surfaces exist; no promoted unattended hardware write.
- **Boot sound** — strict read-only firmware-attribute provider.
- **Updates** — installation-owner/blocker classification only; no fabricated feed/downloader/installer.

## Provider execution hardening

Canonical source primitive:

```text
bounded_provider_call(provider, operation, future)
→ Provider::timeout()
→ result/error unchanged, or ProviderError::Timeout
→ no retry
```

Current bounded coverage:

- public capability probes;
- `orbisctl` Battery/Performance/GPU reads;
- worker Battery refresh;
- worker Performance refresh and Automation recovery read;
- worker Fan curve refresh;
- worker GPU power/MUX/access refreshes.

GPU primitive reads run independently via `tokio::join!`, each with its own provider deadline.

Capability refresh now has one canonical explicit/periodic path: mutation statuses are re-queried before the next whole-swap registry generation is built (#112 source-complete).

Still open under #123:

- telemetry deadline ownership;
- bounded Hardware1 mutation-status requery;
- mutation timeout must enter explicit unknown-outcome recovery rather than generic failure/retry;
- executable Rust validation.

## Effective product write truth (#120 complete)

Main source consumers now use operation-level/equivalent typed write evidence:

- Performance/Battery/Fan UI writability comes from `cap.operations.write.status`;
- disabled reasons come from the same write status;
- Diagnostics copies and renders read/write states independently;
- Panel/Keyboard request paths require explicit `ProductWriteStatus::Supported`;
- `scripts/check-backend-completion-contract.py` protects these mappings.

The support-matrix schema requires separate read/write evidence. Current repository fixtures are only `empty`/`unknown` examples, so there is no optimistic generated model table to reconcile. The product decision is to keep deliberately disabled/unvalidated writes effective `Unsupported` with a reason. A generic `DisabledByPolicy` status is not added because it could imply underlying hardware support has already been proven.

## Automation

Production worker is `crates/orbis-ui/src/worker_runtime.rs`. It owns:

- persisted Automation policy + policy revision;
- raw telemetry/lifecycle observations;
- AC/Battery debounce and resume freshness/coalescing;
- lifecycle revision/supersession;
- capability-generation preflight/revalidation;
- replay-resistant single-slot serialization;
- Performance unknown-outcome recovery;
- Performance-only executor semantics with authoritative read-back.

`AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED` remains `false`. Unattended mutation is deliberately unreachable until executable validation of the exact branch. GPU/Fan/Battery/Display/Lighting automation execution remains disabled.

## Fan read/evidence status

Per-fan profile reads are now concrete: a requested CPU curve no longer requires a GPU curve in the same response, and the client strict-decodes returned fan/profile identity.

Remaining:

- #109 — aggregate `FeatureId::FanCurves` is still published from a CPU-only probe and can overstate GPU availability;
- #116 — stored `FanCurveData.enabled` is not carried end-to-end;
- #104/#105 — dormant write/reset safety defects remain mandatory before any future write promotion.

## Product/release-gated writers

Typed writer code does not equal product support. Current production Hardware1 deliberately reports disabled/Unsupported for raw GPU, Fan, Panel, Keyboard and Aura writes. This remains defense-in-depth with UI gating and packaged policy/sandbox restrictions.

Promotion requires all relevant evidence:

- exact typed owner;
- capability-specific authorization;
- non-mutating readiness preflight;
- authoritative read-back appropriate to the concept;
- explicit product-policy approval;
- executable validation of the exact build;
- live hardware validation where a hardware claim is made.

`Accepted` config confirmation alone cannot authorize unattended execution.

## Fail-closed boundaries

### Display Refresh

Typed target/request/preflight/read-back semantics exist. No concrete compositor configuration owner is enabled. No shell fallback is accepted.

### Updates

No canonical signed release feed or universal installer owner is defined. Network check/install/channel actions remain disabled rather than inventing package-manager/self-replacement behavior.

### Ambiguous Extra controls

Status LEDs, clamshell/ASPM/standby-networking/iGPU-memory/CPU-core/hotkey concepts stay disabled until exact domain ownership and semantics are proven. Similar ASUS names are not treated as equivalent automatically.

## Remaining high-value source blockers

- #125 — early interactive GUI euid-0 rejection;
- #123 — remaining timeout/unknown-outcome contract;
- #107 — dynamic Battery mutation owner/interface liveness;
- #117 — telemetry useful/partial/empty evidence;
- #109/#116 — fan read evidence;
- #115 — production-native UiState and removal of normal `orbis-test-support` GUI dependency.

## Validation state

`scripts/verify-static` provides standard-library source contracts for UI, Automation, Display, backend completion, provider-timeout and documentation-status invariants. It is a fail-fast safety net only.

A fresh Draft PR #129 CI run again failed before repository steps (`steps=null`), so #106 remains an external Actions execution blocker. The available environment also cannot run Rust/Cargo/Slint locally. Therefore this branch is not claimed as passing `cargo check/test/clippy`, Slint compile or final Nix/package acceptance.
