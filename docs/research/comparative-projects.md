# Comparative project research

This document records the read-only comparative research used to shape Orbis Control. It is an architecture and product reference, not a list of code/assets to copy.

## Scope

The reviewed projects were `asusctl/asusd`, `supergfxctl`, LACT, G-Linux, G-Helper Linux, Ayuz and `rog-control-center`. The comparison focused on ownership, capability discovery, read/write separation, pending/reboot semantics, GPU and power controls, and user-facing feature coverage.

External implementation code or assets are not imported without a separate license and ownership review.

## Decisions already adopted by Orbis

### Typed ownership and narrow privilege

Use a capability-specific typed owner for privileged machine mutations. The GUI remains unprivileged; `Hardware1` owns machine-level writes and `sessiond` owns user-session behavior. Do not replace this boundary with a generic root, sysfs or shell proxy.

### Capability evidence before mutation

Treat read and write support independently. A control is writable only when the current runtime exposes an authoritative owner and operation-specific capability evidence. Unsupported, unavailable, permission-denied and unknown states remain distinct and visible.

### Requested, observed and pending state

Keep the requested value separate from authoritative observed state. Represent reboot/logout requirements and queued GPU changes as pending rather than claiming immediate application.

### Read-back and conflict detection

A successful request is not automatically an applied state. Apply flows must validate capability, dispatch through the typed owner, read back authoritative state, and surface mismatch or unknown outcomes. External managers such as `power-profiles-daemon` or `supergfxd` must be treated as owners, not bypassed.

### Transaction safety

Riskier power and GPU operations should follow:

```text
snapshot → validate → apply → authoritative read-back → restore/rollback when required
```

Partial completion, failed read-back and ambiguous timeout are explicit outcomes. Do not blindly retry an uncertain hardware write.

### Thin clients and stable schemas

Keep daemon, CLI and UI clients thin and preserve stable typed schemas across the layers. Fake backends and private P2P/hardware simulators should cover success, unsupported, permission, timeout, partial and unknown paths.

## Project-to-Orbis mapping

| Reference | Useful idea | Orbis decision/status |
|---|---|---|
| `asusctl/asusd` | ASUS-specific backend ownership and firmware/kernel capability handling | Adopted as the typed Hardware1/backend boundary; no direct code import |
| `supergfxctl` | staged GPU/MUX state and reboot-required semantics | Adopted in GPU pending/queued modeling and owner-conflict handling |
| LACT | read-only GPU power/thermal evidence before risky writes | Read-only NVIDIA evidence exists; mutation remains disabled without a safe owner |
| G-Linux | Linux laptop power and thermal feature inventory | Used as feature research, not as a backend dependency |
| G-Helper Linux | ASUS-oriented feature expectations and UX coverage | Used to check product scope; unsupported controls remain truthful |
| Ayuz | alternative ASUS control-surface coverage | Used for comparative feature discovery only |
| `rog-control-center` | broad ASUS control grouping and Advanced Apply shape | Influenced grouping and staged apply; no assets or code copied |

## Current implementation status

Already implemented or validated in Orbis:

- typed `Hardware1` privileged boundary;
- truthful capability/read/write states;
- authoritative read-back for supported daily controls;
- staged Advanced Apply for boot sound, iGPU memory and ASPM;
- session-owned automatic clamshell behavior;
- pending/reboot semantics for applicable GPU/iGPU operations;
- fail-closed ownership handling for `power-profiles-daemon`;
- read-only NVIDIA GPU power/thermal and amd-pstate EPP/boost evidence;
- fake/P2P tests for success and failure paths.

Not enabled on the current target:

- EPP/boost mutation;
- CPU TDP/package-power mutation;
- NVIDIA power-limit mutation;
- status LED, Modern Standby networking, hibernation timeout, active-core and M1–M5 mutation;
- any generic sysfs/root fallback.

These remain disabled until a real typed owner and authoritative read-back contract are established. The next implementation slice should only be selected when new runtime evidence identifies such an owner.

## CPUFreq ownership finding — 2026-09-09

The target exposes the official Linux CPUFreq `amd-pstate-epp` ABI. EPP is read from and, at the kernel ABI level, writable through each CPUFreq policy's `energy_performance_preference`; boost is a separate global CPUFreq control at `devices/system/cpu/cpufreq/boost`. The diagnostics provider uses the canonical global boost path and keeps the surface read-only.

This ABI presence does **not** prove that Orbis owns the mutation. On the target, `power-profiles-daemon` is active and advertises `CpuDriver: amd_pstate`, so direct Orbis EPP writes would compete with the external power-policy owner. There is no authoritative generic ownership signal for boost that would make a direct write safe while that stack is active. Therefore EPP and boost mutation remain disabled. A future implementation must either delegate through a supported owner API or establish an explicit transaction/ownership hand-off, inspect every CPUFreq policy, read back all affected state, and treat timeout or mismatch as unknown.

## power-profiles-daemon delegation boundary — 2026-09-09

The live `org.freedesktop.UPower.PowerProfiles` contract exposes `ActiveProfile`, `Profiles`, `ActiveProfileHolds`, `HoldProfile` and `ReleaseProfile`. It provides delegation for the daemon's supported whole-system profiles (`power-saver`, `balanced`, `performance`), not an arbitrary EPP or boost setter. Orbis already uses the typed `PowerProfilesDaemonClient` for those profile semantics and confirms `ActiveProfile` read-back.

Do not extend that client with invented EPP/boost methods. `HoldProfile` is suitable for a temporary supported profile intent, but it is not an owner API for an individual EPP preference or the global boost switch. CPUFreq EPP/boost therefore remain read-only until an actual owner API or explicit ownership hand-off exists.

## charge_mode mapping boundary — 2026-09-09

The legacy `asus-wmi` ABI documents charger-source values `1` (barrel), `2` (USB-C), and `3` (both, barrel used). The newer `asus-armoury` firmware-attributes driver exposes a read-only `charge_mode` enumeration with raw possible values `0;1;2`, but its public driver contract does not assign semantic labels to those new indices. The target exposes the newer path and currently reads raw value `1`.

Orbis must not silently apply the legacy `1/2/3` labels to the newer `0/1/2` path. Until the newer owner publishes a verified mapping or a device-specific evidence source establishes it, charge mode remains either raw diagnostic evidence or absent; it is not a labeled UI control.

## License and reuse rule

Comparative research may guide interfaces, state models and UX review. Before reusing code, assets or protocol assumptions from any reference project, verify its license, ownership boundary and compatibility with Orbis' fail-closed mutation model.
