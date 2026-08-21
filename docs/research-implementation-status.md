# Research mechanics implementation status

Updated: 2026-08-20

This document tracks implementation of `docs/research-derived-mechanics.md`.
It distinguishes domain/read-only foundations from production runtime wiring and
from hardware-gated work. A checked foundation does not imply live ASUS
validation or that a user-facing control has been enabled.

## Implemented foundations

### Runtime/evidence primitives

- [x] Canonical `bounded_provider_call` based on `Provider::timeout()`.
- [x] Timeout maps to `ProviderError::Timeout` and never retries automatically.
- [x] Structured readiness model with independent dependency and permission
  evidence.
- [x] Bounded read-only readiness probe helper and explicit concurrent join
  primitive.
- [x] Read-only ownership conflict detector; no service is stopped or modified.
- [x] Hardware validation evidence state machine with read-only-first gates.

### Transaction and reconciliation model

- [x] Typed mutation transaction phases: Prepared, AwaitingObservation,
  AwaitingConfirmation, PendingRequirement, Applied, RolledBack, Failed and
  UnknownOutcome.
- [x] `Applied`/`Accepted` provider results still require authoritative
  observation before the transaction is proven Applied.
- [x] No automatic retry after a mutation attempt has been dispatched.
- [x] Pure Desired/Observed/Pending reconciliation decision engine.
- [x] Startup does not dispatch writes merely because configuration was loaded.
- [x] Resume is two-phase: observe first, then a `ResumeObserved` reconciliation
  pass may converge a proven mismatch.
- [x] Pending/unknown transaction state suppresses duplicate mutation dispatch.
- [x] Explicit restoration-debt model for temporary profile/mode switching.
- [x] Pending transition registry re-observes lifecycle candidates but never
  auto-resolves reboot/logout requirements.
- [x] Last-mutation audit model distinguishes Accepted, Applied, Pending,
  RolledBack, Failed and UnknownOutcome.

### Presets and automation

- [x] Global preset intent model; selecting/loading a preset is inert.
- [x] AC/battery/low-power USB-C preset policy with Unknown power source
  remaining unselected.
- [x] Policy automation engine for power source, process, GameMode, resume and
  external-display events.
- [x] Automation selects a preset/Desired policy only; it cannot execute an
  arbitrary command or bypass reconciliation.
- [x] Deterministic priority/tie handling.
- [x] Versioned preset JSON import/export with duplicate-id/schema validation.
- [x] Import remains configuration-only and performs no hardware mutation.

### Telemetry and diagnostics

- [x] Bounded in-memory history ring with explicit capacity.
- [x] CSV history export with escaping.
- [x] Typed telemetry export rows with Fresh/Stale/Partial/Unknown evidence.
- [x] Integer min/max/mean statistics without floating point.
- [x] Alerts for temperature, stopped-hot fan, stale telemetry and persistent
  Desired/Observed divergence; alerts do not remediate by themselves.
- [x] Diagnostic control-flow graph for Trigger -> Preset -> Desired ->
  Capability -> Reconciliation -> Mutation -> Observation.
- [x] Read-only Linux RAM/swap parser/provider.
- [x] Read-only memory PSI parser using integer basis points.
- [x] Read-only zram discovery/stat parsing and zswap enabled-state read.

### Advanced fan-policy foundations

These are pure computations only. They are not connected to fan hardware and do
not change the current firmware-managed/fail-closed fan mutation policy.

- [x] EMA smoothing.
- [x] Temperature hysteresis/deadband.
- [x] Raw-PWM rise/fall rate limiter.
- [x] Curve interpolation.
- [x] Maximum/minimum/average/weighted virtual temperatures.
- [x] Temperature delta and offset derived sensors.
- [x] Composed software fan-policy evaluator.

## Partially implemented / runtime wiring still required

### P0.1 Provider timeout enforcement — PARTIAL

The canonical timeout primitive exists, but existing application/probe/status
paths do not all use it yet. Issue #123 remains open until the production paths
are wrapped without losing mutation `UnknownOutcome` semantics.

Do not fix mutation timeouts by blindly converting them into a definite
`CommandError::Command(Timeout)`: after dispatch, hardware outcome may be
unknown and requires authoritative follow-up observation.

### P0.2 Structured readiness report — PARTIAL

Domain model and bounded probe helpers exist. Production composition still needs
an allowlisted readiness report over actual sessiond/Hardware1/asusd/supergfxd
sources and UI/diagnostics presentation.

### P0.3 Canonical capability refresh — NOT YET WIRED

Issue #112 is still valid: periodic refresh re-queries mutation status first,
while explicit `RefreshCapabilities` currently does not. Both paths need one
canonical refresh function before #112 can close.

### P0.4 Transaction coordinator — FOUNDATION ONLY

The transaction state machine exists, but no new risky production control is
wired through a coordinator yet. The next integration must perform:

1. authoritative pre-read;
2. one typed mutation attempt;
3. authoritative read-back;
4. confirmation/requirement handling;
5. proven rollback where backend restoration semantics are established.

### P1 reconciliation — FOUNDATION ONLY

Decision/scheduling primitives exist. There is not yet a production executor
that owns Desired state for each currently supported feature and runs the full
read/compare/gate/mutate/read-back loop.

### Telemetry history/export — FOUNDATION ONLY

Data structures/export functions exist. The current UI worker does not yet own
history buffers or export actions, and telemetry completeness semantics tracked
by #117 still gate user-facing freshness claims.

### System telemetry — PROVIDER AVAILABLE, NOT COMPOSED

The Linux memory provider is read-only and uses fixed kernel paths, but is not
yet included in the production UI/session telemetry composition.

### Presets/automation — DOMAIN READY, NOT EXECUTING

Preset and policy engines intentionally do not execute hardware changes. They
need persistence/UI integration and a real reconciliation executor before they
become product controls.

## Intentionally blocked / not enabled

- [ ] Fan-curve writes/default reset remain fail-closed under the existing fan
  safety blockers. Pure fan-policy code does not change this.
- [ ] New GPU/MUX/product-policy mutations remain disabled until mapping,
  ownership, lifecycle and read-back semantics are proven.
- [ ] New power-limit/OC/undervolt/adaptive-TDP writes are not added.
- [ ] Predictive `dT/dt` fan control is still R&D and not connected to hardware.
- [ ] Quick-control/advanced-page UI integration awaits runtime state contracts.
- [ ] Hotkeys/tray/overlay await a stable command/reconciliation boundary.
- [ ] Plugin API is deferred until core/protocol contracts stabilize; plugins
  must never widen Hardware1 into arbitrary privileged execution.
- [ ] Broad lighting ecosystem integration remains future work; no OpenRGB
  GPL-2.0-only implementation code is copied into Orbis.

## Verification status

The implementation in this feature branch was produced through the GitHub
repository API. No local checkout is available in the current execution
environment and repository CI is currently blocked by the infrastructure issue
tracked in #106. Therefore Cargo/rustfmt/clippy/test results must remain
**UNVERIFIED** until executable checks run.

Before merge, at minimum run the repository INTEGRATION tier from `AGENTS.md`:

```text
cargo fmt --all
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Do not use absence of a GitHub check run as proof of success.
