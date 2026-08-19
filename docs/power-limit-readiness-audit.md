# Orbis Control power-limit readiness audit

Scope: backlog item #84. Source-level readiness audit only. No power-limit read or write is executed.

## Verdict

Power-limit production support is **BLOCKED**. The repository has useful domain/trait scaffolding, but it does not yet have enough authoritative metadata or a production provider/mutation boundary to expose these controls safely.

## What already exists

The domain layer contains:

- typed `PowerLimitField` values for SPL, SPPT, FPPT, CPU temperature limit, NVIDIA Dynamic Boost, GPU temperature target and an explicit `Other(String)` escape hatch;
- typed units (`Watts`, `DegreesC`, `Percent`, `Count`, `Unknown`);
- `PowerLimitValue` with current/min/max/step/default validation;
- `PowerLimits` keyed by field;
- capability IDs for `PptPl1Spl`, `PptPl2Sppt`, `PptFppt`, `NvDynamicBoost`, `NvTempTarget` and `CpuBoost`;
- `CapabilityConstraints::PowerLimits`, which can carry typed ranges without conflating them with observed current state;
- a generic `PowerLimitProvider` trait with read, set, restore-defaults and validation methods.

This is a domain contract, not production support evidence.

## Missing production pieces

### 1. No production PowerLimitProvider

In the audited provider tree, `PowerLimitProvider` is implemented by `MockProvider`; no production provider implementation was found. Therefore there is no authoritative production read source for current/min/max/step/default/unit metadata.

### 2. No Hardware1 power-limit mutation contract

Current Hardware1 exposes typed mutations for Performance, Battery threshold, raw GPU mode ABI, fan curves/defaults, Panel Overdrive, keyboard brightness and Aura Static RGB. It does not expose a typed power-limit mutation/status method.

Consequently there is also no per-capability polkit action/read-back contract for power limits.

### 3. Field presence is insufficient evidence

A sysfs/firmware attribute name by itself does not establish:

- unit;
- scaling;
- writable range;
- step;
- default;
- whether a value is profile-specific;
- whether changes are persistent, temporary or firmware-clamped;
- whether a write requires another mode/profile to be active;
- authoritative read-back semantics.

A raw integer must never be mapped into `PowerLimitValue` by guessed defaults.

### 4. FA707NV evidence remains incomplete

Existing project notes say FA707NV exposes names such as `ppt_pl1_spl`, `ppt_pl2_sppt`, `ppt_pl3_fppt`, `nv_dynamic_boost` and `nv_temp_target`, while metadata fields were empty because the model was not covered by the relevant kernel DMI metadata table. Those dated notes are evidence of attribute presence, not proven units/ranges/defaults or safe mutation semantics.

Under the release evidence taxonomy this is insufficient for production control and must not be upgraded to `LIVE-VALIDATED` mutation support without a complete provenance/test record.

## Required implementation order

1. **Read-only evidence adapter.** Define a concrete ASUS/kernel source that can return each field only when current value, unit and safe constraints are authoritative. Missing metadata must return `Unknown`/`Unsupported` as appropriate, never synthetic ranges.
2. **Per-field capability probe.** Keep the existing feature IDs independent. Do not expose one aggregate writable `PowerLimits` switch if only some fields are proven.
3. **Read-only UI first.** Display only fields with typed evidence and label unknown constraints honestly.
4. **Mutation design separately.** For each writable field define ownership, authorization, validation, one write path and authoritative read-back semantics.
5. **Hardware1/polkit only after evidence.** Add the minimum typed privileged method/action per proven mutation family; never a generic sysfs writer.
6. **No Restore Defaults until defaults are authoritative.** `restore_defaults()` must not manufacture defaults from another model or hardcoded UI values.
7. **Live validation per field/model/revision.** Record exact kernel/firmware/backend versions and read/write/read-back evidence separately.

## Minimum evidence for one production field

A field can move beyond `BLOCKED/UNKNOWN` only when the evidence record identifies:

- exact backend attribute/API and owner;
- value unit and any scaling;
- current/min/max/step and source of each;
- default source, or explicit absence of a default;
- read behavior on unsupported/permission/backend failure;
- mutation availability independently from read availability;
- validation rule before write;
- authoritative post-write observation;
- rollback/failure behavior if applicable;
- lifecycle behavior across profile change, suspend/resume and reboot where relevant.

## UI consequence

Any current visual power-limit sliders or numeric controls must remain preview/disabled. Percent-like visual ranges must not be relabeled as watts/degrees or enabled simply because similarly named firmware attributes exist.

## Relationship to multi-model discovery

Do not solve missing metadata with `match product_name`. If an upstream kernel ABI is itself DMI-gated, that fact may be documented as evidence, but Orbis should still consume authoritative per-field metadata whenever available and fail closed when it is absent.

## Evidence classification

This document is source-level readiness evidence only. It establishes why production power-limit support is currently blocked; it provides no packaged or live-hardware validation.
