# Backend Completion Status

> Status date: 2026-08-21.
> Source snapshot: development branch (consolidates the former `asus-hardware-validation-20260821` line).
>
> Concise companion to [`current-state.md`](current-state.md) and [`architecture.md`](architecture.md). Source-level `IMPLEMENTED` does not imply executable validation.

## Production-connected source slices

- **Performance** — worker → application/provider mutation → authoritative read-back.
- **Battery charge limit** — Session1 read + Hardware1/application mutation + fresh read-back.
- **GPU primitives** — independent power / physical MUX / access-policy reads.
- **Telemetry** — read-only sysfs snapshot polling; coverage/freshness semantics implemented and pinned by #117 (closed).
- **Fan reads** — profile/fan-specific Session1 reads plus active-curve reads; fan writes remain hard-blocked.
- **Theme** — runtime + persistence (Catppuccin Mocha/Latte).
- **Autostart** — owned XDG desktop-entry read/write/read-back (#110 source-complete/closed).
- **Start Minimized** — persisted and applied at launch.
- **Remember Window Position** — supported on X11-style positioning sessions, explicitly unavailable on Wayland.
- **Close/tray** — StatusNotifier host gating; HideToTray only with a live host; Quit terminates the Slint event loop (#121 closed).
- **Diagnostics** — runtime initialization, capability-generation replacement, Refresh, privacy-bounded JSON Export and Copy Summary (#111 closed); surfaced as actions in Settings → Diagnostics. Open Logs remains disabled.
- **CLI** — `orbisctl status` plus versioned `status --json`; read-only only.
- **Display Quick Control** — authoritative read-only observation; no concrete mutation owner.
- **Keyboard/Panel product controls** — typed Hardware1 status/request/read-back surfaces exist but current production backend status remains Unsupported.
- **Aura** — observation/config-level typed surfaces exist; no promoted unattended hardware write.
- **Boot sound** — strict read-only firmware-attribute provider.
- **Updates** — UI surface removed (real updates need a distribution server); typed providers retained, no UI consumer.

## ASUS FA707NV read-only baseline

The dated read-only snapshot is [`hardware-evidence/asus-fa707nv-baseline.md`](hardware-evidence/asus-fa707nv-baseline.md).
It records a readable `platform_profile` (`balanced`, choices `quiet/balanced/performance`),
active asusd/asusctl profile reads, UPower `BAT1`, AMDGPU/NVIDIA DRM identities, NVIDIA
telemetry, ASUS CPU/GPU fan RPM channels and thermal/hwmon data. Session1, Hardware1,
hardwared, supergfxd and switcheroo-control were not active in the observed session.
These observations are read evidence only; they do not upgrade any capability from
`Detected` or `ReadOnly` to mutation `Supported`.

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

- #109 — aggregate `FeatureId::FanCurves` requires both CPU and GPU read contracts; single-fan failure suppresses the aggregate (source-complete; executable validation still pending);
- #116 — stored `FanCurveData.enabled` is carried end-to-end to UI (`fan-curve-enabled-known`/`enabled`) (source-complete; executable validation still pending);
- #104 — custom-write path now preserves authoritative `FanCurveData.enabled` in hardwared (read → pass-through → read-back confirms; source-complete, executable validation still pending);
- #105 — factory-reset profile restoration enforced at Orbis source level (backend never switches platform profile; any temporary switch stays in asusd) (source-complete, executable validation still pending).

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

No closed source blocker remains in this list. Fan mutation promotion is still
blocked by its independent #104/#105 safety/release gate; hosted Actions are
optional by project policy.

## Validation state

`scripts/verify-static` provides standard-library source contracts for UI, Automation, Display, backend completion, provider-timeout and documentation-status invariants. It is a fail-fast safety net only.

The local flake devShell provides a Rust/Cargo toolchain. On the current
`development` revision, locked Cargo checks, `python3 scripts/verify-static`
and the full `nix flake check` were executed green. This is revision-scoped
evidence; live hardware evidence remains separately recorded.

## Unwired research foundations

Policy/desired-state presets, reconciliation decisions, transaction phases, readiness helpers, software fan-policy computations and system-telemetry parsers are exported from crate APIs but have no runtime consumers. They remain FOUNDATION-only; no preset, policy or reconciliation path can dispatch hardware actions today.

A Draft PR #129 CI run previously failed before repository steps (`steps=null`); the PR has since been closed as superseded by `development`, which is now the single integration line. Hosted Actions are optional by project policy.
