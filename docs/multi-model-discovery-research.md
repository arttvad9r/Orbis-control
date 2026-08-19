# Orbis Control multi-model discovery research

Scope: backlog item #83. This document defines how ASUS laptop differences should be discovered without introducing runtime model-name support tables.

## Decision

Runtime support remains **capability/probe driven**. DMI model identity is diagnostics/evidence metadata, not the primary switch that enables a feature.

A model string may identify a support-matrix evidence record, but production code must not infer `Supported` merely because `product_name` matches a known laptop.

## Why

ASUS feature availability can vary independently by kernel ABI, firmware/BIOS, asusd/supergfxd version, service presence, permissions and current session environment. A model table collapses those independently changing facts into one stale boolean and cannot preserve `BackendMissing`, `TemporarilyUnavailable`, `PermissionDenied`, `ReadOnly` and `Unknown`.

The current Orbis capability layer already has the better primitive: typed read probes plus separate mutation-backend evidence.

## Required discovery source by capability

| Capability | Runtime evidence | Model string role |
|---|---|---|
| Performance profiles | fresh current/available provider reads; typed Hardware1 mutation status for write | none |
| Battery charge threshold | authoritative configured/effective read and bounds; typed Hardware1 mutation status | none |
| GPU runtime power | supergfxd/provider read | none |
| Physical GPU MUX | concrete Armoury/sysfs provider read | none |
| dGPU access policy | concrete Armoury/sysfs provider read | none |
| Product GPU mode | only a separately proven product-policy mapping; never infer from MUX/access/power tuple or model name | evidence label only |
| Fan curves | concrete fan provider read; separate Hardware1 mutation status | none |
| Panel Overdrive | concrete provider/read-back ABI; separate mutation status | none |
| MiniLED | concrete read provider | none |
| Screen Auto Brightness | concrete read provider | none |
| Keyboard backlight | actual LED/provider state and max level; separate mutation status | none |
| Aura | asusd Aura read state/supported modes; separate typed Static RGB mutation status | none |
| Wayland display outputs | compositor/output provider observation | none |
| Power limits | concrete attribute plus proven units/ranges/default semantics; an attribute name alone is insufficient | may reference external evidence, never enable runtime control |
| AniMe / Slash / other extended ASUS controls | concrete typed provider/probe only after semantics are defined | support-matrix evidence only |

## Probe rules

1. Probe the narrowest authoritative interface for the feature being reported.
2. Keep read and write evidence independent. A readable or root-writable file does not prove an Orbis mutation backend.
3. Preserve failure classes instead of reducing them to a device support boolean.
4. Never perform a mutation merely to discover capability support.
5. Do not derive product GPU modes from primitive GPU state.
6. Do not synthesize safe ranges/defaults from another ASUS model.
7. A service being present/running is backend evidence, not feature support by itself.
8. Cache only as lifecycle policy permits; `BackendRecovered`/`CapabilityChanged` should trigger a new probe rather than a model-table lookup.

## Device identity boundary

The privacy-safe diagnostics identity (`vendor`, `product`, `board`, BIOS version/date) can be used to attach a human-readable evidence record or support-matrix row after collection. It must not become a generic `match product_name { ... }` feature-enable mechanism.

A narrowly scoped DMI exception is acceptable only when the underlying ABI itself is explicitly model/DMI gated and Orbis has no more direct observable evidence. Such an exception requires:

- a documented upstream/kernel/firmware contract;
- exact affected models/revisions;
- a typed capability result rather than a broad device profile;
- tests showing unknown models fail closed;
- no guessed values/ranges.

No such new exception is introduced by this research slice.

## Support matrix relationship

The support matrix is documentation/acceptance evidence. It can say that a capability was `LIVE-VALIDATED` on a particular model/revision, but runtime still probes the current machine. A prior positive matrix row cannot override a current `Unsupported`, `BackendMissing`, `PermissionDenied` or `Unknown` probe result.

Likewise, absence of a matrix row means `UNKNOWN` evidence, not runtime `Unsupported`.

## Current architecture fit

The existing production probes for Performance, Charge Limit, fan curves, GPU power/MUX/access, Panel Overdrive, MiniLED, Screen Auto Brightness and display output already follow this direction: provider observations establish read semantics, while mutation support is either separate typed backend evidence or deliberately `ReadOnly`.

Diagnostics hardware identity is separately collected and therefore does not need to contaminate the capability registry with model-name logic.

## Follow-up constraints

- Item #84 Power-limit readiness must remain blocked until per-field units/ranges/default semantics are proven.
- Item #85 extended ASUS controls should build an evidence/provider matrix first, not a model-to-feature switch table.
- Any future multi-model fixtures belong in test/evidence data and must not become production support truth unless they represent an explicit upstream ABI contract.

This is source/architecture research only. No runtime table, provider, probe or hardware behavior is changed.
