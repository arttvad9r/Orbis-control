# Orbis Control extended ASUS controls readiness audit

Scope: backlog item #85. Read-only source/evidence matrix for extended ASUS controls. No hardware reads or writes are performed by this document.

## Verdict

The current hardening tree is intentionally incomplete for extended ASUS controls. Existing typed work on Panel Overdrive, MiniLED, Screen Auto Brightness, keyboard backlight and Aura should be reused, but boot sound, MCU powersave, panel HD, eGPU and Slash must not be implemented through generic firmware-attribute writes or model tables.

## Source/evidence matrix

| Control | Current Orbis source state | Readiness | Safe next step |
|---|---|---|---|
| Boot sound | no dedicated domain type, provider trait, probe or production runtime path found | `UNKNOWN / NOT DESIGNED` | research authoritative ASUS/kernel/asusd read contract first; define typed state before UI |
| MCU powersave | no dedicated domain/provider/probe found | `UNKNOWN / NOT DESIGNED` | establish exact ABI, lifecycle semantics and ownership; read-only probe first |
| Panel HD / panel-specific HD mode | no dedicated concept/provider found; do not conflate with Panel Overdrive or Wayland refresh | `UNKNOWN / NOT DESIGNED` | identify exact firmware concept and allowed values; keep independent from OD/refresh |
| eGPU mode/control | no dedicated eGPU domain/provider found; current GPU architecture models runtime power, physical MUX and dGPU access independently | `UNKNOWN / NOT DESIGNED` | define eGPU identity/state independently; never map it onto product GPU mode by guess |
| AniMe Matrix | `FeatureId::Anime` and a generic `AnimeProvider` trait exist; mock-era API shape exists, but no production provider/evidence was found in the audited tree | `DOMAIN PARTIAL / PRODUCTION BLOCKED` | source research + read-only capability provider; preserve unavailable/unsupported distinction |
| Slash Lighting | `FeatureId::Slash` exists, but no dedicated provider trait or production implementation was found | `DOMAIN STUB / PRODUCTION BLOCKED` | define typed read state and probe only after authoritative ABI research |
| Generic Aura modes | typed Aura read provider exists; Static RGB has a narrow Hardware1 mutation path with `Accepted` config-level semantics | `PARTIAL` | expand only one proven mode/effect contract at a time; preserve unknown raw enum values |
| Keyboard backlight | typed read state and Hardware1 brightness mutation already exist | `BACKEND READY` | UI integration/lifecycle work only; do not fold into generic percent lighting API |
| Panel Overdrive | typed read provider/probe and narrow Hardware1 mutation already exist | `BACKEND READY` | UI wiring with capability gating and authoritative read-back |
| MiniLED | typed read-only provider/probe exists; no production mutation backend | `READ ONLY` | expose only if runtime composition/evidence is present; do not infer write from root-writable attr |
| Screen Auto Brightness | typed read-only provider/probe exists; no production mutation backend | `READ ONLY` | same: read-only exposure first, mutation requires separate proof |

## Required rules for every new extended control

1. **Concept first.** Add a domain concept only after the upstream/kernel/asusd semantic is understood. Do not expose raw firmware integer attributes as product controls.
2. **Read before write.** Production work starts with a read-only source/provider and typed error mapping. Mutation is a separate backlog item.
3. **Probe, not model table.** Runtime support comes from current ABI/service evidence. DMI model names are support-matrix evidence, not the feature switch.
4. **Preserve unknown values.** Future enum/integer values must remain `Unknown`/raw-preserved where the domain allows it, never coerced to the nearest known value.
5. **Separate concepts.** Panel HD is not Panel Overdrive; eGPU is not physical MUX, dGPU access or product GPU mode; keyboard brightness is not generic Aura brightness.
6. **No generic privileged writer.** A future mutation gets the smallest typed Hardware1 method and polkit action needed for that concept.
7. **Authoritative success.** A backend setter returning OK is insufficient where a read-back exists. Use `Applied` only when the defined authoritative observation confirms the request; otherwise use an honest non-applied result/error contract.
8. **Lifecycle/ownership before automation.** Controls whose state is reset by firmware/service restart/suspend need explicit reconciliation ownership before automation can manage them.
9. **No cross-model ranges/defaults.** Values from another ASUS model are test/evidence data, never production defaults.
10. **Privacy-safe diagnostics.** New probes may expose capability/status but must not add serial/UUID/arbitrary firmware dump collection.

## AniMe-specific gap

The legacy `AnimeProvider` shape (`available`, enabled/state operations) is not enough to claim production readiness. Before production use it needs:

- exact backend owner (kernel/asusd) and versioned interface evidence;
- device presence/read semantics;
- typed mode/content constraints rather than generic payload assumptions;
- independent read/write capability evidence;
- failure classification and lifecycle behavior;
- model/live validation for devices that actually contain AniMe hardware.

Absence on one reference laptop is not evidence that the feature is globally unsupported.

## Slash-specific gap

`FeatureId::Slash` is only a capability vocabulary entry in the audited tree. There is no typed `SlashProvider`, state model, probe, constraints or production ownership contract. The next acceptable step is research/read-only domain design, not a write path.

## Boot sound / MCU powersave / panel HD / eGPU gap

No dedicated source concept was found for these names in the audited hardening tree. This means UI controls or firmware attributes with similar labels must not be wired opportunistically. Each needs an upstream evidence record covering:

- exact API/attribute/service and value meaning;
- read availability and error semantics;
- whether the setting is model/firmware/version dependent;
- whether state persists over reboot/resume/service restart;
- ownership when asusd/kernel/supergfxd overlap;
- write constraints and authoritative read-back if mutation is later considered.

## Relationship to existing extended controls

The already implemented Panel Overdrive, keyboard backlight and Aura paths are the template: concept-specific types, narrow providers, explicit probes/status, and narrow privileged mutations. MiniLED and Screen Auto Brightness demonstrate the correct behavior when only read evidence exists: remain read-only instead of inventing mutation support.

## Evidence classification

This is source-level readiness evidence. It does not establish packaged or live-hardware support for any extended control and performs ZERO live hardware writes.
