# Orbis Control — NixOS Deployment Remediation Plan

**Plan date:** 2026-08-18  
**Source audit:** PR #6, `agent/nixos-deployment-audit`, commit `4f19f5c0c835e33f9a1885209625f5a32ca03d6c`  
**Primary production source:** `chatgpt/production-hardening-20260818`  
**Audited hardening SHA:** `a745f559e1dc3aeefc665b31f740242bc4cb7348`  
**Latest hardening SHA re-checked immediately before this documentation commit:** `a745f559e1dc3aeefc665b31f740242bc4cb7348`  
**`main` SHA used to create this branch:** `b9bfdf5bb08fecea58589bcd204215b49384d9ef`  
**Scope:** research and remediation design only; no production code, Nix, Rust, systemd, Slint, CI, or hardware changes  
**Hardware writes performed:** none

## Executive recommendation

The three blockers from PR #6 should become four small implementation commits because the keyboard-backlight blocker contains two independent defects: deployment access and capability honesty.

1. **Sessiond UPower resilience:** make battery/UPower discovery lazy and capability-local. `orbis-sessiond` must acquire Session1 and keep non-battery capabilities alive even when UPower is absent, late, restarted, or exposes no supported battery. On NixOS, enable UPower by default as an integration convenience, but do not make the whole session daemon depend on UPower lifecycle.
2. **PolicyKit lifecycle:** when the full Orbis NixOS module is enabled, enable the system PolicyKit authority with `security.polkit.enable = true`. Do not depend on a particular graphical authentication agent.
3. **Keyboard sandbox:** retain the current strong hardwared sandbox and add one optional `ReadWritePaths=` exception for exactly `/sys/class/leds/asus::kbd_backlight/brightness`. Do not make `/sys` generally writable and do not add device-cgroup permissions for this sysfs attribute.
4. **Keyboard capability honesty:** replace unconditional `Supported` with a fresh read-only structural probe of the LED ABI. The probe must never write brightness and must not claim that a future write is guaranteed to succeed.

After all four fixes and the no-write automated tests in this plan pass, Orbis is ready to begin the first **full packaged NixOS acceptance test** on FA707NV. That later acceptance test is the place to prove real authorized mutations on hardware; this research task performs none.

---

# 1. UPower / `orbis-sessiond`

## 1.1 Current root cause

The current dependency is created by application composition, not by systemd ordering.

The production lifecycle is:

`orbis-sessiond` main
→ `runtime::run_discovered_sessiond()`
→ `bootstrap::connect_discovered_upower_session_server()`
→ open system bus
→ `discovery::discover_battery()`
→ only after successful battery discovery create the Session1 server
→ own `io.github.orbiscontrol.Session`
→ wait for SIGINT/SIGTERM.

Important source locations:

- `crates/orbis-sessiond/src/runtime.rs::run_discovered_sessiond`
- `crates/orbis-sessiond/src/bootstrap.rs::connect_discovered_upower_session_server`
- `crates/orbis-sessiond/src/discovery.rs::discover_battery`
- `crates/orbis-sessiond/src/composition.rs::build_upower_session_server`
- `crates/orbis-sessiond/src/composition.rs::build_upower_session_server_with_effective_source`
- `crates/orbis-sessiond/src/upower.rs::ZbusUPowerChargeLimitSource`
- `crates/orbis-sessiond/src/upower.rs::AsusdBatteryChargeLimitProvider`
- `crates/orbis-sessiond/src/server.rs::build_session_server`
- `crates/orbis-sessiond/src/service.rs::SessionService`

`discover_battery()` calls the UPower root service `org.freedesktop.UPower`, enumerates devices, and requires exactly one object with `Type=Battery` and `PowerSupply=true`. A D-Bus failure is returned as a provider D-Bus error. Zero matching batteries or more than one matching battery are returned as `Unsupported`. Because bootstrap uses `?`, every one of those results aborts Session1 startup.

The deeper design issue is that battery discovery currently happens before Session1 exists even though GPU and Performance providers are separately composable. UPower is therefore an accidental whole-daemon startup dependency.

## 1.2 Is UPower architecturally required for all of Session1?

**No.** UPower is required by the current Battery read composition, not by the Session1 transport, GPU MUX/access reads, kernel Performance reads, or the privileged Hardware1 boundary.

`SessionService` currently receives a mandatory `Arc<dyn BatteryProvider>`, while GPU providers and Performance are optional. That does not require battery *hardware* to be mandatory: the mandatory object can instead be an always-constructible provider whose reads perform lazy discovery and report capability-local errors.

This is preferable to converting all of SessionService to optional battery state. It preserves the existing Session1 property surface and moves backend availability into the provider layer, where it belongs.

## 1.3 Exact current behavior by failure case

| Scenario | Current behavior | Why |
|---|---|---|
| UPower is not installed/registered | `discover_battery()` cannot call `org.freedesktop.UPower`; bootstrap fails; `orbis-sessiond` exits | discovery is startup-critical |
| UPower is configured but not yet running | UPower is designed for system-bus activation when an application calls it; if activation succeeds, discovery continues. If the name is still unavailable/fails, sessiond exits | Orbis waits on a synchronous startup call, not a capability-local probe |
| UPower is running but exposes no system battery | `discover_battery()` returns `Unsupported`; sessiond exits | zero candidates is currently a bootstrap error |
| UPower exposes multiple system batteries | `discover_battery()` returns `Unsupported`; sessiond exits | selection semantics are intentionally not invented |
| UPower restarts after sessiond startup | the system-bus connection itself can remain alive and calls still target the well-known UPower name, but Orbis stores the battery object path/native path discovered only once. Recovery is only incidental if the restarted daemon recreates a compatible object at the same path; there is no explicit re-discovery | fixed startup discovery and cached object path/native path |
| UPower appears after sessiond already failed | systemd user `Restart=on-failure` can eventually restart sessiond, but the whole daemon churns rather than only Battery becoming available | lifecycle recovery is delegated to process restart |

UPower upstream documents that `upowerd` is normally automatically started by D-Bus when a caller invokes `org.freedesktop.UPower`. This makes explicit `After=upower.service` unnecessary for the preferred design; application resilience is still required because service activation does not solve no-battery, crash/restart, or backend errors.

## 1.4 Approach A — NixOS module enables UPower

Proposed NixOS-side behavior:

- under `services.orbis-control.enable`, set `services.upower.enable = lib.mkDefault true`;
- do **not** add `Requires=upower.service` or bind the user service lifecycle to the system service;
- allow an explicit administrator override to `false` because Battery is optional after the application fix.

The pinned nixpkgs UPower module shows that `services.upower.enable` installs/integrates the package through `environment.systemPackages`, `services.dbus.packages`, `services.udev.packages`, and `systemd.packages`. Orbis should use that native integration rather than reproducing UPower units or D-Bus files.

### Advantages

- a normal Orbis NixOS configuration gets the expected battery backend automatically;
- no custom UPower deployment logic is needed;
- matches how full desktop environments commonly enable UPower.

### Limitations

This is **not sufficient alone**. It still does not make these application states correct:

- a laptop/desktop with no matching battery;
- UPower temporarily unavailable;
- UPower restart with a changed object path;
- a deliberate administrator choice not to run UPower;
- multiple battery devices that Orbis does not yet know how to select.

If Approach A is the only fix, an optional capability remains a whole-daemon failure boundary.

## 1.5 Approach B — resilient/lazy Battery provider

### Preferred architecture

Make UPower discovery lazy and retryable inside the Battery provider path.

Preferred shape:

1. `orbis-sessiond` opens the system bus and session bus without requiring UPower discovery to succeed.
2. Bootstrap constructs an always-available production Battery provider object that owns/clones the system-bus connection but does **not** own a permanently discovered battery path.
3. Each Battery read, or each read after invalidation, performs `discover_battery()`.
4. After discovery, build/use the existing typed pieces for that observation:
   - `ZbusUPowerChargeLimitSource`
   - `ZbusAsusdConfiguredSource`
   - `SysfsBatteryEndThresholdSource`
   - `AsusdBatteryChargeLimitProvider` semantics.
5. Return provider errors only from the Battery property; do not tear down Session1.
6. A later read retries discovery, so UPower appearance/restart and battery object recreation can recover without restarting `orbis-sessiond`.

### Why lazy re-discovery is preferred over a permanent startup cache

It has the smallest correctness surface for the first remediation:

- no `NameOwnerChanged` subscription is required;
- no stale object-path invalidation cache is required;
- service restart recovery is naturally retried;
- no hardware writes are introduced.

If repeated enumeration later becomes measurable overhead, a cache can be added with explicit invalidation on UPower owner changes and object errors. Caching should be a later optimization, not a prerequisite for correctness.

### Error classification

The provider should preserve capability-local meaning:

| Evidence | Battery result/classification |
|---|---|
| UPower service/name unavailable, transient D-Bus failure | `BackendUnavailable` / temporary-unavailable semantics |
| no matching system battery | `Unsupported` |
| multiple system batteries and no proven selection policy | `Unsupported` with explicit reason |
| malformed own contract/data | `Internal` / contract failure, not fake Unsupported |
| valid discovery but asusd configured source missing | Battery read unavailable/backend-missing according to existing provider error vocabulary; Session1 remains alive |
| valid backend | normal Battery read |

Do not collapse UPower transport absence into permanent hardware `Unsupported`.

## 1.6 Recommended UPower solution

Use **both layers**, with different responsibilities:

- **Required architecture fix:** Approach B, lazy/resilient Battery provider.
- **Recommended NixOS integration:** Approach A as `services.upower.enable = lib.mkDefault true` so the normal packaged configuration provides UPower, while allowing an explicit opt-out without breaking the whole daemon.

Do not add a hard `Requires=` from `orbis-sessiond` to UPower. Do not make UPower a package-level hard dependency for unrelated Orbis capabilities.

## 1.7 Exact future change locations

**Must change or be deliberately replaced:**

- `crates/orbis-sessiond/src/bootstrap.rs`
  - `connect_discovered_upower_session_server`: stop requiring `discover_battery()` before Session1 creation; rename if the old name becomes misleading.
- `crates/orbis-sessiond/src/composition.rs`
  - replace fixed `battery_object_path`/`battery_native_path` production composition with an always-constructible resilient Battery provider.
- `crates/orbis-sessiond/src/upower.rs`
  - add/implement the lazy discovered Battery provider or equivalent source;
  - reuse existing typed UPower/asusd/sysfs readers rather than duplicating their rules.
- `crates/orbis-sessiond/src/discovery.rs`
  - retain `discover_battery()` as the authoritative read-only discovery primitive;
  - refine service-unavailable/transient error classification if needed so transport absence is not mislabeled permanent Unsupported.
- `packaging/nix/module.nix`
  - add default UPower integration only after/with the resilience fix.

**Prefer not to change unless implementation evidence requires it:**

- `server.rs::build_session_server`
- `service.rs::SessionService`

A mandatory `BatteryProvider` object is acceptable if the object itself is lazy and capability-local. This is a smaller patch than making the D-Bus service dynamically add/remove the property provider.

## 1.8 Security and regression impact

**Security impact:** low. The preferred change adds no privilege and no writes. It removes an unnecessary availability coupling. Enabling UPower uses the distribution's standard system service.

**Regression risks:** error classification drift, repeated discovery cost, unexpected multi-battery behavior, and accidentally hiding an internal software contract error as ordinary unavailability. Preserve fail-loud behavior for `Internal`/malformed own-contract cases while keeping external backend absence capability-local.

**Live test required?** No for implementation validation. A real FA707NV read-only acceptance later should verify UPower recovery behavior, but no hardware mutation is required.

---

# 2. PolicyKit authority lifecycle

## 2.1 NixOS model

At the Orbis-pinned nixpkgs revision, `security.polkit.enable` is the native switch that provisions the system PolicyKit authority. The module:

- installs the polkit package;
- registers its D-Bus files;
- registers its systemd package;
- defines/overrides `systemd.services.polkit` to run `polkitd`;
- installs PolicyKit configuration/rules and supporting PAM/system users.

Therefore the Orbis module should not create a custom `polkit.service`, custom D-Bus activation file, or custom daemon user.

## 2.2 Authority versus authentication agent

These are different components.

### Authority

- system service;
- owns `org.freedesktop.PolicyKit1` on the **system bus**;
- evaluates actions/rules and `CheckAuthorization` requests;
- required by Orbis Hardware1 mutations because `PolkitAuthorizer` constructs an `AuthorityProxy` and calls `CheckAuthorization`.

### Authentication agent

- user-session UI component;
- registers with the authority for a login session;
- only needed when policy requires authentication/acknowledgement and the caller permits user interaction.

Official polkit documentation states that desktop sessions register authentication agents with the authority and that `AllowUserInteraction` is what lets a check invoke an agent when authentication could grant authorization.

Current Orbis is significant here:

- every current Hardware1 policy entry uses `allow_active=yes`, `allow_inactive=no`, `allow_any=no`;
- `PolkitAuthorizer` passes the default/zero check flags, not `AllowUserInteraction`;
- therefore an active local user is currently authorized by policy without requiring a password dialog.

So a graphical agent is **not** a current functional dependency for Orbis mutations. The authority is.

## 2.3 KDE Plasma, GNOME, minimal Wayland

- **KDE Plasma 6 on NixOS:** the Plasma module includes `polkit-kde-agent-1` as a required desktop package (`polkit auth ui`).
- **GNOME:** the NixOS GNOME core OS services enable `security.polkit.enable = true`, and GNOME Shell has a `PolkitAuthenticationAgent` implementation.
- **Minimal Wayland compositors/environments:** an authentication agent may be absent unless the user configures one. Orbis must not choose a compositor-specific or desktop-specific agent on the user's behalf.

Current Orbis policy remains usable in a minimal Wayland session as long as the session is recognized as active/local and the system authority is present, because no authentication challenge is requested.

If Orbis later changes an action to `auth_self`, `auth_admin`, or retained variants and wants interactive authorization, that is a separate product/security decision. At that time the code must deliberately pass `AllowUserInteraction` for explicit user-triggered operations and desktop documentation must say that an agent is needed. Do not pre-install a specific agent now.

## 2.4 Scenario: action XML installed, authority absent

The XML file alone does not evaluate authorization.

Current Hardware1 flow is:

caller
→ hardwared receives original D-Bus sender
→ `PolkitAuthorizer::authorize`
→ construct `org.freedesktop.PolicyKit1` authority proxy
→ `CheckAuthorization`.

If the authority is absent, proxy/check transport fails and Orbis maps that to `AuthorizeError::Failed`, then a D-Bus `Failed` result. The typed hardware backend is not reached, so no mutation occurs. This is fail-closed, but it is not a functional production deployment.

## 2.5 Production policy decision

### Preferred fix

Under `services.orbis-control.enable`, set:

`security.polkit.enable = true`

Use a normal module definition, not merely documentation, warning, or assertion.

Why:

- every supported Hardware1 mutation depends on a working authority;
- the full Orbis module owns the hardwared production lifecycle and installs its action policy;
- NixOS already provides the correct authority module;
- enabling the authority is deterministic and self-contained;
- an assertion that asks the user to enable a standard dependency adds friction without adding security.

### Do not add

- a dependency on `polkit-kde-agent-1`, GNOME Shell, Soteria, or another GUI agent;
- `Requires=polkit.service` on hardwared;
- a custom authority service;
- an assertion that a specific desktop environment is enabled.

Hardwared can start safely before/without authority because it does not mutate on startup. Authorization is checked per mutation. With NixOS polkit enabled, D-Bus/systemd integration provides the authority lifecycle; a temporary authority failure should remain a fail-closed per-call error rather than killing hardwared.

### Alternative considered

Only documenting `security.polkit.enable = true`, or emitting a warning/assertion, is weaker and leaves the fresh-install path non-self-contained. It is not recommended for the full production module.

## 2.6 Exact future change location

- `packaging/nix/module.nix`, inside `config = lib.mkIf cfg.enable { ... }`:
  - enable the native NixOS PolicyKit authority.

No Rust change is required to fix this blocker. Current caller preservation and fail-closed authorization behavior are correct.

## 2.7 Security and regression impact

**Security impact:** positive/neutral. It enables the standard authority needed to enforce the already-installed Orbis actions. It does not grant broader Orbis access beyond the action defaults and does not make the GUI privileged.

**Regression risk:** low. Main risk is NixOS module interaction if another configuration deliberately forces polkit off. A normal conflicting explicit configuration should be visible at Nix evaluation rather than silently producing an unusable mutation path.

**Live test required?** No for this implementation commit. A NixOS VM can prove the service and D-Bus owner without executing any Hardware1 setter.

---

# 3. Keyboard backlight versus hardwared systemd sandbox

## 3.1 Current mutation path

`SysfsKeyboardBacklightMutationBackend` uses two fixed kernel LED-class paths:

- read/write: `/sys/class/leds/asus::kbd_backlight/brightness`
- read-only: `/sys/class/leds/asus::kbd_backlight/max_brightness`

Mutation algorithm:

1. read and parse `max_brightness`;
2. reject a requested value above max;
3. write exactly `brightness`;
4. read `brightness` fresh;
5. return `Applied` only if read-back matches.

No generic client-supplied path exists.

## 3.2 Current hardwared sandbox semantics

Current NixOS unit settings include:

- `NoNewPrivileges=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `PrivateTmp=true`
- `PrivateDevices=true`
- `ProtectControlGroups=true`
- `RestrictAddressFamilies=AF_UNIX`
- `MemoryDenyWriteExecute=true`
- empty capability bounding set
- `ReadOnlyPaths=/sys`
- `ReadWritePaths=-/sys/firmware/acpi/platform_profile`
- **no `ProtectKernelTunables=true`**.

The relevant boundary is therefore `ReadOnlyPaths=/sys`, not `ProtectSystem=strict` alone.

Official systemd `systemd.exec` semantics are explicit:

- `ReadOnlyPaths=` makes listed paths read-only in the service mount namespace;
- `ReadWritePaths=` makes listed paths available with host-side access modes;
- a `ReadWritePaths=` entry may be nested below a `ReadOnlyPaths=` tree to create a narrow writable exception;
- path components containing symlinks are resolved in the service root context;
- `ReadWritePaths=` cannot turn an underlying filesystem whose superblock is itself mounted read-only into a writable filesystem.

Therefore **yes**, with the current Orbis sandbox a specific LED brightness file can be re-opened writable through `ReadWritePaths=`. No broader mechanism is required.

## 3.3 `ProtectKernelTunables`

Current Orbis intentionally does **not** set it.

This matters because `ProtectKernelTunables=true` independently creates read-only protection for `/sys` and other kernel-control surfaces. The remediation should not introduce `ProtectKernelTunables` in the same commit and should not claim it was already active.

For the current unit, the required change is fully expressible with the existing `ReadOnlyPaths` + nested `ReadWritePaths` model.

If a future hardening change adds `ProtectKernelTunables=true`, the exact LED exception must be regression-tested against the target systemd version before adoption. Do not replace that test with an assumption about directive precedence.

## 3.4 Symlink semantics of `/sys/class/leds/...`

The class entry can be a symlink into the actual device tree under `/sys/devices/...`.

This is **not** a reason to make the real device subtree broadly writable. systemd documents that paths containing symlinks are resolved for these namespace path directives. The narrow class path can therefore remain the deployment contract.

The `-` prefix should be retained for the optional LED path:

- on hardware without that LED class, absence must not prevent the whole root daemon from starting;
- if the path is absent when the service mount namespace is created, systemd ignores that exception;
- if the LED were to appear only later, the exception would not retroactively be created, so a service restart may be needed. On the target laptop the built-in keyboard LED is expected to exist before hardwared startup; this should still be verified by the acceptance test.

## 3.5 Exact recommended sandbox solution

Keep all current restrictions and extend only the writable exception list conceptually to:

- `-/sys/firmware/acpi/platform_profile`
- `-/sys/class/leds/asus::kbd_backlight/brightness`

`max_brightness` remains under `ReadOnlyPaths=/sys` and therefore read-only.

### Explicitly do not recommend

- making `/sys` writable;
- making `/sys/class/leds` writable;
- making the whole `asus::kbd_backlight` directory writable;
- disabling `ProtectSystem`, `PrivateDevices`, or other sandbox flags;
- adding generic filesystem/path parameters to Hardware1;
- `DevicePolicy`/`DeviceAllow` for this fix: those controls apply to access to block/character device nodes, while this backend writes a sysfs attribute pseudo-file;
- a broad `BindPaths=/sys/...` tree.

## 3.6 Alternative if the direct exception fails a real namespace test

Only if the NixOS VM proves that the class-symlink path cannot be made writable with the exact `ReadWritePaths` entry on the target systemd version:

1. resolve the canonical LED target during deployment/test investigation;
2. consider an equally narrow bind/write exception for the single `brightness` file;
3. keep the public Hardware1 backend path fixed and typed;
4. do not broaden the writable directory.

This is a fallback, not the preferred implementation. `ReadWritePaths` is the mechanism systemd documents specifically for a writable child inside a read-only tree.

A more architectural alternative would be a typed UPower keyboard-backlight D-Bus backend, since UPower exposes a KbdBacklight interface. That would change backend ownership/dependency semantics and is not justified merely to fix this sandbox mismatch. It should be a separate evidence-based architecture decision, not part of this remediation.

## 3.7 Exact future change location

- `packaging/nix/module.nix`
  - hardwared `serviceConfig.ReadWritePaths` only.

The standalone development unit `packaging/orbis-hardwared.service` should eventually be kept semantically aligned if that deployment path remains supported, but the production fix must be made and tested through the NixOS module. Do not mix the standalone dev deployment with the packaged acceptance path.

## 3.8 Security and regression impact

**Security impact:** bounded and reviewable. The root daemon gains write access to one additional kernel pseudo-file whose path is hard-coded in the typed backend. No client can choose a path or arbitrary content format.

**Regression risks:** systemd mount-namespace behavior around a symlinked class path, path absence at unit startup, and accidentally widening the exception to a directory. These risks are testable without a hardware write.

**Live test required?** A real hardware write is not required to validate the sandbox patch. A later FA707NV acceptance should inspect effective mount properties and only then, outside this research task, perform any deliberately authorized mutation test.

---

# 4. Keyboard mutation capability probe

## 4.1 Current defect

`SysfsKeyboardBacklightMutationBackend::mutation_status()` returns `Supported` unconditionally.

That statement currently means only “the backend object was constructed.” It does not prove:

- the LED device exists;
- `brightness` is readable/valid;
- `max_brightness` is readable/valid;
- current brightness is consistent with max;
- the service mount namespace permits the write;
- a future write will succeed.

The backend is always attached in `crates/orbis-hardwared/src/main.rs` through `HardwareService::with_keyboard_backlight`, so configuration alone cannot be treated as hardware evidence.

## 4.2 What a no-write probe can prove

A fresh read-only probe can safely establish **structural backend evidence**:

1. read `max_brightness`;
2. parse it as the expected integer type;
3. read current `brightness`;
4. parse it;
5. require `brightness <= max_brightness`;
6. preserve filesystem error classes.

Suggested classification:

| Read-only evidence | Mutation status |
|---|---|
| LED/required attribute absent (`NotFound`) | `Unsupported` |
| read permission denied | `PermissionDenied` |
| transient/other expected I/O failure | `TemporarilyUnavailable` |
| malformed numeric data or contradictory `brightness > max` | `Unknown` at the public status boundary, with internal diagnostic retained; do not call it unsupported |
| both attributes valid and internally consistent | `Supported` **as structural mutation-path evidence only** |

Do not add guessed hardware-model tables.

## 4.3 What a no-write probe cannot prove

It cannot prove that a future write will succeed.

In particular, these are insufficient to promise runtime writability:

- POSIX mode bits alone;
- successful read access;
- backend object construction;
- `access(W_OK)`/metadata alone;
- static knowledge that NixOS intends to create a `ReadWritePaths` exception.

Mount namespaces, LSMs, filesystem state, kernel behavior, and runtime changes can still make the real setter fail. The first real mutation remains the authoritative operational proof and already requires fresh read-back.

This is consistent with the existing Orbis Performance mutation-status model: `Supported` can mean the required ABI/backend is present while operational write failures are still possible. The documentation/UI must not reinterpret it as “the next mutation is guaranteed to succeed.”

## 4.4 Three distinct facts to preserve

Do not collapse these concepts:

### A. Hardware capability present

Read-only evidence that the kernel LED class exposes valid `brightness` and `max_brightness`.

### B. Mutation backend configured

The typed Orbis backend is attached to Hardware1. This is necessary but not sufficient for `Supported`.

### C. Actual mutation attempted/authorized

Only an explicit Hardware1 setter request:

- identifies the original caller;
- passes PolicyKit;
- validates requested level;
- writes brightness;
- performs authoritative read-back.

The capability probe must never perform C.

## 4.5 Preferred implementation model

Follow the existing capability-local/read-only status pattern rather than cache a permanent optimistic bool.

Preferred future shape:

- make keyboard `mutation_status` perform a **fresh read-only probe**;
- if necessary, make the trait/status D-Bus method async, as other Hardware1 status paths already support async probing patterns;
- reuse the same typed read helpers used by the setter;
- keep a write counter/fake IO in unit tests and assert it remains zero for every status branch;
- keep the setter unchanged as the only write path.

Avoid reading `/proc/self/mountinfo` in production merely to manufacture stronger confidence. Deployment writability belongs to the NixOS/systemd test layer; hardwared's runtime capability probe should report what it can directly prove without mutation.

## 4.6 Exact future change locations

- `crates/orbis-hardwared/src/keyboard_backlight.rs`
  - `KeyboardBacklightMutationBackend::mutation_status`
  - `SysfsKeyboardBacklightMutationBackend::mutation_status`
  - possibly the IO trait/helper surface if a dedicated read-only probe helper is cleaner.
- `crates/orbis-hardwared/src/lib.rs`
  - `HardwareService::keyboard_backlight_mutation_status` if the status call becomes async or needs error-to-status mapping.
- `crates/orbis-hardwared/src/main.rs`
  - no new privilege; only adjust construction if the chosen status object stores validated structural state. A fresh probe is preferred to a startup-only cache.

## 4.7 Security and regression impact

**Security impact:** positive. No new writes are introduced, and the UI/client receives less optimistic capability information.

**Regression risks:** malformed-data classification, changing a synchronous status method to async internally, and accidentally treating transient failure as permanent Unsupported.

**Live test required?** No. Read-only FA707NV inspection can confirm the status later. This remediation task must not write keyboard brightness.

---

# 5. Exact remediation patch plan — description only

## Commit A — sessiond UPower resilience

| Field | Plan |
|---|---|
| Problem | Battery/UPower discovery is a whole-sessiond startup dependency |
| Root cause | `connect_discovered_upower_session_server()` discovers one battery before Session1 is constructed |
| Preferred fix | always construct Session1 with a lazy/retryable Battery provider; discovery occurs on Battery reads; external backend absence is capability-local |
| Alternative | only enable UPower in NixOS; rejected as incomplete because no-battery/restart still kills sessiond |
| Files/functions | `bootstrap.rs`, `composition.rs`, `upower.rs`, reuse/refine `discovery.rs::discover_battery`; `runtime.rs` only if helper naming/lifecycle changes; avoid `service.rs/server.rs` changes if possible |
| NixOS integration | add `services.upower.enable = lib.mkDefault true` with the resilience change or immediately after it; no `Requires=upower.service` |
| Security impact | none/new reads only; lower availability coupling |
| Regression risk | error classification, repeated discovery cost, multi-battery semantics |
| Tests | missing UPower, late UPower, no battery, UPower restart/object recreation, independent non-battery Session1 reads |
| Live test required? | No |

## Commit B — NixOS PolicyKit lifecycle

| Field | Plan |
|---|---|
| Problem | action XML is installed but authority lifecycle is not guaranteed |
| Root cause | Orbis module does not enable native NixOS polkit module |
| Preferred fix | `security.polkit.enable = true` when full Orbis module is enabled |
| Alternative | assertion/warning/documentation only; rejected as non-self-contained |
| Files/functions | `packaging/nix/module.nix` only |
| Security impact | establishes standard fail-closed authority; no GUI privilege change |
| Regression risk | low; explicit conflicting host policy becomes evaluation/configuration concern |
| Tests | module eval + NixOS VM `polkit.service`/D-Bus owner checks; no setters |
| Live test required? | No |

## Commit C — narrow hardwared LED sandbox

| Field | Plan |
|---|---|
| Problem | direct keyboard LED setter is blocked by `ReadOnlyPaths=/sys` |
| Root cause | only `platform_profile` is currently excepted through `ReadWritePaths` |
| Preferred fix | add optional exact `-/sys/class/leds/asus::kbd_backlight/brightness` to hardwared `ReadWritePaths` |
| Alternative | exact-file bind exception only if VM proves `ReadWritePaths` fails; do not widen directory |
| Files/functions | `packaging/nix/module.nix` hardwared service sandbox |
| Security impact | one additional hard-coded writable sysfs attribute; no generic path API |
| Regression risk | symlink/mount-namespace behavior, path absent at startup |
| Tests | effective systemd properties + test-only namespace/symlink mount assertions without writing |
| Live test required? | No hardware write required for implementation; later acceptance inspects real mount/path |

## Commit D — keyboard mutation capability honesty

| Field | Plan |
|---|---|
| Problem | status is always `Supported` even when LED ABI/path is absent or unreadable |
| Root cause | backend construction is conflated with capability evidence |
| Preferred fix | fresh read-only `brightness` + `max_brightness` structural probe with typed status classification; zero writes |
| Alternative | startup-only probe/cache; less preferred because it becomes stale |
| Files/functions | `keyboard_backlight.rs`, Hardware1 status method in `lib.rs`; `main.rs` only if construction shape changes |
| Security impact | positive; no mutation in probe, less optimistic UI |
| Regression risk | status/error classification and async internal API changes |
| Tests | missing files, permission, transient IO, malformed values, valid ABI; assert write count zero |
| Live test required? | No |

---

# 6. No-write test strategy

## 6.1 NixOS module evaluation/build tests

Add future tests that evaluate the full module and assert:

- `services.orbis-control.enable = true` enables PolicyKit authority;
- UPower is enabled by Orbis's default integration unless explicitly overridden after sessiond becomes resilient;
- hardwared still uses system scope/root boundary;
- sessiond remains a user service;
- no dependency makes sessiond privileged;
- no generic `/sys` writable path appears.

## 6.2 Sessiond missing/late UPower tests

Use fake/isolated D-Bus services; no hardware access.

Required cases:

1. start sessiond with no `org.freedesktop.UPower` owner;
   - Session1 becomes available;
   - Performance/GPU independent read behavior remains accessible according to their own backends;
   - Battery reports unavailable, not process death.
2. expose UPower after Session1 is already running;
   - next Battery read retries discovery and can become available.
3. UPower running but returns zero battery candidates;
   - Battery is Unsupported;
   - Session1 remains alive.
4. UPower owner disappears and returns;
   - sessiond process remains alive;
   - a later Battery read re-discovers rather than relying permanently on stale object path.
5. restarted fake UPower returns a different battery object path/native name;
   - rediscovery follows the new path.
6. malformed own provider contract still fails loudly as internal/contract failure rather than being silently converted to Unsupported.

## 6.3 PolicyKit tests

### Module/VM positive path

With the full Orbis module enabled:

- `systemctl is-active polkit.service` succeeds after boot/activation as appropriate;
- `org.freedesktop.PolicyKit1` is owned on the system bus when queried/activated;
- Orbis action XML is installed;
- hardwared remains root and Session1 remains user-scoped.

Do not call a Hardware1 mutation in this test.

### Negative unit path

With a fake authorizer/authority transport failure:

- authorization returns failure;
- backend write counter remains zero.

The current code already follows this pattern; retain it as a regression invariant.

## 6.4 Hardwared sandbox/static tests

Extend the NixOS lifecycle VM/property assertions to require:

- `ReadOnlyPaths` still contains `/sys`;
- `ReadWritePaths` contains exactly the intended writable capability paths, including keyboard `brightness`;
- `max_brightness` is not listed writable;
- no parent LED directory is listed writable;
- `ProtectSystem=strict`, `PrivateDevices=true`, empty capability bounding set, and the other existing restrictions remain intact.

For mount-namespace proof without hardware writes:

- create a test-only fake sysfs-shaped tree or namespace fixture;
- include a symlinked class path analogous to `/sys/class/leds/asus::kbd_backlight` pointing to a fake device subtree;
- inspect effective mount options (`findmnt`/mount namespace evidence) for `brightness` and its parent/read-only sibling;
- prove `brightness` is the sole intended RW exception and `max_brightness` remains RO;
- do not write to either file.

If the test framework cannot represent `/sys` faithfully, retain the static unit property assertion and perform the real-path mount inspection later on FA707NV before any mutation.

## 6.5 Keyboard provider/unit tests

Use fake IO and a write counter.

Required status cases:

- brightness missing → `Unsupported`, writes = 0;
- max missing → `Unsupported`, writes = 0;
- read permission denied → `PermissionDenied`, writes = 0;
- transient read error → `TemporarilyUnavailable`, writes = 0;
- malformed brightness → `Unknown`/internal mapping, writes = 0;
- malformed max → `Unknown`/internal mapping, writes = 0;
- brightness greater than max → inconsistent/Unknown, writes = 0;
- valid brightness/max → structural `Supported`, writes = 0.

Keep existing setter tests separate: setter authorization/validation/read-back behavior must not be invoked by status probing.

---

# 7. Manual FA707NV packaged acceptance plan — perform later

This section defines the later acceptance gate. **Do not execute it as part of this remediation-plan task.**

## Phase 1 — clean deployment state

1. Remove/disable the standalone development `orbis-hardwared.service` and `/usr/local/bin/orbis-hardwared` deployment if present; do not mix it with the NixOS module.
2. Configure the full Orbis NixOS module from the implementation branch containing Commits A-D.
3. Rebuild/reboot as required by the NixOS test procedure.
4. Log into the normal graphical user session as a non-root user.

## Phase 2 — lifecycle/read-only checks

Without hardware writes:

- verify `orbis-hardwared.service` active and root-owned;
- verify `orbis-sessiond.service` active in the user manager;
- verify Hardware1 system-bus name and Session1 session-bus name;
- verify `polkit.service`/`org.freedesktop.PolicyKit1` authority availability;
- verify UPower availability in the default configuration;
- verify the GUI starts as the ordinary user;
- read capability/status properties only;
- read `/sys/class/leds/asus::kbd_backlight/brightness` and `max_brightness` only;
- inspect `systemctl show orbis-hardwared` effective sandbox properties;
- inspect mount namespace/effective mount mode for the exact brightness path and confirm `max_brightness`/parent `/sys` remain read-only;
- verify keyboard mutation status does not require a write.

## Phase 3 — UPower resilience checks

Still without hardware writes:

- stop/restart UPower in a controlled maintenance step if the acceptance procedure allows service lifecycle testing;
- confirm sessiond itself remains available;
- confirm only Battery becomes temporarily unavailable during the outage;
- confirm Battery recovers after UPower returns without restarting sessiond;
- do not change battery charge settings.

## Phase 4 — mutation acceptance, separate explicit step

Only after Phases 1-3 pass should a later, explicitly authorized hardware acceptance execute selected mutations and verify read-back. This plan does not prescribe or execute a keyboard brightness change. If keyboard mutation is included later, choose a reversible/observed value under a separate hardware-test procedure and preserve the existing polkit/original-caller boundary.

---

# 8. Final remediation order and acceptance gate

## Order

1. **Commit A — sessiond UPower resilience**
   - removes the incorrect whole-daemon dependency first;
   - add/default-enable UPower NixOS integration only once absence is safe.
2. **Commit B — NixOS PolicyKit lifecycle**
   - makes privileged authorization self-contained without changing policy semantics.
3. **Commit C — narrow hardwared LED sandbox**
   - makes the existing typed keyboard setter deployable while preserving `/sys` read-only by default.
4. **Commit D — keyboard mutation capability honesty**
   - ensures clients do not confuse backend construction with proven structural capability.

Commits C and D are independent and should remain separate even if reviewed in one PR: one changes privilege/mount surface, the other changes read-only capability evidence.

## Gate for first full packaged NixOS acceptance test

Orbis can proceed to the first full packaged NixOS acceptance test on FA707NV when all of the following are true in the implementation branch:

- Session1 starts and survives with UPower absent/no battery, and Battery can recover after UPower appears/restarts;
- the NixOS module provides UPower by default without making it a whole-session hard lifecycle requirement;
- the NixOS module enables the PolicyKit authority;
- no specific GUI authentication agent is required by Orbis's current `allow_active=yes` policy;
- hardwared's effective sandbox keeps `/sys` read-only except the existing Performance file and exact keyboard `brightness` file;
- the symlinked LED class path behavior is covered by VM/static namespace evidence or inspected read-only on target hardware before a setter is used;
- keyboard mutation status is produced by a zero-write structural probe rather than unconditional `Supported`;
- all no-write unit, module, D-Bus, and NixOS VM checks pass;
- the packaged PR/branch contains no leftover standalone dev deployment conflict.

At that point the deployment blockers from PR #6 are remediated at source/deployment level, and a controlled real-hardware acceptance may begin.

---

# 9. Official/source references used

## Orbis

- Audit PR #6: `https://github.com/arttvad9r/Orbis-control/pull/6`
- Audit document: `docs/nixos-production-deployment-audit.md` at `4f19f5c0c835e33f9a1885209625f5a32ca03d6c`
- `packaging/nix/module.nix`
- `crates/orbis-sessiond/src/{runtime,bootstrap,discovery,composition,upower,server,service}.rs`
- `crates/orbis-hardwared/src/{main,lib,keyboard_backlight}.rs`
- `packaging/nix/polkit/io.github.orbiscontrol.hardware.policy`

## NixOS / nixpkgs

Checked against the Orbis-pinned nixpkgs revision `b7c2ada94fe99c15b0dbcf4d11fd7850b957a436`:

- UPower module: `https://github.com/NixOS/nixpkgs/blob/b7c2ada94fe99c15b0dbcf4d11fd7850b957a436/nixos/modules/services/hardware/upower.nix`
- PolicyKit module: `https://github.com/NixOS/nixpkgs/blob/b7c2ada94fe99c15b0dbcf4d11fd7850b957a436/nixos/modules/security/polkit.nix`
- Plasma 6 module: `https://github.com/NixOS/nixpkgs/blob/b7c2ada94fe99c15b0dbcf4d11fd7850b957a436/nixos/modules/services/desktop-managers/plasma6.nix`
- GNOME module: `https://github.com/NixOS/nixpkgs/blob/b7c2ada94fe99c15b0dbcf4d11fd7850b957a436/nixos/modules/services/desktop-managers/gnome.nix`

## systemd

- `systemd.exec` source/man page: `https://github.com/systemd/systemd/blob/e1515be25ba6ed5966bb5bce92bc7c5956d60b24/man/systemd.exec.xml`
- relevant semantics: `ProtectSystem`, `ProtectKernelTunables`, `ReadOnlyPaths`, `ReadWritePaths`, `BindPaths`, path/symlink resolution.
- `systemd.resource-control` source/man page for `DevicePolicy`/`DeviceAllow`: `https://github.com/systemd/systemd/blob/e1515be25ba6ed5966bb5bce92bc7c5956d60b24/man/systemd.resource-control.xml`

## polkit

- Authority API: `https://polkit.pages.freedesktop.org/polkit/PolkitAuthority.html`
- system-bus Authority interface: `https://polkit.pages.freedesktop.org/polkit/eggdbus-interface-org.freedesktop.PolicyKit1.Authority.html`
- authentication agents: `https://polkit.pages.freedesktop.org/polkit/polkit-agents.html`
- GNOME Shell authentication-agent API: `https://gnome.pages.gitlab.gnome.org/gnome-shell/shell/class.PolkitAuthenticationAgent.html`

## UPower

- UPower service/API: `https://upower.freedesktop.org/docs/UPower.html`
- `upowerd` lifecycle/automatic D-Bus startup: `https://upower.freedesktop.org/docs/upowerd.8.html`

No blogs or forum posts were used as authority for the recommendations above.
