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
