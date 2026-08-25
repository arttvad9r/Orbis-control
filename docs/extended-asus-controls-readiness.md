# Extended ASUS Controls — Current Readiness

> Роль: current source/evidence readiness for extended ASUS controls.
> Реализованный typed backend **не равен** production-enabled mutation.
> Operational truth: [`current-state.md`](current-state.md).
> Обновлено: 2026-08-19.

## Current policy

Production `orbis-hardwared` currently enables live mutation backends only for
historically validated Performance and conditional Battery. Raw GPU, Fan,
Panel Overdrive, Keyboard Backlight and Aura Static RGB remain in typed code/ABI
where useful, but production composition returns `Unsupported` for those write
paths and packaged polkit denies them by default. The root service exposes only
`/sys/firmware/acpi/platform_profile` as a direct writable sysfs path.

This distinction is intentional: source implementation can be useful for tests,
research and future validation without being a shipped writable feature.

## Source/evidence matrix

| Control | Current source state | Production readiness | Next acceptable step |
|---|---|---|---|
| Boot sound | no dedicated domain/provider/probe | `UNKNOWN / NOT DESIGNED` | research authoritative read contract first |
| MCU powersave | no dedicated domain/provider/probe | `UNKNOWN / NOT DESIGNED` | establish ABI/ownership/lifecycle before UI |
| Panel HD | no dedicated concept; distinct from Panel Overdrive | `UNKNOWN / NOT DESIGNED` | define exact concept and read evidence |
| eGPU control | no dedicated eGPU domain/provider | `UNKNOWN / NOT DESIGNED` | model independently from MUX/access/power |
| AniMe Matrix | capability/domain vocabulary exists; no proven production provider | `PRODUCTION BLOCKED` | read-only provider/evidence first |
| Slash Lighting | capability vocabulary exists; no dedicated production provider | `PRODUCTION BLOCKED` | typed read model + probe after ABI research |
| Generic Aura effects | typed read/provider work exists | `PARTIAL / READ-ORIENTED` | expand one proven mode/effect at a time |
| Aura Static RGB write | typed Hardware1/backend code exists; result semantics are config-level `Accepted` | **HARD-BLOCKED** | prove owner/interface/lifecycle + packaged/live evidence, then deliberate re-enable |
| Keyboard backlight read | typed read state/provider exists | `IMPLEMENTED SOURCE` | validate packaged/runtime evidence |
| Keyboard backlight write | typed Hardware1/sysfs backend is production-wired; exact ASUS LED paths are sandboxed and FA707NV `3→0→3` live-validated | **LIVE-VALIDATED / FA707NV** | keep capability-specific probe/read-back; evidence is model/revision scoped |
| Panel Overdrive read | typed read provider/probe exists | `IMPLEMENTED SOURCE` | validate runtime/device evidence |
| Panel Overdrive write | typed asusd backend is production-wired; kernel current-value read-back and FA707NV `1→0→1` live evidence complete | **LIVE-VALIDATED / FA707NV** | keep capability-specific owner/read-back; evidence is model/revision scoped |
| MiniLED | typed read-only provider/probe exists | `READ ONLY` | expose only with runtime evidence; write needs separate proof |
| Screen Auto Brightness | typed read-only provider/probe exists | `READ ONLY` | same: read first, mutation separately proven |

## Rules for every new or re-enabled extended control

1. **Concept first.** Understand upstream/kernel/asusd semantics before product UI.
2. **Read before write.** Add typed read/evidence before mutation.
3. **Probe, not model table.** DMI is support-matrix evidence, not a runtime switch.
4. **Preserve unknown values.** Never coerce future backend values to nearest known state.
5. **Keep concepts separate.** Panel HD ≠ Overdrive; eGPU ≠ MUX/access/power; keyboard brightness ≠ Aura effect.
6. **No generic privileged writer.** Hardware1 methods stay narrow and semantic.
7. **Product enablement is separate from backend existence.** A code-present backend remains disabled until evidence and policy explicitly enable it.
8. **Authoritative success.** Use `Applied` only after the defined confirmation; otherwise retain `Accepted`/Pending/error truthfully.
9. **Lifecycle before automation.** Reconciliation ownership must be explicit before automatic writes.
10. **No cross-model defaults/ranges.** Values from another ASUS model are evidence, never production defaults.
11. **Privacy-safe diagnostics.** No serial/UUID/arbitrary firmware dump collection.
12. **Executable evidence required.** Source review alone cannot promote a control to packaged/live support.

## Current write blockers

- Fan: #104/#105/#109/#116; production writer is code-disabled.
- Battery dynamic owner/status refresh: #107/#112; startup probing is fail-closed and non-activating.
- Effective capability/product-policy truth: #120.
- Provider operation timeout enforcement: #123.
- Executable CI/release validation: #106.

## AniMe / Slash / new firmware controls

The presence of legacy traits or capability identifiers is insufficient. New
controls need an evidence record covering exact owner/API, value semantics,
read failure classification, persistence over reboot/resume/service restart,
write constraints, confirmation semantics, and model/revision-specific live
validation where applicable.

Absence on the FA707NV reference system does not prove global ASUS absence; the
inverse is also true: presence on another model does not justify enabling a
control here.

## Evidence classification

This document is source/evidence readiness only. It performs no live hardware
writes and does not elevate any blocked extended mutation to packaged or
live-validated support.
