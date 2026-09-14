# ASUS FA707NV privileged backend map

Status: read-only architecture and host availability audit, 2026-08-21.

This document records the state observed on the FA707NV host and the source
ownership model. It does not promote any capability and no hardware mutation
was attempted.

## Executive result

### Available

- The host system and user D-Bus transports are reachable.
- The installed polkit action file exists:
  `/etc/polkit-1/actions/io.github.orbiscontrol.hardware.policy`.
- Source contracts exist for `Session1` and `Hardware1`.
- Host read-only evidence exists for `platform_profile`, ASUS profile
  observation, UPower, DRM/sysfs GPU, hwmon, thermal and power-supply data;
  see [`asus-fa707nv-baseline.md`](asus-fa707nv-baseline.md).

### Unavailable

- `orbis-sessiond.service` is not installed in the current user systemd
  manager; it is `not-found`/`inactive`.
- `orbis-hardwared.service` is not installed in the current system systemd
  manager; it is `not-found`/`inactive`.
- `io.github.orbiscontrol.Session` is not owned and is not activatable.
- `io.github.orbiscontrol.Hardware` is not owned and is not activatable.
- Neither D-Bus service could be introspected because its name is unavailable.
- The installed system D-Bus policy file was not present at
  `/etc/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf`.

## Power-limit discovery matrix

The deterministic selection order is: (1) a typed ASUS backend with complete
current+metadata evidence; (2) no backend. `asus-nb-wmi` is inspected as a
fixed-path source, but its current-only PPT files are not enough to advertise a
field or enable writes. If both fixed ASUS backends become fully readable, the
selector must compare overlapping values and report a conflict instead of
silently choosing a writer. `asusd` is not an owner for these fields.

The authoritative owner for the target power/thermal fields is the Linux
kernel ASUS WMI/Armoury firmware-attributes ABI, not `xyz.ljones.Asusd`.
The fixed production paths are under
`/sys/class/firmware-attributes/asus-armoury/attributes`. `asusd` exposes the
same names over `xyz.ljones.AsusArmoury`, but on this host its `CurrentValue`
read returns `Could not read current value`; that is not a production contract.

| Field | Backend owner | Read | Metadata | Write | Read-back | Privilege | Evidence | Status |
|---|---|---|---|---|---|---|---|---|
| SPL / PPT PL1 | kernel ASUS Armoury `ppt_pl1_spl` | fixed sysfs `current_value` | fixed sysfs `min_value`, `max_value`, `scalar_increment`, `default_value` | typed Hardware1 candidate, disabled | fresh `current_value` | hardwared + polkit if later proven | kernel ABI documents ASUS WMI PPT; current host returns `ENODEV` for Armoury value/limits | candidate rejected; runtime unsupported on audited host |
| SPPT / PPT PL2 | kernel ASUS Armoury `ppt_pl2_sppt` | same | same | same | same | same | same | candidate rejected; runtime unsupported on audited host |
| FPPT / PPT PL3 | kernel ASUS Armoury `ppt_pl3_fppt` | same | same | same | same | same | same | candidate rejected; runtime unsupported on audited host |
| NVIDIA Dynamic Boost | kernel ASUS Armoury `nv_dynamic_boost` | fixed sysfs `current_value` | fixed sysfs metadata | typed Hardware1 candidate, disabled | fresh `current_value` | hardwared + polkit if later proven | kernel ABI documents `5..25` legacy range; current Armoury value/limits return `ENODEV` | candidate rejected; runtime unsupported on audited host |
| GPU temperature target | kernel ASUS Armoury `nv_temp_target` | fixed sysfs `current_value` | fixed sysfs metadata | typed Hardware1 candidate, disabled | fresh `current_value` | hardwared + polkit if later proven | kernel ABI documents `75..87` legacy range; current Armoury value/limits return `ENODEV` | candidate rejected; runtime unsupported on audited host |
| CPU temperature limit | no authoritative kernel ASUS Armoury field found | — | — | — | — | — | no kernel ABI field; no separate AMD authoritative interface found | Unsupported |
| CPU Boost | no authoritative read/write interface found | — | — | — | — | — | `cpufv` is write-only and does not prove a CPU boost toggle; `amd_pstate` boost is a generic policy switch, not an ASUS CPU boost contract | Unsupported |

### Host discovery evidence

| Candidate | Read-only evidence on FA707NV | Ownership/semantics | Decision |
|---|---|---|---|
| AMD `amd_pstate` / cpufreq | `amd_pstate/status=active`; `cpufreq/boost=1`; policy `boost` is present and root-owned | Generic CPU frequency policy, not package PPT/SPL/SPPT/FPPT metadata; cannot stand in for CPU Boost product semantics without a separate contract | Not selected for power limits or CPU Boost |
| powercap/RAPL | `/sys/class/powercap` exposes `intel-rapl` names on this host despite AMD CPU; no usable AMD package-limit owner | RAPL energy/powercap interfaces are not evidence for ASUS PPT fields; telemetry/accounting must not become a write backend | Rejected |
| hwmon / thermal / power-supply | Read-only sensors and AMDGPU `PPT` telemetry are present | Telemetry has no authoritative limit ownership or write/read-back contract | Rejected |
| AMD SMU / `ryzenadj` | Fixed candidate `/usr/bin/ryzenadj --info`; current host reports `Version: v0.19.0`, detects `ryzen_smu`, then fails `Unable to get os_access Obj` / `Unable to init ryzenadj` | Reference G-Helper Linux uses a bundled `ryzenadj -i` table plus elevated `sudo`/NOPASSWD batch setters and apply-on-start. That provides neither Orbis-authoritative min/max/step metadata nor an approved original-caller polkit/read-back contract; current host provides no current table values | Blocked-by-backend; read-only evidence parser only |
| NVIDIA NVML / `nvidia-smi` | RTX 4060 Laptop; current GPU ceiling limit 80 W, min 5 W, max 140 W | Driver GPU ceiling limit is distinct from ASUS `nv_dynamic_boost`; `nvidia-smi` output is telemetry/driver state, not proof of Dynamic Boost ownership | Rejected for target fields |
| `asusd` / `asusctl` Armoury | `asusctl armoury list` reports target Armoury fields `unavailable`; `xyz.ljones.AsusArmoury` is not activatable | Existing daemon does not provide a usable target-field owner; no fallback to old `asusd` wiring | Rejected |

No supported power-limit write backend was found on the current host. The
production selection therefore leaves power-limit writes disabled; candidate
names and the first kernel probe error are surfaced in capability diagnostics.

### SMU / RyzenAdj feasibility decision

The repository now records a bounded, read-only evidence contract:

- executable path: `/usr/bin/ryzenadj`;
- arguments: `--info` only;
- parser: version, detected kernel module, and known current-value table rows;
- no generic command/path API and no setter representation;
- `--info` metadata is explicitly not treated as authoritative min/max/step;
- no write promotion is possible without a separate typed Hardware1 operation,
  original-caller polkit authorization, validation against authoritative
  metadata, timeout/unknown handling, and fresh read-back.

The G-Helper Linux reference is useful feasibility evidence for the SMU tool,
but its elevated `sudo`/NOPASSWD helper and startup re-apply behavior are not a
safe Orbis privileged boundary. No privileged executable or ryzenadj write path
is added until that contract and its threat model are independently proven.

The provider advertises a field only after all required runtime reads succeed;
legacy `/sys/devices/platform/asus-nb-wmi/*` values without authoritative
metadata are not promoted and are never used as fake defaults.

### Current NixOS activation audit

The active configuration at `/home/artt/.nixos` imports only
`nixosModules.orbis-hardwared-policies` and sets:

```nix
services.orbis-hardwared-policies.enable = true;
```

It does not import `nixosModules.orbis-control`. Consequently the full
`services.orbis-control` option is not declared, neither generated systemd unit
is present, and the Orbis binaries are absent from the current system/profile
paths. This is an intentional policy-registration-only configuration, not a
failed daemon activation.

Required production step, to be performed explicitly by the system owner:

```nix
imports = [ inputs.orbis-control.nixosModules.orbis-control ];
services.orbis-control.enable = true;
```

No configuration was changed automatically during this validation.

### Reason

The host has no deployed/active Orbis sessiond or hardwared runtime. This is a
deployment/lifecycle availability failure, not evidence that the Rust
protocols or service implementations are absent. The repository contains a
NixOS-generated user unit for sessiond, a NixOS system unit and standalone
development unit for hardwared, and package-time D-Bus/polkit installation
steps; those artifacts are not installed on this host.

## Runtime connection path

The production GUI opens the user session bus and system bus in
`crates/orbis-ui/src/main.rs`, then passes both connections to
`build_production_runtime`:

```text
GUI
├─ session bus → Session1Proxy → io.github.orbiscontrol.Session
└─ system bus  → Hardware1Proxy → io.github.orbiscontrol.Hardware
```

`orbis-session-client` receives an already-open `zbus::Connection`; it does
not create buses or runtimes. Read providers use generated `Session1Proxy`
with no property cache. Mutation providers use generated `Hardware1Proxy` on
the system connection and preserve the original caller identity for
hardwared/polkit.

`orbis-sessiond` opens a system-bus connection for UPower/asusd/supergfxd
reads and a user session-bus connection for its own `Session1` service. Its
main production entry point is `run_discovered_sessiond`; startup and
reconnect lifecycle are supplied by the service manager.

## Session1 contract

| Feature | Backend owner | Current availability | Evidence |
|---|---|---|---|
| Battery charge limit | UPower + ASUS/effective-threshold reads inside sessiond | Unavailable on host | `Session1.ChargeLimit`; sessiond unit/name absent |
| Battery threshold evidence | sessiond source-labelled aggregation | Unavailable on host | `Session1.BatteryThresholdEvidence`; protocol DTO and validation exist |
| GPU runtime power | supergfxd read provider inside sessiond | Unavailable on host | `Session1.GpuPower`; baseline reports supergfxd inactive |
| Physical GPU MUX | ASUS Armoury firmware-attribute sysfs read inside sessiond | Unavailable through Orbis on host | `Session1.GpuMux`; host sysfs evidence is separate from Session1 liveness |
| dGPU access policy | ASUS Armoury firmware-attribute sysfs read inside sessiond | Unavailable through Orbis on host | `Session1.GpuAccess`; host sysfs evidence is separate from Session1 liveness |
| Performance profile | kernel `platform_profile` read inside sessiond | Unavailable through Orbis on host | `Session1.Performance`; host ABI read was confirmed separately |
| Fan curve read | asusd `FanCurveData(profile)` read inside sessiond | Unavailable on host | `Session1.FanCurve`; RPM evidence does not prove curve availability |

Session1 is getter/read-only by design. It has no mutation methods and must not
be replaced by a privileged deputy.

## Hardware1 contract

### Read/status operations

Hardware1 does not serve the normal observation model. Its read-only methods
are capability/status evidence for mutation backends:

- `PerformanceMutationStatus` — fresh read of kernel
  `platform_profile_choices`;
- `BatteryMutationStatus` — configured/effective backend preflight result;
- `FanMutationStatus` — typed fan backend evidence;
- `PanelMutationStatus`;
- `KeyboardBacklightMutationStatus`;
- `AuraMutationStatus`.

The status methods do not write hardware and do not require polkit
authorization. Normal read state remains a Session1/provider responsibility.

### Typed write operations (not enabled by this audit)

The exported typed interface contains:

- `SetPerformanceProfile` — kernel `platform_profile`, fresh choices check and
  authoritative read-back;
- `SetChargeLimit` — typed ASUS battery backend with read-back;
- `SetGpuMode` — typed supergfxd staged operation;
- `SetFanCurve` and `ResetFanCurvesToDefaults` — typed ASUS fan operations;
- `SetPanelOverdrive` — typed ASUS panel operation;
- `SetKeyboardBacklight` — typed LED/backlight operation;
- `SetAuraStaticRgb` — typed ASUS Aura operation with config-level `Accepted`
  semantics because the RGB state is write-only.

The production `hardwared` composition currently enables only the validated
Performance path and conditionally constructs the Battery path after
non-activating preflight. GPU, fan, panel, keyboard and Aura mutation
backends are disabled or fail closed. Their polkit actions exist in the
policy file but do not by themselves enable the operations.

## ASUS ownership map

```text
Feature                 → Backend owner                         → Current availability → Evidence
------------------------------------------------------------------------------------------------
Platform Profile        → kernel platform_profile / hardwared    → Host read available;  → baseline read;
                           Performance writer                    write not attempted      Hardware1 typed path

Battery threshold       → UPower + asusd + effective sysfs       → Host evidence only;   → baseline sources conflict;
                           sessiond read / hardwared write        Orbis services absent   no write attempted

GPU runtime power       → supergfxd via sessiond                  → Unavailable            → supergfxd inactive;
                                                                                         Session1 absent

GPU MUX/access          → ASUS Armoury firmware attributes        → Host reads available;  → sysfs evidence;
                           via sessiond                           Session1 unavailable    no Orbis service

Fan telemetry           → ASUS hwmon/sysfs                        → Host read available   → RPM evidence only

Fan curve              → asusd FanCurveData via Session1          → Unknown/unavailable   → Session1/hardwared absent;
                           and typed Hardware1 mutation             write blocked          RPM ≠ curve evidence

Panel Overdrive        → asusd panel backend + kernel read-back   → Not validated         → typed source exists;
                                                                                         production backend blocked

Keyboard brightness    → LED-class provider + Hardware1           → Not validated         → typed source exists;
                                                                                         production write blocked

Aura Static RGB        → asusd kbd_rgb_mode                       → Not validated         → typed source exists;
                                                                                         no hardware read-back
```

## Service and policy sources

- Session1 protocol: `crates/orbis-session-protocol/src/lib.rs`.
- Session client/runtime composition: `crates/orbis-session-client/src/lib.rs`,
  `crates/orbis-ui/src/main.rs`, and `crates/orbis-ui/src/composition.rs`.
- Session daemon: `crates/orbis-sessiond/src/{runtime,bootstrap,service}.rs`.
- Hardware1 service and typed operations: `crates/orbis-hardwared/src/{lib,main}.rs`.
- NixOS lifecycle: `packaging/nix/module.nix`.
- Standalone hardwared unit: `packaging/orbis-hardwared.service`.
- System D-Bus policy: `packaging/nix/dbus/io.github.orbiscontrol.Hardware.conf`.
- Polkit actions: `packaging/nix/polkit/io.github.orbiscontrol.hardware.policy`.

No unit, D-Bus policy, polkit rule, provider or backend was changed during
this audit. No real system/session D-Bus mutation call was made.
