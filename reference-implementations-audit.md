# Reference implementations audit

## Scope and method

This is a source-level research dossier, not an Orbis design decision record.
The observations below distinguish **SOURCE FACT** (directly visible in the
referenced source), **INFERENCE** (a bounded interpretation of that fact), and
**UNKNOWN** (not established by the inspected source).  Line numbers refer to
the pinned checkouts listed below and should be re-checked if a source revision
changes.

No hardware, system bus, service, reboot, logout, or mutation path was run.

## Pinned sources and licenses

| Project | Revision inspected | License evidence |
|---|---|---|
| asusctl/asusd + rog-platform + rog-profiles | tag `6.3.7`, `de4297a1c0490527f7b427db011bae4a35c7c832` | MPL-2.0, `LICENSE:1` |
| supergfxctl | tag `5.2.7`, `a86383e1b2f32d4f87f8dd47f0d6b06690877c64` | repository `LICENSE` |
| system76-power | `9a334723fbc96b993bcbe9ade1914224f8170bf7` | GPL-3.0-only, `LICENSE:1-3` |
| TUXEDO Control Center | `1a4d39ba5795e08e4e1ff6245ac908e2712262bf` | GPL notice in inspected files, e.g. `src/service-app/classes/FanControlHwmon.ts:4-17` |
| LACT | `fabf7e4e7e111fc57db9c19825d93a879eaf3d9f` | repository `LICENSE` |
| UPower | `78466b1690be12f11666abbd500cc49d85c15fa9` | license header in inspected C sources |
| Linux kernel sparse checkout | `3d6d817622b0a9721e3cc404df3469171582be13` | kernel `COPYING` / per-file SPDX metadata |

The license files are evidence for source provenance only; this document does
not perform a compatibility or redistribution analysis.

## State and mutation comparison

### ASUS ecosystem: asusd / rog-platform / rog-profiles

**SOURCE FACT.** `AsusPower` discovers `power_supply` devices through udev,
preferring batteries exposing `charge_control_end_threshold`, then `BAT<n>`,
then a last-resort `type=battery` match (`rog-platform/src/power.rs:22-32,
38-84`). Missing a battery does not make construction fail; an empty path is
returned so capability queries can report absence (`:96-113`). The threshold is
an attribute on the discovered battery (`:22-24`).

**SOURCE FACT.** Fan-curve configuration is persisted as `fan_curves.ron`
under the project config directory (`asusd/src/ctrl_fancurves.rs:22-45`). On
first setup it changes platform profile, reads defaults from the device, restores
the original profile, and writes the stored configuration (`:64-85`). A curve
mutation writes the device and then persists configuration (`:163-184`);
reset similarly changes profile, reads defaults, restores the active profile,
and persists (`:187-202`).

**SOURCE FACT.** Curve points are eight temperature/PWM pairs, with an explicit
enabled bit (`rog-profiles/src/fan_curve_set.rs:33-40`). Parsing rejects fewer
than eight points, percentages over 100, and decreasing temperature or PWM
values (`:68-130`). Device writes use hwmon-style `pwmX_auto_pointX_*`
attributes and select manual versus automatic mode (`:162-170`, continuation
through `:259`).

**INFERENCE.** Persistence and device mutation are separate operations in the
same call path. The inspected code does not establish rollback if the device
write succeeds but config persistence fails, or if profile restoration fails.

**UNKNOWN.** The inspected files do not establish the complete authorization
boundary or which concrete daemon unit owns each permission on every distro.

### supergfxctl

**SOURCE FACT.** The D-Bus interface exposes distinct `mode`, `supported`,
`power`, `set_mode`, `pending_mode`, and `pending_user_action` operations
(`src/zbus_iface.rs:15-90,131-160`). `set_mode` returns a
`UserActionRequired` value and emits action and graphics notifications
(`:131-150`). The proxy exposes the same pending state and signals
(`src/zbus_proxy.rs:63-94`).

**SOURCE FACT.** The mode/action vocabulary includes logout, reboot,
switch-to-integrated, ASUS eGPU disable, and nothing (`src/zbus_iface.rs:114-130`).
The ASUS implementation reads `gpu_mux_mode` and `dgpu_disable`; paths and
read/write helpers are explicit (`src/special_asus.rs:15-17,73-135`).

**SOURCE FACT.** A boot safety check can wait for a kernel attribute to appear,
detect inconsistent MUX and dGPU-disable state, attempt to re-enable a disabled
dGPU, and downgrade the effective mode when the requested mode is unsafe
(`src/special_asus.rs:220-301`). It explicitly says the returned mode may differ
from the requested mode and that the returned value must be used (`:223-224`).

**INFERENCE.** Requested state, effective state, pending state, and required
user action are intentionally different concepts. A successful request is not
equivalent to immediate hardware convergence.

**UNKNOWN.** This inspection does not prove whether all mutation authorization
is enforced by D-Bus policy, process privileges, or both.

### system76-power

**SOURCE FACT.** Threshold support is capability-gated by platform identity and
the existence of both sysfs attributes (`src/charge_thresholds.rs:14-24`). Reads
return `Unsupported`-style errors when unavailable (`:62-73`). Writes validate
0–100 and require `end > start` (`:76-83`). To avoid kernel ordering failure,
the implementation first writes end=`100`, then start, then end (`:85-92`).

**SOURCE FACT.** The daemon applies a profile under a lock and emits the active
profile change only after the operation succeeds (`src/daemon/mod.rs:167-229`).
Graphics mode, switchability, power, and external-display dGPU requirement are
separate D-Bus operations (`:231-289`).

**INFERENCE.** The threshold sequence is an example of an operation-specific
transaction protocol, not a generic “write both values” operation.

**UNKNOWN.** The inspected snippets do not establish rollback after an
intermediate sysfs write fails.

### TUXEDO Control Center

**SOURCE FACT.** Fan control discovers a named `tuxedo` hwmon path
(`src/service-app/classes/FanControlPwm.ts:24-36`) and tracks read/write
availability separately (`FanControlHwmon.ts:27-41`). Manual mode is represented
by PWM enable `1`, automatic mode by `2`; initialization only writes when both
the backend and requested write capability are available (`:61-95`).

**SOURCE FACT.** Fan and temperature inputs are enumerated and mapped by labels;
multiple CPU fans may share one temperature sensor (`FanControlHwmon.ts:103-180`).

**SOURCE FACT.** The user-session-facing service requests the system-bus name
`com.tuxedocomputers.tccd`, exports `/com/tuxedocomputers/tccd`, reports errors,
and unexports on exit (`TccDBusService.ts:26-39,55-89`).

**INFERENCE.** “Available” is not one boolean: sensor discovery, write
permission, and active control mode are separate states.

**UNKNOWN.** The inspected fan files do not establish the complete privilege
policy for sysfs writes.

### LACT

**SOURCE FACT.** The daemon uses a Unix socket, selecting `/run/lactd.sock` as
root and `/run/user/<uid>/lactd.sock` otherwise, with an environment override
(`lact-daemon/src/socket.rs:19-28`). It refuses to bind over an existing socket
and sets a restrictive umask before binding (`:31-58`). Optional admin user/group
ownership is applied from daemon configuration (`:61-87`).

**SOURCE FACT.** Requests and responses are newline-delimited JSON; malformed
requests become structured error responses (`lact-daemon/src/server.rs:131-164`).
The client reports disconnects and can reconnect unless configured otherwise
(`lact-client/src/lib.rs:42-78,85-109`).

**SOURCE FACT.** Fan settings include mode, static speed, temperature key,
interval, curve, spindown delay, change threshold, and automatic threshold
(`lact-schema/src/config.rs:186-227`).

**INFERENCE.** LACT separates transport lifecycle from request validation and
uses explicit reconnect state rather than treating a socket as permanently
available.

**UNKNOWN.** The inspected IPC layer does not by itself establish peer
authorization for every TCP or Unix-socket deployment.

### UPower

**SOURCE FACT.** UPower derives a PolicyKit subject from the D-Bus method sender
(`src/up-polkit.c:48-70`). One check permits user interaction and returns an
error for unauthorised callers (`:72-100`); another uses non-interactive checks
and treats either authorized or challenge as allowed (`:103-129`).

**SOURCE FACT.** Its self-test skips the PolicyKit test when no system bus is
present (`src/up-self-test.c:292-307`).

**INFERENCE.** Authorization behavior depends on the invocation context and the
chosen PolicyKit check flags; a unit test without the real system bus cannot
prove the live policy result.

### Linux kernel subsystems

**SOURCE FACT.** ACPI thermal helpers expose active, passive, hot, and critical
trip temperatures through distinct functions (`drivers/acpi/thermal_lib.c:90-164`).
The kernel also exposes vendor-specific hwmon implementations; for example,
the Uniwill driver advertises and reads temperature, fan, and PWM channels
(`drivers/platform/x86/uniwill/uniwill-acpi.c:1143-1302`).

**INFERENCE.** A frontend should not infer one universal thermal-control model
from a single vendor driver: sensor, trip-point, fan, and PWM capabilities can
come from different kernel interfaces.

**UNKNOWN.** This sparse checkout is not a hardware validation and does not
establish behavior of a particular ASUS laptop or firmware combination.

## Ownership and conflict observations

| Resource / concern | Evidence | What remains to establish |
|---|---|---|
| Battery threshold | ASUS discovers a battery attribute; system76-power uses fixed BAT0 paths (`rog-platform/src/power.rs:38-84`; `charge_thresholds.rs:8-24`) | Which provider is authoritative when multiple agents expose the same sysfs attribute |
| Fan PWM mode | ASUS writes curve attributes; TUXEDO toggles `_pwm_enable`; both use hwmon-shaped controls (`fan_curve_set.rs:162-170`; `FanControlHwmon.ts:61-95`) | Whether two daemons can safely coexist on a given machine |
| GPU MUX / dGPU disable | supergfxctl reads and writes ASUS platform attributes and performs safety reconciliation (`special_asus.rs:248-301`) | Ownership, locking, and recovery when another agent changes state |
| Profile state | asusd persists profile curves and reads active platform profile (`ctrl_fancurves.rs:61-92`) | Cross-daemon persistence format and stale-config behavior |
| IPC | D-Bus and Unix-socket examples expose state and mutation through separate services | Which boundary is authoritative for each Orbis capability |

These are conflict surfaces, not claims that a conflict exists on a particular
system.

## Failure and recovery patterns

1. **Unsupported capability:** system76-power checks both platform identity and
   attribute existence before reads/writes (`charge_thresholds.rs:14-24,62-65`).
2. **Invalid input:** threshold range/order and fan-curve monotonicity are
   rejected before mutation (`charge_thresholds.rs:76-83`; `fan_curve_set.rs:87-123`).
3. **Ordering-sensitive mutation:** system76-power raises the end threshold
   before changing the start threshold (`charge_thresholds.rs:85-90`).
4. **Divergence correction:** supergfxctl changes the effective mode when
   firmware attributes disagree with requested mode (`special_asus.rs:248-301`).
5. **Lifecycle failure:** TUXEDO logs D-Bus initialization, name-request, export,
   and unexport errors (`TccDBusService.ts:47-89`); LACT refuses stale socket
   ownership and reports malformed/disconnected IPC (`socket.rs:40-58`;
   `server.rs:141-164`).
6. **Not established:** none of the inspected excerpts proves complete rollback
   across a multi-step hardware mutation plus persistence update.

## Easy-to-miss details

- A device can be constructible while a capability is absent: asusd returns an
  empty battery path instead of failing construction (`rog-platform/src/power.rs:104-113`).
- Fan PWM values may be represented as raw `0..255` or percentages converted to
  raw values (`fan_curve_set.rs:68-114`).
- Fan defaults may only be readable while their profile is active, forcing
  temporary profile changes (`ctrl_fancurves.rs:68-83`).
- A requested GPU mode may be replaced by a safer effective mode and may require
  logout/reboot (`supergfxctl/src/zbus_iface.rs:114-150`).
- Sysfs paths and capability checks are backend-specific; system76-power’s fixed
  BAT0 paths are not equivalent to udev discovery (`charge_thresholds.rs:8-24`;
  `rog-platform/src/power.rs:38-84`).
- Socket presence alone is not enough: LACT rejects an existing socket and
  separately configures ownership (`socket.rs:40-87`).
- UPower’s interactive and non-interactive PolicyKit checks intentionally differ
  (`up-polkit.c:72-129`).

## Cross-project matrices

### Capability / state model

| Project | Capability probe | Authoritative read | Mutation result / pending state | Persistence |
|---|---|---|---|---|
| asusd | platform profile + fan node; udev power discovery | platform/sysfs reads | D-Bus result; no complete pending model in inspected file | RON fan-curves file |
| supergfxctl | supported modes and ASUS attribute reads | effective mode/power | explicit action + pending mode/action + signals | base config |
| system76-power | platform identity + both threshold files | sysfs threshold pair | D-Bus error or success | profile/config outside excerpt |
| TUXEDO | hwmon files and write availability | mapped sensors/PWM | mode writes when writable | not established in fan excerpt |
| LACT | socket/config/device handler | daemon response | structured response + reconnect/disconnect | serialized profile/config structures |
| UPower | daemon/backend/device availability | D-Bus daemon state | PolicyKit-gated method outcome | not established here |

### Authorization / privilege boundary

| Project | Boundary visible in evidence | Gap |
|---|---|---|
| asusd | daemon-side D-Bus service and direct platform/sysfs code | exact policy not inspected |
| supergfxctl | daemon D-Bus interface and platform mutation | exact policy not inspected |
| system76-power | daemon D-Bus interface and sysfs writes | exact policy not inspected |
| TUXEDO | system-bus daemon plus write-availability flag | exact polkit/service policy not inspected |
| LACT | Unix-socket path, umask, optional owner/group | TCP peer authorization not established |
| UPower | sender-derived PolicyKit subject and action ID | distro policy files not included |

## Questions for architectural review

1. Which component is the authoritative owner for each mutable sysfs or D-Bus
   resource when multiple compatible tools are installed?
2. Which capability states must distinguish unsupported, read-only, temporarily
   unavailable, pending, effective, and failed?
3. For multi-step writes, what is the safe ordering and what can be verified
   after each step?
4. What should happen when persistence succeeds but hardware application fails,
   or hardware application succeeds but persistence fails?
5. Which mutations require explicit user action, and how should logout/reboot
   requirements remain visible after the initiating request returns?
6. Which IPC calls need interactive authorization versus non-interactive denial?
7. How are stale sockets, crashed daemons, and another tool changing the same
   resource detected and surfaced?
8. Which vendor-specific paths are facts discovered at runtime, rather than
   assumptions encoded in the UI or domain layer?

## Limits

This dossier records inspected source evidence only. It does not claim complete
coverage of any project, distro policy, firmware behavior, kernel ABI, or live
hardware behavior. No Orbis API, provider, UI, configuration, or architecture
was changed as a consequence of this research.
