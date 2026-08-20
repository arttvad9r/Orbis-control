# Automation backend design

Status: AC/Battery and resume shadow observation plus executor handoff guards are implemented; hardware execution is not.

## Invariant

A saved policy is intent, not proof that automation is active. The production design is deliberately split into seven stages:

1. **Policy persistence** — hardened, user-owned `desired-state.toml`.
2. **Lifecycle observation** — authoritative power-source telemetry and paired logind resume signals with explicit freshness/coalescing semantics.
3. **Pure planning** — convert one confirmed lifecycle event + observed power source into typed `orbis_core::automation::AutomationAction` values.
4. **Capability preflight** — evaluate the entire immutable plan against one immutable capability generation.
5. **Execution revalidation** — rebuild the plan from current persisted policy and re-check generation/freshness/preflight immediately before any future executor handoff.
6. **Executor** — future serialized application-layer component; not implemented yet.
7. **Authoritative read-back** — future executor must re-read every changed feature before reporting success.

No earlier stage may claim the semantics of a later stage.

## Persistence

`orbis_config::automation_policy::OrbisDesiredState` is the current typed payload of the hardened generic desired-state store.

Defaults are hardware-inert:

- automation disabled;
- Performance/GPU/Display/Lighting all `KeepCurrent`;
- resume and AC-change triggers disabled;
- `reconcile_only=true` only affects future execution behavior and does not cause writes itself.

`load_automation_policy()` is the shared hardened read boundary used by both the Automation window and shadow runtime. Missing state means the inert typed default. A malformed/invalid existing desired-state source is preserved and rejected rather than silently treated as editable default state.

The UI keeps unsaved draft state separate from the last successfully loaded/saved persisted policy. Shadow observation sees only persisted policy; changing a ComboBox or toggle without Save cannot alter runtime planning.

`AutomationWindow.backend-ready` means only that typed load/save/read-back works. `AutomationWindow.runtime-ready` remains separate and currently stays `false`; therefore the Enable control remains disabled even when persistence and shadow observation are available.

## Lifecycle observation

### AC / Battery

`PowerSourceEdgeDetector` owns the typed AC/Battery transition contract:

- first fresh sample establishes a baseline and emits no transition;
- `None`, stale or future-dated telemetry cannot create a transition;
- a source change requires two consecutive fresh samples before emitting `OnAc` or `OnBattery`;
- `rebaseline()` changes the stable source without emitting an event and is reserved for a separately proven lifecycle handoff;
- observation itself has no mutation API.

### Resume

`ResumeTelemetryGate` owns the pure pairing contract for logind-style sleep/resume observations:

- `PrepareForSleep(true)` arms one suspend cycle and invalidates an older unresolved resume;
- `PrepareForSleep(false)` is accepted only after this process saw the matching `true` signal;
- startup or an unpaired `false` never synthesizes `OnResume`;
- after the paired `false`, the gate waits for a new telemetry sample timestamped at or after that resume observation;
- the sample must be fresh and contain an authoritative AC/Battery value;
- unknown, stale, future-dated or pre-resume samples cannot create `OnResume`;
- the pending resume expires after a bounded wait rather than accepting late state.

The production transport is `resume_observer.rs`. It opens a system-bus connection and subscribes only to `org.freedesktop.login1.Manager.PrepareForSleep`. It acquires no inhibitor and invokes no login1 mutation method. zbus internally installs/removes the normal signal match rule required for subscription. The observer uses zbus' own public `ordered_stream` re-export rather than adding a new direct futures dependency, so the existing lockfile remains authoritative. Signals are marshalled back to the Slint event-loop thread before touching thread-local Automation state.

On the matched `PrepareForSleep(false)` observation the bridge requests a fresh bounded `SysfsTelemetryProvider` read. The resume signal by itself does not plan anything. `AutomationShadowRuntime::observe_resume_telemetry()` independently re-checks sample freshness and AC/Battery presence before selecting the AC or Battery resume policy branch.

### Resume / power-source coalescing

A source can change while the machine is asleep. Without an explicit rule the first post-resume Battery sample could satisfy `OnResume`, then the next poll could emit a redundant `OnBattery` for the same physical change.

The shadow lifecycle now handles this deterministically:

- if persisted Automation is enabled **and** `on_resume=true`, the accepted fresh post-resume source becomes the `PowerSourceEdgeDetector` baseline after resume validation;
- the same sleeping source change therefore cannot immediately reappear as `OnAc`/`OnBattery`;
- if `on_resume=false`, no rebaseline occurs, so `on_ac_change=true` retains the opportunity to detect a source change that happened during sleep;
- future genuinely distinct source changes after wake still require the normal two-sample debounce.

This is lifecycle coalescing only. It does not execute or mark a plan applied.

### Shadow result boundary

`AutomationShadowRuntime` consumes typed lifecycle evidence, persisted policy and one immutable `CapabilityRegistrySnapshot`. It verifies capability freshness before planning/preflight and returns only:

- ordinary AC/Battery observation state;
- a freshness/evidence blocker;
- a preflight blocker; or
- `ReadyButExecutionDisabled`.

A ready shadow result is deliberately not an executor command.

### Current UI telemetry bridge

The existing `main.rs` calls the Quick Controls refresh hook for every `WorkerEvent::TelemetryRefresh`. Until the large entrypoint is safely refactored to pass the original telemetry object directly, that hook performs a separate bounded read-only `SysfsTelemetryProvider` snapshot for Automation shadow observation.

This compatibility bridge:

- uses the same production typed sysfs provider as normal telemetry;
- keeps the provider-generated timestamp and freshness semantics;
- has a two-second timeout and an overlap guard;
- obtains the current whole-swap capability generation from the Diagnostics runtime, which is already updated by `RegistryChange`;
- performs no setter/provider mutation;
- is intentionally independent from the slower 10-second visual Display/Keyboard refresh cadence;
- is also reused for the immediate post-resume read, so resume planning never relies on UI strings or a cached pre-suspend power source.

The extra sysfs read is temporary duplication, not a new hardware owner. Once `main.rs` can be changed with executable validation available, the preferred final wiring is to feed the original successful worker telemetry snapshot directly into the shadow runtime and remove the duplicate read.

## Pure planner

`AutomationPolicy::plan_for(trigger, power_source)` performs no I/O.

Supported policy triggers in the first design:

- `OnAc` when `on_ac_change=true` and observed source is AC;
- `OnBattery` when `on_ac_change=true` and observed source is Battery;
- `OnResume` when `on_resume=true`, selecting the policy from the fresh post-resume observed power source.

Other `AutomationTrigger` variants are not silently mapped to one of these policies.

The planner returns blockers instead of guessing when:

- policy enable intent is false;
- the matching trigger is disabled;
- AC/Battery transition contradicts the supplied observed source;
- the trigger is not configured by this policy model;
- a requested lighting level cannot be represented exactly by the current core action model.

Planner output is still not executable evidence.

## Capability preflight

`preflight_automation_plan(plan, capabilities)` is pure and fail-closed.

The entire batch remains blocked unless:

- `FeatureId::Automation` has operation-level **write = Supported**;
- every action feature has operation-level **write = Supported**;
- no action is `SupportedWithRequirement` — unattended automation must not implicitly accept reboot/logout/confirmation requirements;
- Performance targets are present in `CapabilityConstraints::PerformanceProfiles`;
- GPU targets are present in `CapabilityConstraints::GpuModes`;
- a future typed DisplayRefresh target-evidence model exists for requested refresh policy;
- the lighting action identifies a concrete owner instead of the current ambiguous `SetLighting(bool)` contract;
- no `CustomCommand` is present.

`AutomationPreflight::actions_if_ready()` returns no actions when any blocker exists. There is intentionally no helper for obtaining a partially permitted subset.

The current production registry does not expose Automation write support, so real shadow transitions remain blocked before executor handoff. Synthetic unit fixtures may expose write support solely to test later guard stages.

## Execution revalidation guard

`automation_execution_guard` is the next hardware-inert boundary.

`AutomationExecutionCandidate` can be created only from `ReadyButExecutionDisabled`. `revalidate_automation_candidate()` then requires all of the following again:

- current capability generation equals the candidate generation;
- current snapshot timestamp is not in the future;
- current snapshot age is below the supplied execution-age ceiling;
- rebuilding from the current persisted policy produces exactly the same plan;
- a second complete capability preflight succeeds.

Any mismatch blocks the whole batch. A successful result is an `AutomationExecutionHandoff` with private fields, the exact ordered action slice, and the required capability generation. It deliberately exposes no execute/apply/provider/worker function.

The future serialized executor must still compare `required_generation()` with the generation it owns at the instant immediately before the first mutation. The handoff is therefore a checked input, not lasting authorization.

## Known gaps before executor work

### Automation capability

`FeatureId::Automation` must remain non-writable until the executor, lifecycle ownership, serialization semantics and read-back semantics are production-wired and executable tests pass. Individual Performance/GPU support alone is insufficient.

### Lifecycle serialization

The sleeping power-source duplication case is now coalesced in shadow mode. A future executor must still serialize genuinely distinct lifecycle events that occur close together after wake and explicitly supersede stale handoff candidates rather than racing or partially interleaving them.

### Display refresh

The current Wayland output provider is observation-only. A product-level DisplayRefresh mutation owner still needs:

- unambiguous target selection;
- typed supported target evidence;
- compositor-specific mutation implementation;
- authoritative compositor read-back.

Until then Display actions fail preflight even if a generic write status were accidentally exposed.

### Lighting

The existing core `AutomationAction::SetLighting(bool)` does not identify KeyboardBacklight versus Aura and cannot represent the UI's Dim/Normal levels exactly. Automation must not choose an owner by convention. The action contract needs to become typed before lighting execution is implemented.

### GPU

Current production GPU mutation policy remains disabled. The planner can represent product GPU intent, but preflight must reject it until operation-level write evidence is actually Supported. `SupportedWithRequirement` is not accepted for unattended automation.

### Notifications

`notify_transitions` is persisted intent only. No notification host integration is currently implied or enabled.

## Executor requirements

When executor development begins, it must preserve these invariants:

- run under one serialized owner together with capability-generation changes;
- consume only a revalidated `AutomationExecutionHandoff`;
- compare the handoff generation immediately before the first mutation;
- preserve the established resume/power-source coalescing rule;
- serialize and explicitly supersede genuinely distinct lifecycle events;
- perform no partial batch when planning/preflight/revalidation failed;
- use typed existing mutation owners only;
- do not route around provider/polkit policy;
- treat unknown mutation outcome as failure, not success;
- read back every mutated feature;
- publish success only from authoritative read-back;
- keep fan-curve mutation outside the first automation scope while fan safety blocks remain open.

## Local validation status

The repository requires Rust >= 1.87 and currently resolves Slint 1.13.1. The sandbox can execute native build tools, but it currently contains neither Rust/Cargo nor Slint tooling and outbound network access is blocked, so Rust/Slint semantic compilation has not yet run here.

`check-automation-shadow-contract.py` provides an additional stdlib-only fail-closed check in this environment. Its positive fixture passes, and mutation tests correctly fail when a hardware setter is introduced, `runtime-ready=true` is published, the resume gate is removed, the `policy.enabled && policy.on_resume` coalescing condition is weakened, or a redundant direct `futures-util` dependency is added. It is also registered in `scripts/verify` for normal toolchain-equipped runs. These static checks are not substitutes for `cargo check`, `cargo test`, clippy and Slint validation; those remain mandatory before enabling execution.
