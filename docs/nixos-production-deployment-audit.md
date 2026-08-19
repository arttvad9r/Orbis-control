# Orbis Control — NixOS Production Deployment Audit

**Audit date:** 2026-08-18  
**Audited production source:** `chatgpt/production-hardening-20260818`  
**Audited hardening SHA:** `a745f559e1dc3aeefc665b31f740242bc4cb7348`  
**`main` SHA observed immediately before this audit commit:** `b9bfdf5bb08fecea58589bcd204215b49384d9ef`  
**Audit type:** source-level deployment/readiness audit only  
**Hardware writes performed:** none

## Executive assessment

Orbis has a coherent NixOS packaging skeleton and the important privilege split is structurally correct: `orbis-hardwared` is a root system service, `orbis-sessiond` is a user-session service, and the GUI is an ordinary user process. The Hardware1 D-Bus mutation API is narrow and typed, policy files exist for every current mutation method, and authorization is performed against the original system-bus caller rather than through `sessiond`.

The audited snapshot is **not yet ready to be called a self-contained production NixOS installation**. Three deployment blockers should be fixed before the first full packaged installation acceptance test:

1. `orbis-sessiond` currently requires a working UPower battery discovery path at startup, but the NixOS module neither enables nor declares that runtime requirement. On a fresh system without UPower, the user daemon fails as a whole rather than degrading only Battery.
2. The module installs the Orbis polkit action file but does not ensure a PolicyKit authority is enabled/running. Hardware1 mutations therefore are not a self-contained fresh-install path.
3. The production `orbis-hardwared` sandbox makes all `/sys` read-only except `platform_profile`, while `SetKeyboardBacklight` writes `/sys/class/leds/asus::kbd_backlight/brightness`. The backend also reports mutation status `Supported` without probing that path. The deployed keyboard mutation is therefore inconsistent with the unit sandbox and can be falsely advertised.

A **read-only/package smoke test** on FA707NV can still be useful now if UPower and polkit are pre-existing and no hardware mutations are attempted. A **full first packaged acceptance test** should wait for the three blockers above.

---

# 1. Flake / package

## 1.1 Flake inputs and outputs

`flake.nix` exposes:

- `nixosModules.orbis-control` — the full NixOS module;
- `nixosModules.orbis-hardwared-policies` — a separate dev-only/static policy registration module;
- `packages.orbis-control` / default package — the full application package;
- `packages.orbis-hardwared` — standalone fast dev package for the root daemon only;
- `packages.orbis-hardwared-policies` — D-Bus/polkit static files only;
- `apps.default` — `${orbis-control}/bin/orbis-control`;
- `checks.default` — fmt/clippy/workspace tests;
- targeted NixOS VM checks for hardwared lifecycle, Performance mutation, and Battery mutation.

`flake.lock` pins `nixpkgs` and `flake-utils`; the production build is therefore reproducible against the recorded lock rather than an unpinned live `nixos-unstable` checkout.

## 1.2 Full package binaries

The workspace contains these binary crates:

| Binary | Cargo crate | Deployment role | Required for normal production runtime? |
|---|---|---|---|
| `orbis-control` | `orbis-ui` | GUI | Yes, for interactive application |
| `orbis-sessiond` | `orbis-sessiond` | user/session read service | Yes, for current Session1 read path |
| `orbis-hardwared` | `orbis-hardwared` | privileged Hardware1 service | Yes, for privileged mutations/status |
| `orbisctl` | `orbis-cli` | CLI | No; not referenced by the service lifecycle |

The full `package.nix` does not limit Cargo to one package. The NixOS module directly references `${cfg.package}/bin/orbis-sessiond` and `${cfg.package}/bin/orbis-hardwared`, and the NixOS VM test imports the full module and expects the hardware daemon to start. Source/tests therefore expect the full derivation to provide the service binaries in addition to the GUI. `orbisctl` is not a deployment prerequisite.

No additional helper executable is required by the audited Hardware1 implementations. Hardware mutation backends use fixed sysfs paths or typed D-Bus clients; there is no shell helper or generic root helper binary.

## 1.3 Build/runtime libraries

`package.nix` includes GUI/build libraries for fontconfig/freetype/OpenGL/xkbcommon/Wayland, D-Bus, OpenSSL, systemd, GLib/Cairo/Pango/GdkPixbuf. `orbis-control` alone is wrapped with a fixed Nix-store `LD_LIBRARY_PATH` prefix for the dlopen libraries proven necessary for the Wayland/winit/glutin path.

The root Cargo configuration enables both Slint/winit Wayland and X11 backends. The Nix wrapper explicitly supplies the Wayland-side dlopen set, but the package does not explicitly add/wrap an X11 runtime library set such as X11/XCB. Therefore:

- Wayland: **PARTIAL/PROVEN BY PACKAGING INTENT** — explicit wrapper exists;
- X11: **UNKNOWN/PARTIAL** — compiled backend is enabled, but the package does not explicitly prove the complete runtime dlopen closure.

This is not a blocker for a Wayland-only FA707NV smoke test, but must be resolved before claiming general NixOS desktop support.

## 1.4 Installed deployment files

The full package installs:

- `share/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf`;
- `share/polkit-1/actions/io.github.orbiscontrol.hardware.policy`.

The full NixOS module additionally creates the systemd services and places the package in `environment.systemPackages`; it also exposes the polkit policy through `/etc/polkit-1/actions/...`.

The package does **not** install:

- a `.desktop` file;
- an application icon;
- AppStream metadata;
- a D-Bus activation `.service` file for Hardware1;
- a session-bus activation `.service` file for Session1;
- an autostart entry.

The lack of D-Bus activation files is not intrinsically a defect because both Orbis services currently use explicit systemd lifecycle. The missing desktop/icon/autostart integration is a product deployment gap.

---

# 2. systemd services

## 2.1 `orbis-hardwared`

| Property | Audited deployment |
|---|---|
| Scope | system service |
| Effective user | root (no `User=` override) |
| Type | `dbus` |
| Bus name | `io.github.orbiscontrol.Hardware` |
| ExecStart | `${cfg.package}/bin/orbis-hardwared` |
| Startup target | `multi-user.target` |
| Ordering | `After=dbus.service`, `Requires=dbus.service` |
| Restart | `on-failure`, 2 seconds |
| Environment | no custom environment/path command execution |
| Privilege model | root service with empty capability bounding set and strong systemd sandbox |
| D-Bus activation | no activation file; explicit systemd startup |

This is the correct architectural boundary: the hardware daemon remains the system/root boundary and the GUI does not run privileged.

### Sandbox

The module sets, among others:

- `NoNewPrivileges=true`;
- `ProtectSystem=strict`;
- `ProtectHome=true`;
- `PrivateTmp=true`;
- `PrivateDevices=true`;
- `ProtectControlGroups=true`;
- `RestrictAddressFamilies=AF_UNIX`;
- `MemoryDenyWriteExecute=true`;
- empty ambient/bounding capabilities;
- `/sys` read-only;
- only `/sys/firmware/acpi/platform_profile` re-opened writable.

This is correct for Performance mutation and for backends that write through `asusd`, but **incorrect for the current direct LED-class keyboard mutation**. `SetKeyboardBacklight` writes `/sys/class/leds/asus::kbd_backlight/brightness`, which remains read-only inside the production unit.

No generic writable `/sys` exception should be added. If the keyboard backend is retained, only the exact proven LED path should be made writable and its capability/status must be probed honestly.

## 2.2 `orbis-sessiond`

| Property | Audited deployment |
|---|---|
| Scope | user service |
| User | logged-in ordinary user |
| Type | `dbus` |
| Bus name | `io.github.orbiscontrol.Session` |
| ExecStart | `${cfg.package}/bin/orbis-sessiond` |
| Lifecycle | `WantedBy` and `PartOf` `graphical-session.target` |
| Restart | `on-failure`, 2 seconds |
| D-Bus activation | no activation file; explicit user-systemd startup |
| Privileged deputy | No; Session1 is getter-only |

The logout lifecycle is appropriate for a session service: `PartOf=graphical-session.target` makes it leave with that user graphical session, while hardwared remains system-wide.

The current production bootstrap opens the system bus, performs UPower battery discovery once, opens the session bus, and then owns Session1 until SIGINT/SIGTERM. A UPower/battery discovery failure is a startup error for the entire daemon. This makes UPower an effective required dependency of the present service lifecycle, even though battery capability should ideally be separable from unrelated capabilities.

No ordering or enabling relation to UPower is present in `module.nix`.

## 2.3 GUI lifecycle

The GUI is not a systemd service. It runs as the invoking user, opens both the user session bus and system bus, then builds the production runtime. This is correct privilege-wise.

There is no packaged desktop launcher or autostart backend. Therefore the application does not automatically start merely because `services.orbis-control.enable = true`.

## 2.4 Stale/development module options

`services.orbis-control.mockDevice` and `readOnlyEmpty` still affect the generated `ExecStart` expression, but the audited `orbis-sessiond` main path does not parse those legacy command-line controls. The Nix expression also constructs nested lists when those options are enabled. These are development/dead-option debt and should not be used as production deployment features.

---

# 3. D-Bus deployment

## 3.1 Hardware1

Source contract:

- bus: `io.github.orbiscontrol.Hardware`;
- object: `/io/github/orbiscontrol/Hardware`;
- interface: `io.github.orbiscontrol.Hardware1`;
- bus: system bus;
- owner: `orbis-hardwared` root service.

Installed system-bus policy allows root to own the well-known name and allows callers to send to that destination. The broad send allowance is paired with per-method polkit authorization inside hardwared; it is not itself the mutation authorization boundary.

No D-Bus activation service file is installed. This matches the explicit `Type=dbus` systemd service design.

## 3.2 Session1

Source contract:

- bus: `io.github.orbiscontrol.Session`;
- object: `/io/github/orbiscontrol/Session`;
- interface: `io.github.orbiscontrol.Session1`;
- bus: session bus;
- owner: `orbis-sessiond` user service.

Session1 is read-only. The audited interface exposes Battery, GPU primitive state and Performance getters; mutation APIs are deliberately absent.

No session-bus activation file is installed. Explicit `systemd --user` startup under `graphical-session.target` is the selected lifecycle.

## 3.3 Naming/ownership result

No naming mismatch was found between:

- systemd `BusName`;
- source constants/proxies;
- object paths/interfaces;
- installed system-bus policy.

No duplicate production owner is created by the full NixOS module itself.

There **is** an operational duplicate-owner hazard if the standalone dev deployment is left active. `packaging/deploy-dev-hardwared.sh` installs `/usr/local/bin/orbis-hardwared` plus `/etc/systemd/system/orbis-hardwared.service`, while the production module separately creates `orbis-hardwared.service` and wants the same D-Bus name. The policies-only module is also a separate dev path. A packaged test must remove/disable the standalone path first rather than mixing both deployments.

---

# 4. polkit / Hardware1 mutation matrix

The installed policy contains an action for every current Hardware1 mutation family. No Hardware1 mutation method without a corresponding policy action was found.

| Action | Hardware1 method | Backend | Policy entry present? | Installed by full package/module? | Original caller authorization preserved? | Deployment note |
|---|---|---|---|---|---|---|
| `io.github.orbiscontrol.set-performance-profile` | `SetPerformanceProfile` | fixed kernel `platform_profile` sysfs writer + fresh read-back | Yes | Yes | Yes | production sandbox explicitly allows this exact write path |
| `io.github.orbiscontrol.set-charge-limit` | `SetChargeLimit` | typed `asusd` battery setter + configured read-back + kernel effective threshold read | Yes | Yes | Yes | requires `asusd` for actual mutation; no direct sysfs write |
| `io.github.orbiscontrol.set-fan-curve` | `SetFanCurve` | typed `xyz.ljones.FanCurves` asusd D-Bus setter + fresh `FanCurveData` read-back | Yes | Yes | Yes | backend probe can distinguish missing asusd/interface |
| `io.github.orbiscontrol.set-fan-curve` | `ResetFanCurvesToDefaults` | typed asusd profile-wide defaults reset + fresh fan-curve observation | Yes (shared fan risk class) | Yes | Yes | sharing the fan action is consistent with same capability/risk class |
| `io.github.orbiscontrol.set-panel-overdrive` | `SetPanelOverdrive` | typed asusd armoury setter + kernel `asus-armoury` read-back | Yes | Yes | Yes | requires both attribute presence and asusd at mutation time |
| `io.github.orbiscontrol.set-keyboard-backlight` | `SetKeyboardBacklight` | direct kernel LED-class sysfs write + read-back | Yes | Yes | Yes | **INCORRECT deployment:** systemd sandbox does not permit the write path; backend status is unconditionally `Supported` |
| `io.github.orbiscontrol.set-aura-static-rgb` | `SetAuraStaticRgb` | typed asusd Aura setter + config-level read-back | Yes | Yes | Yes | returns config-confirmed/Accepted semantics; hardware write-only state cannot be read back |
| `io.github.orbiscontrol.set-gpu-mode` | `SetGpuMode` | production backend intentionally disabled | Yes | Yes | Yes | public surface exists, but current production result is `NotSupported` by design |

### Original caller preservation

Each Hardware1 mutation reads the sender from the inbound D-Bus message header. `PolkitAuthorizer` creates a PolicyKit `system-bus-name` subject using that unique sender and checks the capability-specific action. There is no route where `sessiond` calls Hardware1 on behalf of the GUI for these writes. This preserves the original application caller and avoids making `sessiond` a privileged deputy.

### Policy behavior caveat

The policy defaults are `allow_any=no`, `allow_inactive=no`, `allow_active=yes`. Therefore an active local session is authorized by policy without necessarily showing an authentication dialog. “Checks polkit” is correct; “always prompts the user” is not guaranteed by this policy.

### Missing PolicyKit authority

The full module makes the action XML visible but does not set/ensure `security.polkit.enable`. On a minimal fresh NixOS system the action file alone does not create `org.freedesktop.PolicyKit1`. Hardware1 mutations will fail authorization if no authority is available. This is a deployment blocker for the requested fresh-install mutation flow.

---

# 5. Desktop application integration

| Item | Status | Evidence/result |
|---|---|---|
| Application binary | READY | `orbis-control` and flake app entry exist |
| Application name | PARTIAL | UI/window uses Orbis Control naming, but no desktop metadata |
| `.desktop` file | MISSING | none found/installed |
| Icon | MISSING | no installed desktop icon integration found |
| Categories | MISSING | no `.desktop` metadata |
| Startup command | READY for shell | `orbis-control` / flake app points to packaged binary |
| Wayland | PARTIAL/READY for targeted test | explicit Slint Wayland backend and Nix dlopen wrapper |
| X11 | UNKNOWN/PARTIAL | backend compiled, but X11 dlopen runtime closure not explicitly packaged/wrapped |
| Autostart | MISSING | no XDG autostart entry or systemd-user GUI unit |
| Production `Run on Startup` persistence | MISSING | no production autostart backend/file found |

The module installs the package system-wide but does not create a graphical application launcher. For a first technical smoke test, launch from a terminal is possible. Before beta, desktop entry, icon and categories are required.

---

# 6. Runtime dependencies on NixOS

| Dependency/capability | Required or optional? | Current expected failure when missing | Package/service dependency today | Recommended deployment treatment |
|---|---|---|---|---|
| system D-Bus | Required | hardwared/sessiond system-bus operations fail; hardwared unit requires dbus | hardwared has `Requires/After=dbus.service`; `dbus` is a build input | keep hard runtime requirement |
| user session D-Bus | Required for GUI + Session1 | session service/GUI cannot connect | provided by graphical session, no activation file | keep session requirement; document supported session environment |
| UPower | **Required by current `sessiond` startup** | battery discovery failure aborts sessiond bootstrap | module does not enable/order it | either ensure UPower in module or redesign startup so Battery can degrade independently; until then treat as required |
| polkit | Required for Hardware1 mutation authorization | mutations fail at authorization; reads continue | action XML installed, authority not enabled by module | ensure PolicyKit authority for production module; do not treat XML alone as sufficient |
| `asusd` | Optional globally; required for configured battery read and Battery/Fan/Panel/Aura mutations | affected capability returns D-Bus/backend failure; fan mutation status has explicit missing-backend classification | no hard package/service dependency | do not make all Orbis depend on it; document/probe per capability, optionally expose module integration |
| `asus-armoury` kernel ABI | Optional per ASUS firmware capability | relevant MUX/access/panel/display files become Unsupported/Unavailable | no package dependency | probe filesystem capability; no model-wide hard dependency |
| `supergfxd` | Optional for current GPU power read; product mutation disabled | GPU power read unavailable; unrelated GPU sysfs capabilities can remain usable | no service dependency | probe service; do not hard-depend on it globally |
| Wayland | Optional desktop backend but primary packaged path | GUI cannot use Wayland if compositor/libs unavailable; X11 may be attempted | explicit runtime wrapper | keep runtime support; verify actual target compositor |
| X11 | Optional fallback | current packaged runtime completeness not proven | feature compiled, X11 runtime libs not explicit | verify and package required X11 dlopen libraries before claiming support |
| `/sys/class/hwmon` | Optional telemetry source | missing directory produces no hwmon telemetry rather than fake values | no package dependency | probe/read only |
| `/sys/class/power_supply` | mixed: required for current battery threshold path, optional for telemetry fields | Battery/session paths can fail; telemetry can omit fields | no package dependency | probe; remove cross-capability startup coupling where possible |
| LED class `asus::kbd_backlight` | Optional capability | read/write should become Unsupported/PermissionDenied | no package dependency; current status incorrectly says Supported | probe; only expose mutation if path/capability and sandbox permission agree |

### Dependency policy

No hard dependency should be added for optional ASUS capabilities merely because some models provide them. The correct model is capability-local probing and degradation. UPower is the exception **only because the current `sessiond` bootstrap makes it mandatory in source**; that coupling is a current deployment fact, not a desired architecture recommendation.

---

# 7. Fresh install scenario

Source-level walkthrough for a fresh NixOS system:

| Step | Status | Assessment |
|---|---|---|
| Add flake/module and enable `services.orbis-control` | READY | module/package entry points exist |
| Build/switch configuration | PARTIAL | package/module are defined, but external runtime requirements UPower/polkit are not made self-contained |
| Reboot/system startup | READY for hardwared | root `Type=dbus` hardwared is wanted by `multi-user.target` and requires system D-Bus |
| Login/graphical session | PARTIAL | `orbis-sessiond` is tied to `graphical-session.target`, but UPower battery discovery can abort the whole service |
| `orbis-sessiond` owns Session bus name | PARTIAL | correct name/lifecycle if bootstrap succeeds |
| GUI appears in desktop launcher | MISSING | no `.desktop`/icon |
| GUI launched manually as normal user | READY/PARTIAL | user process is correct; Wayland path is explicitly wrapped; X11 package runtime is not fully proven |
| Read capabilities/state | PARTIAL | Battery depends on UPower + asusd + power_supply; GPU primitives degrade per backend; telemetry probes sysfs directly in GUI; fan reads also have direct sysfs composition in GUI |
| Performance privileged mutation | PARTIAL | Hardware1/polkit contract and writable sandbox path align, but PolicyKit authority is not ensured by module |
| Battery/fan/panel/Aura mutation | PARTIAL | polkit/original caller are correct; actual optional asusd capability must exist |
| Keyboard backlight mutation | **INCORRECT** | Hardware1 is deployed but its direct LED write is blocked by the production unit sandbox |
| GPU product mutation | READY as unsupported | method/policy exist but production backend intentionally returns NotSupported; it must not be advertised as functional |
| hardwared survives user logout | READY | system service independent of graphical session |
| sessiond exits/restarts with graphical session | READY if bootstrap dependencies exist | `PartOf` graphical session is appropriate |
| GUI autostarts on next login | MISSING | no autostart backend |

Overall fresh-install status: **PARTIAL, not production-ready**.

---

# 8. Upgrade / removal

## 8.1 NixOS-managed production path

The normal full module uses immutable Nix store paths in `ExecStart` and declarative systemd/environment configuration. There is no custom in-place binary replacement in the production module.

No custom service migration logic is present. No claim should be made that state migrations occur on package upgrade.

The full module manages the active D-Bus policy through the package in `environment.systemPackages` and manages the polkit action through `environment.etc`. Disabling/removing the module should remove those paths from the active system generation; historical Nix store outputs may remain until garbage collection, which is normal and is not an active policy installation.

## 8.2 Persistent application state

`orbis-config` defines an XDG user configuration location:

- `$XDG_CONFIG_HOME/orbis-control`, or
- `~/.config/orbis-control` when `XDG_CONFIG_HOME` is unset.

It also contains TOML load/save, atomic rename and backup helpers. However, the audited production GUI/sessiond entrypoints inspected for deployment do not establish a production startup persistence workflow using that store. Therefore this audit does **not** claim that Orbis currently writes or migrates production configuration during normal installation.

If/when the config store is wired into production, uninstall should normally leave user-owned XDG configuration intentionally unless a separate purge action is explicitly defined. No purge semantics exist in this snapshot.

## 8.3 Standalone dev path cleanup

`packaging/deploy-dev-hardwared.sh` is not the production NixOS module. It installs persistent host artifacts:

- `/usr/local/bin/orbis-hardwared`;
- `/etc/systemd/system/orbis-hardwared.service`;
- `/nix/var/nix/gcroots/orbis-hardwared`.

It has no uninstall/purge mode. Its D-Bus/polkit registration is expected to come from the separate policies-only NixOS module.

A machine previously using this dev path must be cleaned before testing the full packaged module; otherwise service/unit and D-Bus ownership can be confused with the production deployment.

---

# 9. Deployment-specific security review

## 9.1 Privilege and service boundary

**Confirmed correct:**

- GUI is not root.
- `orbis-sessiond` is a normal user service.
- `orbis-hardwared` is the only Orbis root service in the full module.
- Hardware1 mutation methods use typed semantic arguments, not arbitrary paths/commands.
- Session1 is getter-only and is not a privileged deputy.

## 9.2 Filesystem access

Hardwared is root but heavily sandboxed. `/sys` is readable and only the Performance path is writable under the current unit. That is preferable to granting generic sysfs write access.

The keyboard direct-write feature conflicts with this sandbox. The fix must preserve least privilege: do not replace the sandbox with broad `/sys` write access.

Battery, fan, panel and Aura mutation backends do not require generic direct sysfs writes from hardwared:

- Battery: mutation through asusd; sysfs used for effective read-back.
- Fan curves/defaults: mutation through asusd.
- Panel Overdrive: mutation through asusd; kernel sysfs read-back.
- Aura: mutation through asusd; config-level read-back.

## 9.3 D-Bus policy breadth

The system-bus policy permits senders to contact `io.github.orbiscontrol.Hardware`. The mutation boundary is the per-method polkit check. This is acceptable provided all mutation methods continue to authorize before backend mutation.

The policy grants active local sessions (`allow_active=yes`). This means another unprivileged process running in the same active user session may be authorized for these actions without an interactive challenge. The project should explicitly decide whether that is the intended beta policy; current behavior is not equivalent to “always ask for a password”.

## 9.4 Polkit action granularity

Actions are capability-specific for Performance, Battery, GPU, fan curves, Panel Overdrive, keyboard backlight and Aura. Fan custom write and factory reset intentionally share one fan action/risk class. No generic “all hardware” action is used.

## 9.5 Original caller

Hardware1 reads the sender from the incoming D-Bus message header and passes it to polkit as a `system-bus-name` subject. The GUI-side production providers for Battery, Performance and fan mutation call Hardware1 directly; fan defaults also call it directly. Thus `sessiond` does not erase caller identity.

## 9.6 Environment/PATH/shell injection

- Production systemd `ExecStart` uses absolute immutable Nix-store paths.
- Hardware mutation backends use zbus/std filesystem APIs, not shell command strings.
- No generic path or command mutation endpoint was found.
- The GUI wrapper prefixes `LD_LIBRARY_PATH` with immutable Nix-store library paths. It runs as the user, not root.

No deployment-specific root shell/PATH injection path was identified in the audited service chain.

## 9.7 Writable configuration ownership

The defined XDG config path is user-owned rather than a root daemon config path. No evidence was found that hardwared consumes arbitrary user-writable config to decide generic privileged paths/commands.

---

# 10. Final readiness assessment

| Area | Status | Blocker before real install? | Evidence | Recommended fix |
|---|---|---:|---|---|
| Flake/package structure | READY | No | locked inputs; full package/module outputs; service paths referenced by VM/module | keep |
| Binary coverage | READY/PARTIAL evidence | No | workspace contains GUI/sessiond/hardwared/orbisctl; module/VM source expects service binaries | verify actual `nix build` output in packaged test |
| hardwared privilege boundary | READY | No | root system `Type=dbus`, strong sandbox, typed Hardware1 | keep narrow root boundary |
| hardwared keyboard mutation | **INCORRECT** | **Yes** | LED-class direct write conflicts with `/sys` read-only + only platform_profile writable; status unconditionally Supported | add only exact safe write allowance or redesign backend; probe actual capability/status |
| sessiond lifecycle | **PARTIAL** | **Yes** | UPower battery discovery can abort startup; module does not enable/order UPower | make dependency explicit or decouple Battery startup failure |
| polkit runtime | **PARTIAL** | **Yes** | action XML installed; PolicyKit authority not enabled by Orbis module | ensure/declare PolicyKit runtime for production module |
| D-Bus names/paths | READY | No | module and source names match | keep |
| D-Bus activation | READY by explicit lifecycle | No | no activation files, but systemd services explicitly own names | document as intentional |
| original caller authorization | READY | No | sender header -> `system-bus-name` polkit subject | keep; do not route writes through sessiond |
| Performance mutation deployment | READY/PARTIAL | No after polkit blocker fixed | sandbox exact path aligns with writer | validate in real packaged environment |
| Battery mutation deployment | PARTIAL | No global hard dependency | asusd writer + sysfs readback; optional backend | probe/document asusd, do not hard-depend globally |
| Fan mutation deployment | PARTIAL | No global hard dependency | typed asusd backend and read-only status probe | keep capability-local |
| Panel Overdrive deployment | PARTIAL | No global hard dependency | asusd setter + asus-armoury readback | capability-local probe; improve backend-status honesty if needed |
| Aura Static RGB deployment | PARTIAL | No global hard dependency | asusd setter; hardware state cannot be read back | preserve Accepted/config-confirmed semantics |
| GPU product mutation | READY as unavailable | No | production backend intentionally disabled | do not expose as functional |
| Desktop entry/icon | MISSING | No for shell smoke; Yes before beta | none installed | add `.desktop`, icon, categories, AppStream as appropriate |
| Autostart | MISSING | No for first shell test; Yes before claiming feature | no production persistence/autostart backend | implement one explicit user-session mechanism |
| Wayland runtime | PARTIAL | No for targeted test | explicit wrapper | verify on FA707NV compositor |
| X11 runtime | UNKNOWN/PARTIAL | No for Wayland test | backend enabled but runtime libs not explicitly proven | package/test X11 closure or narrow support claim |
| Upgrade/removal | PARTIAL | No for first smoke | declarative NixOS path; no migration/purge semantics | define when persistence becomes real; document dev cleanup |
| Dev-vs-production coexistence | INCORRECT if mixed | No if cleaned first | standalone `/usr/local` unit owns same service/name | remove dev deployment before packaged test |
| NixOS production readiness | **PARTIAL** | **Yes** | three blockers above plus desktop maturity gaps | fix blockers, then run FA707NV packaged plan |

---

# A. MUST FIX before first real packaged test

1. **Make the current UPower requirement deterministic.** Either the NixOS module must ensure the UPower service required by current sessiond bootstrap, or sessiond must stop treating Battery discovery failure as a whole-daemon startup failure. The test must not depend on an undocumented pre-existing desktop service.
2. **Ensure a PolicyKit authority is part of the production deployment contract.** Installing `io.github.orbiscontrol.hardware.policy` is insufficient when `org.freedesktop.PolicyKit1` is absent. The expected privileged flow must work from a fresh supported NixOS configuration.
3. **Reconcile keyboard-backlight Hardware1 with the hardwared sandbox and capability status.** The deployed service currently cannot legitimately write the LED brightness path under its own unit policy, while the backend reports `Supported` without probing. Preserve least privilege; do not broadly reopen `/sys`.

# B. SHOULD FIX before beta

1. Add `.desktop`, icon, categories and normal desktop discoverability.
2. Implement/choose a real user autostart mechanism before exposing or claiming “Run on Startup”.
3. Verify and package the X11 runtime dlopen dependency set, or explicitly scope the supported GUI environment.
4. Remove/fix the stale `mockDevice` / `readOnlyEmpty` Nix module options.
5. Make mutation-status evidence as honest for Battery/Panel/Aura/keyboard as the fan backend probe: constructing an asusd client object must not by itself prove the service/capability is available.
6. Explicitly document that `allow_active=yes` can authorize without an authentication prompt and decide whether that is the desired beta policy.
7. Document and/or provide cleanup guidance for the standalone dev deployment so users do not run both root daemons.
8. Add a packaged Session1 lifecycle VM test analogous to the hardwared lifecycle test, including behavior when UPower is missing/unavailable.

# C. Can defer

1. D-Bus activation files, as long as explicit systemd service startup remains the chosen architecture.
2. Hard dependencies on `asusd`, `supergfxd`, `asus-armoury`, hwmon, LED class, or optional ASUS features. These should remain capability-local probes.
3. AppStream metadata until desktop launcher/icon integration is being prepared for beta distribution.
4. Config migration/purge semantics until the XDG config store is actually wired into production persistence.
5. GPU product-mode deployment; current production `NotSupported` is honest and safer than enabling an unproven transition backend.

# D. Confirmed correct

1. GUI is an ordinary user process.
2. hardwared remains the system/root mutation boundary.
3. sessiond remains an unprivileged read service and is not a generic privileged deputy.
4. Hardware1 methods are typed/bounded rather than generic path/command setters.
5. System D-Bus name, object path and interface names are internally consistent with the NixOS unit/policy.
6. Session D-Bus name, object path and interface match the user service.
7. Every current Hardware1 mutation family has a polkit action.
8. Original system-bus caller identity is preserved for Hardware1 authorization.
9. Performance direct sysfs write is limited to the exact `platform_profile` path in the systemd sandbox.
10. Battery/fan/panel/Aura mutations delegate to typed owners rather than opening generic root sysfs mutation APIs.
11. GPU product mutation is disabled in production rather than optimistically exposed.
12. Telemetry uses read-only dynamic hwmon/power_supply discovery and does not require hardcoded `hwmonN` paths.
13. The standalone dev deployment is visibly separate from the full module; it is not silently part of the packaged lifecycle.

---

# Minimal manual packaged-install test plan — FA707NV

**Plan only. This audit did not execute any command on FA707NV and performed no hardware writes.**

## Phase 0 — clean deployment boundary

1. Record the current NixOS generation and Orbis-related configuration.
2. Verify the standalone dev service is not active: no `/etc/systemd/system/orbis-hardwared.service` overriding the generated unit, no `/usr/local/bin/orbis-hardwared` being used, and no stale `orbis-hardwared-policies` dev module enabled in parallel with the full module.
3. Do not delete user configuration; only record whether `$XDG_CONFIG_HOME/orbis-control` / `~/.config/orbis-control` exists.

## Phase 1 — prerequisites, before enabling Orbis

1. Confirm system D-Bus is active.
2. Confirm UPower is installed/running and a battery is discoverable. Until blocker A1 is fixed, failure here makes the test invalid rather than proving Orbis package readiness.
3. Confirm PolicyKit authority exists on the system bus. Until blocker A2 is fixed, this is an external prerequisite.
4. Record whether `asusd`, `supergfxd` and the kernel `asus-armoury` firmware-attribute tree exist. Treat absence as capability evidence, not a test failure for unrelated features.
5. Record the desktop session type (Wayland/X11).

## Phase 2 — install/activate package

1. Import `nixosModules.orbis-control` and set `services.orbis-control.enable = true`.
2. Build/test the NixOS generation first; do not perform any hardware mutation.
3. Verify the package output contains at least `orbis-control`, `orbis-sessiond`, `orbis-hardwared`; record whether `orbisctl` is present.
4. Activate the generation.

## Phase 3 — service lifecycle, read-only

1. Verify `orbis-hardwared.service` is active, root-owned, uses the Nix store `ExecStart`, and owns `io.github.orbiscontrol.Hardware` on the system bus.
2. Inspect effective sandbox properties and confirm the write whitelist has not widened unexpectedly.
3. Login to the graphical session and verify `orbis-sessiond.service` is active in the user manager and owns `io.github.orbiscontrol.Session`.
4. Introspect Hardware1 and Session1; compare bus/object/interface names with this audit.
5. Restart hardwared and sessiond independently and verify they reacquire their names.
6. Logout/login: hardwared must remain system-active; sessiond should follow the graphical user session.

## Phase 4 — GUI/read paths, no hardware writes

1. Launch `orbis-control` manually from a normal user terminal because no `.desktop` file exists yet.
2. Confirm the process UID is the logged-in user, not root.
3. Observe Performance/Battery/GPU/Fan/telemetry states and compare unavailable states with the prerequisites recorded in Phase 1.
4. Confirm missing optional backends do not become fake values.
5. Check journal logs for D-Bus ownership errors, missing service errors, permission denials, or repeated restart loops.
6. On Wayland, confirm normal window creation. If testing X11 separately, treat missing dlopen libraries as a packaging defect rather than falling back to manual host library injection.

## Phase 5 — privileged mutation gate

Do **not** run this phase until the three MUST FIX items are resolved and a separate hardware-write test is explicitly approved.

When approved later:

1. Start with the lowest-risk already-proven reversible mutation (Performance profile), record the original value, perform one change through the normal GUI/Hardware1 path, verify authoritative read-back, then restore the original value.
2. Verify PolicyKit evaluated the original GUI caller. Do not assume a password dialog must appear because current policy is `allow_active=yes`.
3. Test Battery/fan/panel/Aura only when the corresponding capability/backend is proven present and the dedicated hardware-validation procedure exists.
4. Do not test keyboard-backlight mutation until the systemd sandbox/status inconsistency is fixed.
5. Do not test GPU product mutation; current production backend is intentionally disabled.

## Phase 6 — removal/rollback

1. Disable `services.orbis-control` and activate the new NixOS generation.
2. Confirm both generated Orbis units and active D-Bus ownership disappear as expected.
3. Confirm the active polkit/D-Bus policy paths from the full module are no longer present in the active generation.
4. Confirm no standalone `/usr/local` daemon/unit was created by the production module.
5. Leave any user XDG config untouched unless a future explicit purge contract is defined.

---

## Audit conclusion

**Deployment blockers: 3.**

The source is close enough for a controlled read-only NixOS package smoke test, but **not yet for the first full production-path packaged acceptance test**. Fix the UPower lifecycle contract, ensure PolicyKit runtime availability, and reconcile keyboard-backlight mutation with the hardwared sandbox/capability status. After those fixes, the FA707NV plan above can validate the real installation chain without mixing dev deployment artifacts or optional ASUS dependencies.
