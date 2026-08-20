# Automation backend design

Status: implementation foundation in progress. Policy persistence and pure planning/preflight exist; runtime execution does not.

## Invariant

A saved policy is intent, not proof that automation is active. The production path is deliberately split into five stages:

1. **Policy persistence** — hardened, user-owned `desired-state.toml`.
2. **Pure planning** — convert one explicit lifecycle event + observed power source into typed `orbis_core::automation::AutomationAction` values.
3. **Capability preflight** — evaluate the entire immutable plan against one immutable `DeviceCapabilities` snapshot.
4. **Executor** — future application-layer component; not implemented yet.
5. **Authoritative read-back** — future executor must re-read every changed feature before reporting success.

No earlier stage may claim the semantics of a later stage.

## Persistence

`orbis_config::automation_policy::OrbisDesiredState` is the current typed payload of the hardened generic desired-state store.

Defaults are hardware-inert:

- automation disabled;
- Performance/GPU/Display/Lighting all `KeepCurrent`;
- resume and AC-change triggers disabled;
- `reconcile_only=true` only affects future execution behavior and does not cause writes itself.

Invalid/malformed desired-state source is preserved. The UI persistence bridge refuses editable state instead of overwriting a preserved source.

`AutomationWindow.backend-ready` means only that typed load/save/read-back works. `AutomationWindow.runtime-ready` remains separate and currently stays `false`; therefore the Enable control remains disabled even after persistence is available.

## Pure planner

`AutomationPolicy::plan_for(trigger, power_source)` performs no I/O.

Supported policy triggers in the first design:

- `OnAc` when `on_ac_change=true` and observed source is AC;
- `OnBattery` when `on_ac_change=true` and observed source is Battery;
- `OnResume` when `on_resume=true`, selecting the policy from the supplied observed power source.

Other `AutomationTrigger` variants are not silently mapped to one of these policies.

The planner returns blockers instead of guessing when:

- policy enable intent is false;
- the matching trigger is disabled;
- AC/Battery transition contradicts the supplied observed source;
- the trigger is not configured by this policy model;
- a requested lighting level cannot be represented exactly by the current core action model.

Planner output is still not executable evidence.

## Capability preflight

`preflight_automation_plan(plan, capabilities)` is also pure and fail-closed.

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

## Known gaps before executor work

### Lifecycle ownership

A production owner is still required for AC/Battery and resume events. It must define ordering/debounce/coalescing semantics and must not infer a transition from stale telemetry.

### Automation capability

`FeatureId::Automation` must remain non-writable until the executor and lifecycle ownership are both production-wired. Individual Performance/GPU support alone is insufficient.

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

- consume one frozen policy/plan/capability generation;
- re-check staleness before the first mutation;
- serialize the batch;
- perform no partial batch when preflight failed;
- use typed existing mutation owners only;
- do not route around provider/polkit policy;
- treat unknown mutation outcome as failure, not success;
- read back every mutated feature;
- publish success only from authoritative read-back;
- cancel/supersede stale lifecycle plans explicitly;
- keep fan-curve mutation outside the first automation scope while fan safety blocks remain open.
