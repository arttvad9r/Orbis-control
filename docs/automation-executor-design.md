# Automation executor design

Status: execution semantics are under development; production execution remains disabled.

## Purpose

This document covers the layers after Automation lifecycle observation and capability preflight. It does not change the core rule that a saved policy is intent, not evidence that automation is active.

The current path is:

1. hardened desired-state persistence;
2. AC/Battery or logind resume observation;
3. pure policy planning;
4. immutable capability preflight;
5. execution candidate revalidation;
6. single-slot serialization admission;
7. first-version execution-scope validation;
8. Performance executor boundary under the worker/capability-generation owner;
9. future production promotion after the release gate;
10. authoritative read-back before any success publication.

Only stages 1–8 exist today. The executor boundary is compiled privately in
production, but execution promotion remains disabled and no unattended mutation
path is reachable.

## Revalidation boundary

`AutomationExecutionCandidate` can be created only from `ReadyButExecutionDisabled`. `revalidate_automation_candidate()` rebuilds the plan from current persisted policy and requires:

- the same capability generation;
- fresh capability metadata;
- byte-equivalent typed plan semantics after rebuilding;
- a second complete capability preflight.

A successful result is `AutomationExecutionHandoff`. Its fields are private and it exposes no execution method.

## Serialization admission

`AutomationSerializationCoordinator` models the ownership rules the real executor must preserve.

- admission consumes an `AutomationExecutionHandoff`;
- capability generation is compared again immediately before slot ownership;
- only one `AutomationDryRunLease` may exist at a time;
- a second batch is rejected as `Busy` rather than partially interleaved;
- lease identifiers are monotonic and sequence exhaustion fails closed;
- `AutomationDryRunLease` is not `Clone`/`Copy` and cannot be constructed externally;
- releasing the slot consumes the exact lease object.

The coordinator is hardware-inert. It has no provider, worker, D-Bus, sysfs or process execution API.

This is intentionally not yet sufficient for production execution: the future worker integration must serialize capability-generation replacement and the mutation/read-back sequence under the same owner. Passing a generation number by value cannot by itself eliminate a TOCTOU race.

## First executor scope

`prepare_automation_execution_scope()` defines the initial production scope conservatively:

- an empty action set becomes an explicit `NoOp`;
- exactly one `SetProfile(PerformanceProfile)` becomes `Performance(profile)`;
- a duplicate Performance mutation is a contract error;
- any GPU, Display, Lighting or Custom action blocks the entire batch.

There is deliberately no API that returns a permitted subset. A policy asking for Performance plus an unsupported Display/GPU/Lighting action must not silently apply only Performance.

### Why Performance first

Performance is the only candidate in the current UI/application stack with all of these existing properties:

- typed operation-level capability evidence;
- an existing production mutation owner;
- application-layer mutation through `PerformanceServiceRuntime::set_performance`;
- mandatory authoritative read-back inside `AppService`;
- existing worker serialization for normal interactive Performance commands.

GPU product mutation remains product/policy disabled. Display refresh is observation-only. Lighting still lacks an unambiguous typed owner for Automation intent. Fan writes remain outside Automation scope while fan safety blocks are open.

## Performance executor boundary

`automation_performance_executor.rs` is privately included by the UI crate.
The compile-time promotion gate remains false, so this does not authorize
unattended mutation.

Its purpose is to develop mutation-result semantics without making Automation reachable in production. The proof uses the existing `PerformanceServiceRuntime` contract and requires:

1. prepared batch metadata matches the active serialization lease;
2. capability generation still matches immediately before the owner call;
3. the prepared kind is Performance or NoOp;
4. the application command succeeds;
5. `ApplyResult` is exactly `Applied`;
6. authoritative `PerformanceState.current` exactly equals the requested profile.

The following are never reported as Automation success:

- `ApplyResult::Accepted`;
- any `Pending` result;
- `Failed` or `RolledBack`;
- successful command result with mismatching read-back;
- provider command failure;
- mutation followed by read-back failure.

`CommandError::Command` and `CommandError::ReadBack` remain distinct. The second case is especially important: the mutation may have occurred, so the future production executor must treat state as unknown and require authoritative reconciliation before another unattended mutation.

## Promotion gate to production

The Performance executor must remain promotion-disabled until all of the following are true:

1. Rust 1.87+ workspace `cargo check --locked --workspace --all-targets` passes;
2. `cargo test --locked --workspace` passes, including executor failure-path tests;
3. clippy passes with `-D warnings`;
4. the executor is moved under the same sequential owner as capability registry replacement, eliminating generation TOCTOU between final check and mutation;
5. an in-flight/unknown-outcome state blocks subsequent Automation work until authoritative read-back/reconciliation completes;
6. cancellation/coalescing semantics for newer lifecycle events are explicit;
7. `FeatureId::Automation` production capability is assembled from actual lifecycle/executor readiness instead of synthetic fixtures;
8. `AutomationWindow.runtime-ready` is set from that evidence, not from persistence availability;
9. no unsupported action can enter the executor through a partial-batch path;
10. the first production rollout remains Performance-only.

Until this promotion gate is satisfied, `FeatureId::Automation.write` must remain unsupported/unknown and `AutomationWindow.runtime-ready` must remain `false`.

## Later scopes

### GPU

Requires an explicit product-policy change, effective write evidence and unattended-safe requirement semantics. `SupportedWithRequirement` is insufficient for background automation.

### Display

Requires a typed mutation owner, deterministic target selection and authoritative compositor read-back. Current `wl_output` observation is not a mutation API.

### Lighting

Requires replacing the ambiguous `SetLighting(bool)` action contract with a concrete owner/level model that can distinguish keyboard backlight from Aura and represent Dim/Normal exactly.

### Notifications

`notify_transitions` is persisted intent only. A notification host integration must be designed independently and must not influence mutation success semantics.
