# Orbis Control — UI → production backend wiring matrix

**Audit date:** 2026-08-18  
**UI source:** `agent/light-theme-toggle` @ `4a63c6b8f36688a8b81fc2f87a94eb2216d8ee56`  
**Production backend source:** `chatgpt/production-hardening-20260818` @ `a745f559e1dc3aeefc665b31f740242bc4cb7348`  
**Audit branch:** `agent/ui-backend-wiring-audit`  
**Scope:** documentation only. No production code was changed.

This is a source-level audit. PR descriptions were not used as implementation evidence. The active Slint entrypoint is `ui/app-entry.slint`, compiled by `crates/orbis-ui/build.rs`; therefore the audited visual tree is `ui/audited/*`, not the legacy `ui/app-window.slint`.

The FA707NV hardware statements in this document are the task-supplied dated evidence and are intentionally kept separate from source-derived facts.

## Status vocabulary

Each interactive control receives exactly one primary status:

| Status | Meaning in this audit |
|---|---|
| `WIRED_READ_WRITE` | The current UI reaches a production read path and a production mutation path, and success is based on authoritative state/read-back. |
| `WIRED_READ_ONLY` | The current UI reaches a production authoritative read path; no mutation is exposed by that control. |
| `WIRED_PARTIAL` | Some end-to-end behavior is real, but the control/window is not fully production-usable because part of the read/write/persistence semantics is not connected or is currently blocked by a source-level defect. |
| `PREVIEW_ONLY` | The control only changes local Slint/Rust UI state or performs UI-only navigation; it does not execute the represented production operation. |
| `BACKEND_READY_NOT_WIRED` | A matching production provider/Hardware1 contract already exists, but the current UI/application runtime does not call it. |
| `UNSUPPORTED_ON_FA707NV` | The requested hardware capability is confirmed absent on the reference FA707NV. |
| `BACKEND_MISSING` | The UI semantic is understandable, but no matching production provider/application operation exists. |
| `SEMANTICS_NOT_PROVEN` | Safe mapping, units/ranges, ownership, lifecycle/security semantics, or the product-level meaning is not proven. It must not be wired by guessing. |

### Counting convention

The final counts cover **194 interactive semantic controls**. Repeated controls with identical wiring are grouped in the tables but counted individually. Examples: eight fan temperature steppers count as eight; the nine Extra binding action selectors plus nine parameter fields count as eighteen. Read-only text/metric surfaces are audited separately and are not added to the interactive-control count.

Interactive-control totals:

| Status | Count |
|---|---:|
| `WIRED_READ_WRITE` | 5 |
| `WIRED_READ_ONLY` | 0 |
| `WIRED_PARTIAL` | 26 |
| `BACKEND_READY_NOT_WIRED` | 2 |
| `PREVIEW_ONLY` | 74 |
| `BACKEND_MISSING` | 20 |
| `UNSUPPORTED_ON_FA707NV` | 5 |
| `SEMANTICS_NOT_PROVEN` | 62 |
| **Total** | **194** |

`WIRED` in the summary means the union of `WIRED_READ_WRITE`, `WIRED_READ_ONLY`, and `WIRED_PARTIAL`: **31 controls**.

---

# 1. Executive summary

The production UI/backend integration is narrower than the visual surface.

Fully working interactive production mutations are currently:

1. Performance profile: Silent / Balanced / Turbo.
2. Battery charge threshold: slider and `100%` action.

The fan editor has substantial real wiring, including typed Hardware1 custom-curve mutation and factory-default reset, but the **current production composition makes its required profile-specific read fail**. `build_production_runtime()` constructs `SessionHardwareFanCurveProvider<SysfsFanCurveProvider<...>, ...>`, while `WorkerCommand::RefreshFanCurve` calls `fan_curve_for_profile(profile, fan)`. `SysfsFanCurveProvider::fan_curve_for_profile()` explicitly returns `Unsupported` because the kernel hwmon ABI contains only the active curve. The same source file already contains a typed asusd `FanCurveData(profile)` read source, but it is not the provider used by the production UI composition. Result: the initial fan refresh becomes `Unavailable`/error and blocks normal editor use. This is documented only; it is not fixed here.

Production read plumbing already exists for the three independent GPU primitives:

- physical MUX (`gpu_mux_mode`);
- dGPU access (`dgpu_disable`);
- runtime dGPU power (supergfxd `Power()`).

They are refreshed independently into `UiState`. They do **not** establish the product mode Eco / Standard / Ultimate / Optimized. Production explicitly sets product GPU mode to unavailable and the production `GpuPrimitiveServices` rejects `SetGpuMode`. Raw Hardware1 `SetGpuMode` also uses a disabled production backend. Therefore the four product-mode cards remain `SEMANTICS_NOT_PROVEN`.

Several newer backend capabilities are source-complete but not integrated into the current UI runtime:

- Panel Overdrive read + mutation;
- keyboard backlight read + mutation;
- Aura read + Static RGB mutation;
- Wayland display-output read-only;
- MiniLED read-only;
- Screen Auto Brightness read-only.

On FA707NV, MiniLED and Screen Auto Brightness are confirmed absent, as are AniMe and Slash. Panel Overdrive, keyboard backlight and Aura TUF Static RGB are confirmed present and are high-value wiring candidates.

Telemetry is already real and read-only, but its current production path is `orbis-ui` → `SysfsTelemetryProvider` directly, not Session1 → sessiond. The same composition-level architecture deviation exists for the active fan-curve read provider. These are source-level defects/debt, not claims that the data is fake.

Power-limit controls must stay disabled/preview. Although FA707NV exposes `ppt_pl1_spl`, `ppt_pl2_sppt`, `ppt_pl3_fppt`, `nv_dynamic_boost`, and `nv_temp_target`, the task-supplied evidence says `current/min/max/default` are empty because FA707NV is missing from the kernel DMI `power_limits` table. The source tree contains domain types/traits but no safe production `PowerLimitProvider` implementation with proven device metadata. The controls are therefore `SEMANTICS_NOT_PROVEN`, not merely “not wired”.

---

# 2. Backend capability baseline

This table describes backend facts independently of whether a current UI control uses them.

| Domain concept | Production source/provider | Application/runtime status | Hardware1 mutation | FA707NV conclusion |
|---|---|---|---|---|
| Battery charge threshold | Session1 read via `SessionChargeLimitProvider`; composed `SessionHardwareBatteryProvider`; hardwared typed asusd writer + kernel effective read | In `ProductionRuntime`; capability registry contains `ChargeLimit` | `SetChargeLimit`, polkit `io.github.orbiscontrol.hardware.set-charge-limit`, `Applied` | Runtime-probed; do not substitute `charge_mode` as threshold evidence |
| Performance profile | Session1 `KernelPerformanceProvider`; composed `SessionHardwarePerformanceProvider` | In `ProductionRuntime`; capability registry contains `Performance` | `SetPerformanceProfile`, polkit `...set-performance-profile`, `Applied` | Production-wired |
| Physical GPU MUX | `ArmouryGpuProvider` → `gpu_mux_mode/current_value` | In Session1 and `ProductionRuntime`, read-only | None for product UI | `gpu_mux_mode` confirmed present |
| dGPU access | `ArmouryGpuProvider` → `dgpu_disable/current_value` | In Session1 and `ProductionRuntime`, read-only | None for product UI | `dgpu_disable` confirmed present |
| dGPU runtime power | `SupergfxdGpuPowerProvider` → supergfxd `Power()` | In Session1 and `ProductionRuntime`, read-only | None | Backend-dependent, separate from product mode |
| Product GPU mode | No proven production `GpuProductPolicy`; production runtime is primitive-only | Explicitly unavailable; registry does not synthesize `GpuProductPolicy` | Raw `SetGpuMode` method exists but production backend is deliberately disabled | `SEMANTICS_NOT_PROVEN` |
| Fan active curve | `SysfsFanCurveProvider` → `asus_custom_fan_curve` hwmon | Used by capability probe; current UI profile-specific read is incompatible | N/A | Active curve readable if hwmon exists |
| Fan profile curve | Typed `ZbusAsusdFanCurveSource::read_curves(profile)` exists, but production UI composition does not use a matching profile provider | Current `RefreshFanCurve` fails through `SysfsFanCurveProvider::fan_curve_for_profile()` | N/A | Backend read source exists; integration defect |
| Fan custom mutation | `AsusdFanCurveMutationBackend` | Worker/AppService/Hardware1 path exists, but UI is blocked after profile read failure | `SetFanCurve`, polkit `...set-fan-curve`, `Applied` | Mutation backend is source-complete |
| Fan factory defaults | `AsusdFanCurveMutationBackend::reset_curves_to_defaults` | Worker FIFO (`WorkerCommand::ResetFanCurvesToDefaults`) → Hardware1; then refreshes selected curve; currently hard-blocked | `ResetFanCurvesToDefaults`, same fan polkit action, `Accepted` (not `Applied`; no independent default-evidence source) | Profile-wide reset |
| Telemetry | `SysfsTelemetryProvider` dynamic hwmon/power_supply | In `ProductionRuntime`, 1 Hz polling | None | Real read-only data; direct GUI runtime path |
| Panel Overdrive | `AsusArmouryPanelOverdriveProvider`; hardwared asusd setter + kernel current read | Production Hardware1 composition + UI operation-level gate | `SetPanelOverdrive`, polkit `...set-panel-overdrive`, `Applied` | FA707NV live-validated |
| MiniLED | `AsusArmouryMiniLedModeProvider` | Provider + probe exist; read-only; not current runtime/UI | None | Confirmed absent → `UNSUPPORTED_ON_FA707NV` |
| Screen Auto Brightness | `AsusArmouryScreenAutoBrightnessProvider` | Provider + probe exist; read-only; not current runtime/UI | None | Confirmed absent → `UNSUPPORTED_ON_FA707NV` |
| Wayland display outputs | `WaylandDisplayOutputProvider<WaylandCompositorOutputSource>` | Provider + probe exist; no modeset API; not current runtime/UI | None | Read-only candidate; does not make 60/120 controls writable |
| Keyboard backlight brightness | `AsusKeyboardBacklightProvider`; hardwared LED-class writer | Production Hardware1 + exact NixOS sandbox/polkit; UI derives writability from Supported | `SetKeyboardBacklight`, polkit `...set-keyboard-backlight`, `Applied` | FA707NV live-validated |
| Aura state | `AsusAuraProvider` over `xyz.ljones.Aura` | Read provider exists; not current runtime/UI | None for generic Aura mode | TUF Aura confirmed present |
| Aura Static RGB | hardwared `AsusdAuraStaticRgbMutationBackend` | Hardware1 exists; not current runtime/UI | `SetAuraStaticRgb`, polkit `...set-aura-static-rgb`, **`Accepted`** | Confirmed present; hardware RGB read-back is impossible |
| Power limits | Domain `PowerLimitProvider` / `PowerLimitValue` exists; no safe production implementation found in audited composition | Not in current `ProductionRuntime` | None | Attributes exist but metadata empty; unsafe to expose |
| Automation | Core/config models exist; no production desired-state/reconciliation executor in current runtime | UI local only | No approved background privileged execution path | Security/lifecycle semantics not proven |
| Updates | `FirmwareUpdateProvider` trait exists, no production implementation found | UI shell only | None | `BACKEND_MISSING` |

Key source locations:

- UI active entry: `crates/orbis-ui/build.rs`, `ui/app-entry.slint`.
- Current production composition: `crates/orbis-ui/src/composition.rs::build_production_runtime`.
- Session1 server: `crates/orbis-sessiond/src/service.rs`, `bootstrap.rs`.
- Hardware1: `crates/orbis-hardwared/src/lib.rs`, `main.rs`.
- Capability probes: `crates/orbis-providers/src/probes.rs`.
- GPU primitives: `crates/orbis-sessiond/src/armoury.rs`, `supergfxd.rs`.
- Fan reads: `crates/orbis-sessiond/src/fans.rs`.
- Fan mutations: `crates/orbis-hardwared/src/fans.rs`.
- Display ASUS attrs: `crates/orbis-providers/src/asus_armoury.rs`.
- Keyboard: `crates/orbis-providers/src/keyboard_backlight.rs`, `crates/orbis-hardwared/src/keyboard_backlight.rs`.
- Aura: `crates/orbis-providers/src/aura.rs`, `crates/orbis-hardwared/src/aura.rs`.
- Config: `crates/orbis-config/src/store.rs`.

---

# 3. Full UI → backend matrix

## 3.1 Main

| UI control(s) | Count | Domain concept | Provider / runtime path | Mutation/security | Status |
|---|---:|---|---|---|---|
| Silent / Balanced / Turbo | 3 | `PerformanceProfile` | Slint `perf-clicked` → worker `SetPerformance` → `AppService::set_performance` → `SessionHardwarePerformanceProvider`; read-back through Session1 | Hardware1 `SetPerformanceProfile`; `...set-performance-profile`; kernel `platform_profile` fresh read + application `PerformanceState`; `Applied`; no pending | `WIRED_READ_WRITE` |
| Fans + Power | 1 | Fan editor / power shell | Opens `FansWindow`; fan mutations/read callbacks exist, power panels local | See Fans section | `WIRED_PARTIAL` |
| Eco / Standard / Ultimate / Optimized | 4 | Product GPU policy, **not** MUX/access/power | Production sets `gpu_mode_state=Unavailable`, `gpu_mode_writable=false`; `GpuPrimitiveServices::set_gpu_mode` rejects product mode | Raw Hardware1 `SetGpuMode` exists but production `DisabledGpuMutationBackend` returns Unsupported | `SEMANTICS_NOT_PROVEN` |
| Laptop Screen: Auto | 1 | Display refresh policy | Only `screen-preview` local property | No display modeset/configuration backend | `BACKEND_MISSING` |
| Laptop Screen: 60 Hz | 1 | Display refresh mutation | Local only; Wayland provider is read-only current-output observation | No production refresh mutation | `BACKEND_MISSING` |
| Laptop Screen: 120 Hz + OD | 1 | Two distinct concepts conflated: refresh + Panel Overdrive | Local only | Panel OD backend is ready, refresh mutation is not; one button cannot safely map to both | `SEMANTICS_NOT_PROVEN` |
| Flicker-free Dimming slider | 1 | Display/OLED dimming | Local `dimming-preview` only | No proven provider/units/ownership | `SEMANTICS_NOT_PROVEN` |
| Visual Mode | 1 | Display color/profile | Local ComboBox | No matching production provider | `BACKEND_MISSING` |
| Color temperature | 1 | Display color temperature | Local ComboBox | No matching production provider | `BACKEND_MISSING` |
| Gamut | 1 | Display gamut | Local ComboBox | No matching production provider | `BACKEND_MISSING` |
| Slash/AniMe: effect, mode, interval, disable-on-battery, disable-on-lid | 5 | Slash/AniMe | Local properties only | No applicable device on FA707NV | `UNSUPPORTED_ON_FA707NV` |
| Keyboard mode | 1 | General Aura effect mode | Local ComboBox | Aura **read** exists; generic mode mutation is not implemented | `BACKEND_MISSING` |
| Keyboard Color | 1 | Aura Static RGB | Local color cycle today; exact Static RGB Hardware1 backend exists | `SetAuraStaticRgb`; `Accepted`, not `Applied` | `BACKEND_READY_NOT_WIRED` |
| FN-Lock | 1 | Keyboard policy/hotkey | Local toggle | No matching production backend | `BACKEND_MISSING` |
| Extra | 1 | UI navigation | Opens `ExtraWindow` | No hardware operation by launcher | `PREVIEW_ONLY` |
| Battery limit slider + `100%` | 2 | `ChargeLimit` | `charge-changed` → worker `SetChargeLimit` → `AppService` → composed Session1/Hardware1 provider | Hardware1 `SetChargeLimit`; `...set-charge-limit`; asusd configured + kernel effective read-back, then Session1 `ChargeLimit`; `Applied` | `WIRED_READ_WRITE` |
| Run on Startup | 1 | Application startup | Local bool only | No current runtime/autostart action | `PREVIEW_ONLY` |
| Donate | 1 | UI-only support action | Local support note | No backend expected in this audit | `PREVIEW_ONLY` |
| More | 1 | UI-only menu | Local popup | No backend expected | `PREVIEW_ONLY` |
| Updates | 1 | UI navigation | Opens Updates shell | No updater backend | `PREVIEW_ONLY` |
| Quit | 1 | UI lifecycle | Rust hides all maintained windows | No hardware backend | `PREVIEW_ONLY` |
| More → Automation / Preferences / Diagnostics / UI Dialogs | 4 | UI navigation | Opens native windows | No represented production mutation at launcher level | `PREVIEW_ONLY` |

### Main read-only data surfaces

These do not change the 194-control count.

| Surface | Source | Status / caveat |
|---|---|---|
| CPU temperature | `SysfsTelemetryProvider` (`k10temp`) → worker polling → UiState | `WIRED_READ_ONLY`; current path is direct application sysfs, not Session1 |
| GPU temperature | `SysfsTelemetryProvider` (`amdgpu`) | `WIRED_READ_ONLY`; absent stays `—` |
| CPU/GPU fan RPM | `SysfsTelemetryProvider` ASUS hwmon | `WIRED_READ_ONLY` |
| Battery percent/health/cycles/status | `SysfsTelemetryProvider` power_supply | `WIRED_READ_ONLY` |
| AC online | `SysfsTelemetryProvider` power_supply `online` | `WIRED_READ_ONLY` |
| dGPU telemetry power | `amdgpu power1_input` | `WIRED_READ_ONLY`; distinct from `GpuPowerState` |
| `power_ac_mw` detail | UiState field exists, but provider intentionally sets `PowerTelemetry.ac=None` | `WIRED_PARTIAL`; expected to remain `—` with current provider |
| Physical MUX / dGPU access / runtime power | worker refreshes all three independently into UiState | Backend `WIRED_READ_ONLY`, but current Main has no dedicated visible status control |

## 3.2 Fans + Power

The fan statuses below intentionally reflect the latest production composition defect: profile-specific reads are requested by UI but the composed `SysfsFanCurveProvider` rejects them.

| UI control(s) | Count | Domain concept | Current source path | Mutation/security | Status |
|---|---:|---|---|---|---|
| CPU / GPU tabs | 2 | `FanId` + profile curve read | `fan-changed` → `RefreshFanCurve(profile, fan)` → `fan_curve_for_profile` → current Sysfs provider returns Unsupported | Read-only operation | `WIRED_PARTIAL` |
| Advanced tab | 1 | Experimental advanced controls | Changes local page only | None | `PREVIEW_ONLY` |
| BIOS Fan Curves profile selector: Balanced/Performance/Quiet/Low Power | 1 | lossless `AsusdFanProfile` | `profile-changed` → `RefreshFanCurve`; current composed provider rejects profile-specific read | Typed asusd profile source exists elsewhere but is not this runtime provider | `WIRED_PARTIAL` |
| Add / Rename / Remove profile | 3 | Custom profile management | Only sets local `profile-status` | No production profile CRUD contract | `PREVIEW_ONLY` |
| CPU power mode selector | 1 | Power/EPP policy | Local only | No matching production operation | `BACKEND_MISSING` |
| CPU Boost selector | 1 | CPU boost policy | Local only | FeatureId/domain placeholder exists; no production provider | `BACKEND_MISSING` |
| SPL / sPPT / fPPT sliders | 3 | `ppt_pl1_spl`, `ppt_pl2_sppt`, `ppt_pl3_fppt` | Local preview values | FA707NV metadata empty; units/ranges/default not safe | `SEMANTICS_NOT_PROVEN` |
| GPU Power / Dynamic Boost / GPU Temperature sliders | 3 | GPU power limits / `nv_dynamic_boost` / `nv_temp_target` | Local preview | FA707NV power-limit metadata insufficient | `SEMANTICS_NOT_PROVEN` |
| Memory Offset / Core Offset / Clock Limit | 3 | GPU tuning | Local preview | No proven production contract/ranges | `SEMANTICS_NOT_PROVEN` |
| Apply Power Limits toggle + Apply Power Preview | 2 | Power-limit mutation | Local only | No safe production mutation backend | `SEMANTICS_NOT_PROVEN` |
| CPU Voltage Offset / iGPU Voltage Offset / CPU Temperature Limit | 3 | Undervolt/thermal advanced | Explicit `% preview` | No proven semantics/ranges/backend | `SEMANTICS_NOT_PROVEN` |
| Advanced Auto Apply / Read Limits / Apply / Restore Preview Defaults | 4 | Advanced lifecycle/persistence | Local status only | Would require proven contracts and safe ownership first | `SEMANTICS_NOT_PROVEN` |
| CPU / GPU chart selection | 2 | Fan selection/read | Same failing profile-specific refresh path | None | `WIRED_PARTIAL` |
| Eight PWM graph points | 8 | `FanCurvePoints.pwms`, raw 0..255 | UI editor emits `fan-pwm-point-changed`; mutation code exists, but editor becomes unavailable after failed profile read | Hardware1 `SetFanCurve`, same fan polkit, asusd `FanCurveData` full read-back, `Applied` | `WIRED_PARTIAL` |
| Eight temperature steppers | 8 | `FanCurvePoints.temps` | Local dirty edit + same custom Apply path; blocked by failed profile read | Same custom fan mutation | `WIRED_PARTIAL` |
| Clamp to Grid | 1 | Editor convenience | Local bool only | No backend needed/used | `PREVIEW_ONLY` |
| Hysteresis up/down | 2 | Fan hysteresis | Local preview only | No production contract | `PREVIEW_ONLY` |
| Calibrate | 1 | Fan calibration | Toggles local label | No production calibration operation | `PREVIEW_ONLY` |
| Factory Defaults | 1 | Staged profile-wide default reset intent | Button only stages local pending; actual reset is sent from Apply via worker FIFO; current fan error state can disable it | Hardware1 `ResetFanCurvesToDefaults`, fan polkit, asusd reset + `FanCurveData` observation, `Accepted` (not `Applied`) | `WIRED_PARTIAL` |
| Apply Custom Fan Curve / Apply Factory Defaults | 1 | Explicit fan mutation commit | Custom → worker; reset → worker FIFO (`ResetFanCurvesToDefaults`); both source-complete, but normal production editor is blocked by failed profile read | See mutation matrix | `WIRED_PARTIAL` |

**Documented defect — no fix in this branch:**  
`crates/orbis-ui/src/composition.rs::build_production_runtime()` constructs the fan read side with `orbis_sessiond::fans::SysfsFanCurveProvider`. `crates/orbis-ui/src/worker.rs` handles `RefreshFanCurve` through `FanServiceRuntime::fan_curve_for_profile`. `crates/orbis-sessiond/src/fans.rs::SysfsFanCurveProvider::fan_curve_for_profile()` explicitly returns `Unsupported`. `crates/orbis-ui/src/main.rs` maps that error to `fan_curve_state=Unavailable` and `fan_curve_error=true`. The source file also contains a typed `ZbusAsusdFanCurveSource::read_curves(profile)`, but current production composition does not use it for the UI profile read.

## 3.3 Extra

`ExtraWindow` starts with `local-status = "Visual-only controls · no hardware writes"`. Except for backend-ready keyboard brightness noted below, none of these controls currently have Rust production callbacks.

| UI control(s) | Count | Domain concept | Backend evidence | Status |
|---|---:|---|---|---|
| Binding action selectors: M1, M2, M3, M4, M5, Fn+F4, Fn+C, Fn+V, Fn+NmEnt | 9 | Hotkey/action bindings | Core `HotkeyProvider` trait exists, no production binding service/runtime found | `PREVIEW_ONLY` |
| Binding parameter fields for same 9 keys | 9 | Optional action/command argument | Local strings only | `PREVIEW_ONLY` |
| Bindings Reset / Help | 2 | UI-local editor actions | Reset mutates local properties; Help sets local status | `PREVIEW_ONLY` |
| Backlight Brightness | 1 | `KeyboardBacklightState` / hardware level | Read provider + Hardware1 mutation are production-wired; max is read from kernel and live read-back is proven on FA707NV | `WIRED_READ_WRITE_FA707NV` |
| Keyboard/Logo/Lightbar/Lid × Awake/Boot/Sleep/Battery state chips | 16 | Aura/device power-state policy | Current Aura provider covers effect/brightness state, not these 16 power-state semantics | `SEMANTICS_NOT_PROVEN` |
| XG Mobile | 1 | External GPU/device policy | No proven current product mapping | `SEMANTICS_NOT_PROVEN` |
| Animation speed | 1 | Aura effect speed mutation | Aura read includes speed, but no generic effect mutation backend | `BACKEND_MISSING` |
| Plugged timeout / Battery timeout | 2 | Backlight timeout policy | Local numeric fields; no production policy service | `BACKEND_MISSING` |
| Auto clamshell mode | 1 | UI/system policy | Local bool | `PREVIEW_ONLY` |
| Always on top | 1 | Window behavior | Local bool; no Rust window binding | `PREVIEW_ONLY` |
| Disable panel overdrive automation | 1 | Automation preference, not direct Panel OD state | Panel OD mutation exists, but automation engine/security path does not | `PREVIEW_ONLY` |
| Boot sound | 1 | ASUS firmware/device setting | No production provider | `BACKEND_MISSING` |
| Keystone sound | 1 | ASUS device setting | No production provider | `BACKEND_MISSING` |
| Status LEDs | 1 | ASUS device lighting/system setting | No production provider | `BACKEND_MISSING` |
| Keep GPU disabled on USB-C charger | 1 | GPU product/automation policy | Physical `dgpu_disable` read exists; policy and authorized automatic writes do not | `SEMANTICS_NOT_PROVEN` |
| Stop GPU apps when switching to Eco | 1 | Product GPU preflight policy | Product Eco mutation is not proven; application-kill policy not implemented | `SEMANTICS_NOT_PROVEN` |
| Manage NVIDIA services with dGPU | 1 | GPU lifecycle policy | Product lifecycle semantics not implemented | `SEMANTICS_NOT_PROVEN` |
| Touchpad NumberPad | 1 | ASUS device feature | No production provider | `BACKEND_MISSING` |
| Disable PCIe ASPM (plugged in) | 1 | PCIe/system power policy | No scoped production contract; broad system mutation would require separate ownership/safety design | `SEMANTICS_NOT_PROVEN` |
| Disable networking in Modern Standby | 1 | systemd/logind/network policy | No production contract; platform semantics not proven for NixOS runtime | `SEMANTICS_NOT_PROVEN` |
| Hibernate after | 1 | system power policy | No production runtime/config mapping | `BACKEND_MISSING` |
| UMA frame buffer | 1 | iGPU firmware memory | No proven safe provider/range/reboot semantics | `SEMANTICS_NOT_PROVEN` |
| Performance cores / Efficiency cores / Apply Cores | 3 | CPU topology control | Local numeric values only; no production ownership/read-back contract | `SEMANTICS_NOT_PROVEN` |
| ACPI DEVS Command / Parameter / Send | 3 | Generic firmware command | No closed typed capability. A generic privileged command/path API would violate current Hardware1 design | `SEMANTICS_NOT_PROVEN` |
| Optimal Brightness policy | 1 | Display/automation policy | No proven mapping to Screen Auto Brightness or compositor brightness | `SEMANTICS_NOT_PROVEN` |
| asusd toggle / supergfxd toggle / Start Services | 3 | Service lifecycle | Local state only; no production service-management API | `BACKEND_MISSING` |

### Extra Aura/keyboard boundary

Do not combine these controls:

- keyboard **brightness level** has a complete dedicated provider + Hardware1 mutation and is a wiring candidate;
- Aura **state/effect** has a read provider;
- Aura **Static RGB** has a narrow mutation with `Accepted`;
- generic Aura mode/speed/device-state mutations do not yet have equivalent production contracts.

## 3.4 Automation

The UI models desired-state rules, and `orbis-core::automation` plus `orbis-config::AutomationConfig` already define related data. That is not an executor. No production reconciliation/authorization path currently applies privileged actions in response to background events.

| UI control(s) | Count | Domain concept | Current backend/application status | Status |
|---|---:|---|---|---|
| Enable automation policies | 1 | Automation config | Local bool; config field exists but UI does not persist it and no engine consumes it | `PREVIEW_ONLY` |
| AC + Battery Performance selectors | 2 | Desired `PerformanceProfile` on power source | Interactive Performance mutation is proven, but background caller/security/reconciliation semantics are not | `SEMANTICS_NOT_PROVEN` |
| AC + Battery GPU policy selectors | 2 | Desired product GPU policy | Product GPU mode itself is not proven | `SEMANTICS_NOT_PROVEN` |
| AC + Battery Display selectors | 2 | Desired refresh policy | Core/config representation exists; no production display mutation backend | `SEMANTICS_NOT_PROVEN` |
| AC + Battery Lighting selectors | 2 | Desired lighting policy | No general lighting mutation/reconciliation contract | `SEMANTICS_NOT_PROVEN` |
| Apply on AC/battery change / Reconcile after resume / Write only when observed differs / Notify staged transitions | 4 | Trigger/reconciliation policy | Local toggles; no production event/reconciler integration in current runtime | `PREVIEW_ONLY` |
| Reset / Save Rules | 2 | Local rule editing/persistence | Reset/local status only; Save explicitly says no persistence/backend write | `PREVIEW_ONLY` |

The blocking issue is architectural, not UI plumbing: background automation cannot simply reuse sessiond as a generic privileged deputy. A pre-authorized scoped mutation model and desired/observed reconciliation path must be established before privileged automation is enabled.

## 3.5 Preferences

| UI control(s) | Count | Domain concept | Current status | Status |
|---|---:|---|---|---|
| Dark / Light | 2 | UI theme | Real cross-window session switch through Rust `ThemeState`; `orbis-config::UiConfig.theme` and atomic config store exist, but the UI neither loads nor saves it | `WIRED_PARTIAL` |
| Language | 1 | Localization | Local index only | `PREVIEW_ONLY` |
| Close button | 1 | Window lifecycle preference | Local index only. Config only has binary `close_to_tray`, not the complete three-state UI semantic | `PREVIEW_ONLY` |
| Restore window position | 1 | UI persistence | Local bool. Config has `remember_position`, but no current UI/runtime position persistence wiring | `PREVIEW_ONLY` |
| Run on startup | 1 | Autostart | Local bool; no current preference-to-NixOS/autostart action | `PREVIEW_ONLY` |
| Desktop notifications | 1 | Notification policy | Local bool | `PREVIEW_ONLY` |
| Tray indicator | 1 | Tray behavior | Local bool | `PREVIEW_ONLY` |
| Automatic update checks | 1 | Update policy | Local bool; updater backend absent | `PREVIEW_ONLY` |
| Update channel | 1 | Release channel | Local selector; updater backend absent | `PREVIEW_ONLY` |
| Telemetry refresh | 1 | Sampling cadence | Local slider. Production provider has `default_poll_interval()` and worker polling, but no current runtime setter/reconfigure command | `PREVIEW_ONLY` |
| UI scale preview | 1 | UI scale | Local slider only | `PREVIEW_ONLY` |
| Reset / Save | 2 | Preference persistence | Reset is local; Save explicitly says persistence not connected | `PREVIEW_ONLY` |

Persistence facts:

- `orbis-config` already provides XDG TOML load/save, schema validation and atomic temp-file rename.
- `UiConfig` has `theme`, `close_to_tray`, `start_minimized`, `remember_position`.
- The current UI Preferences model is broader than `UiConfig`; therefore the whole window cannot be labeled backend-ready as one capability.
- Theme switching is real for the current process but **session-only**.

## 3.6 Diagnostics

Interactive controls:

| UI control | Count | Current behavior | Production data that already exists | Status |
|---|---:|---|---|---|
| Copy Summary | 1 | Local status says clipboard backend not connected | Capability snapshot/UiState data could be formatted without privileged API | `PREVIEW_ONLY` |
| Open Logs / Capabilities | 1 | Toggles static local preview text | No log transport is currently wired to this window | `PREVIEW_ONLY` |
| Export Report | 1 | Local status says file export not connected | A sanitized report can be assembled from existing read-only data, but export path is not wired | `PREVIEW_ONLY` |

Current displayed data surfaces:

| Diagnostics surface | Current UI | Real backend data available now? | Wiring conclusion |
|---|---|---|---|
| Kernel | static `Linux / preview` | Yes, process/system metadata can be read unprivileged | UI still preview/static |
| Platform | static `ASUS / preview` | Some provider/device metadata exists | UI still preview/static |
| Orbis version | receives UiState version | UI-local value, not a hardware capability | Partially real presentation |
| Performance capability row | `Preview` badge | Yes: current runtime registry | Can wire without new privileged API |
| Battery threshold capability row | `Preview` badge | Yes: current runtime registry | Can wire without new privileged API |
| GPU primitives row | single `Preview` badge | Yes: **three separate** registry entries + observed states | Must present MUX/access/runtime power separately or explicitly as a summary; never infer product mode |
| Fan curves row | `Preview` badge | Registry entry exists, but profile-read defect must be visible | Can wire metadata; must not hide read failure |
| Power limits row | `Preview` badge | No safe FA707NV metadata | Keep unsupported/experimental, not a fake value |
| Display / Lighting row | `Preview` badge | Panel/DisplayOutput/Keyboard/Aura read providers exist outside current runtime | Requires unprivileged composition/transport wiring, not new privileged read API |
| asusd / supergfxd / Hardware1 provider rows | `Preview` | Provider diagnostics/status pieces exist, but several `health()` implementations are static `Healthy` and are not service liveness proofs | Do not display `Connected/Healthy` without live evidence |
| Log preview | static hardcoded lines | No diagnostics log transport to this window | Preview only |

Data that can be connected to Diagnostics **without adding a new privileged mutation API**:

1. current six-entry application capability registry (`Performance`, `ChargeLimit`, `GpuPower`, `GpuMux`, `GpuAccess`, `FanCurves`);
2. observed GPU primitive states already maintained in `UiState`;
3. existing telemetry snapshot/freshness;
4. Panel/MiniLED/ScreenAuto/DisplayOutput/Keyboard/Aura read providers after unprivileged composition/transport integration;
5. existing Hardware1 mutation-status getter methods are read-only metadata and require no polkit authorization, although current registry only consumes Performance/Battery/Fan mutation statuses.

## 3.7 Updates

`UpdatesWindow` explicitly states that Check/Install only exercise local UI state and no network/package operation occurs.

| UI control | Count | Backend | Status |
|---|---:|---|---|
| Channel | 1 | No production update-channel implementation | `PREVIEW_ONLY` |
| Check | 1 | `FirmwareUpdateProvider` is only a trait; no production implementation found | `PREVIEW_ONLY` |
| Release Notes | 1 | Toggles static local preview text | `PREVIEW_ONLY` |
| Install Update | 1 | No package/NixOS updater executor | `PREVIEW_ONLY` |

Conclusion: **Updates is currently a UI shell.** There is no real update backend in the audited production composition.

## 3.8 Dialogs

The dialog window has four local variants: Reboot required, Logout required, Action failed, Confirm action. Current Main opens only the generic confirm preview (`kind=3`) from `UI Dialogs`; mutation results are not automatically routed into these dialogs.

| Dialog action(s) | Count | Current path | Status |
|---|---:|---|---|
| Reboot variant: Cancel / Reboot Later | 2 | Dismiss hides dialog only | `PREVIEW_ONLY` |
| Logout variant: Cancel / Logout Later | 2 | Dismiss hides dialog only | `PREVIEW_ONLY` |
| Error variant: Close | 1 | Dismiss hides dialog only | `PREVIEW_ONLY` |
| Confirm variant: Cancel / Confirm | 2 | Both only dismiss the preview dialog | `PREVIEW_ONLY` |

The only current backend family with explicit pending/logout/reboot result modeling is the staged GPU backend contract, but product GPU semantics are deliberately disabled; therefore these dialog visuals must not yet be treated as a real GPU transition UX.

---

# 4. Functions that are really production-wired

## 4.1 Interactive read/write

### Performance profile

`ui/audited/main-window.slint::perf-clicked`  
→ `crates/orbis-ui/src/main.rs`  
→ `WorkerCommand::SetPerformance`  
→ `AppService::set_performance`  
→ `SessionHardwarePerformanceProvider`  
→ direct system-bus `Hardware1.SetPerformanceProfile`  
→ polkit original caller  
→ fixed kernel `platform_profile` write  
→ fresh kernel read-back  
→ application fresh Session1 `PerformanceState`.

Result: `Applied`, no pending/reboot/logout.

### Battery charge threshold

`charge-changed`  
→ `WorkerCommand::SetChargeLimit`  
→ `AppService::set_charge_limit`  
→ `SessionHardwareBatteryProvider`  
→ direct `Hardware1.SetChargeLimit`  
→ polkit original caller  
→ asusd configured threshold setter  
→ fresh asusd configured read + kernel effective threshold read  
→ application fresh Session1 `ChargeLimit`.

Result: `Applied`, no pending/reboot/logout.

## 4.2 Read-only data already reaching the current UI

- CPU/GPU temperature where sources exist;
- CPU/GPU fan RPM where ASUS hwmon exposes them;
- battery percent/health/cycles/status;
- AC online;
- dGPU telemetry power;
- capability registry metadata used for Performance/Battery/Fan mutation gating;
- GPU physical MUX, dGPU access and runtime power are refreshed into `UiState`, although the current Main does not expose dedicated primitive status controls.

## 4.3 Fan path: wired code but not currently end-to-end usable

Custom `SetFanCurve` and factory-default Hardware1 paths are production implementations with authoritative read-back. They are **not** listed as fully production-wired UI functions because the current profile-specific read path fails before the editor can reach normal Ready state.

---

# 5. `BACKEND_READY_NOT_WIRED`

## 5.1 Current interactive UI controls with this primary status

1. Main `Keyboard Color` → Aura Static RGB backend exists.
2. Extra `Backlight Brightness` → keyboard LED brightness backend exists.

## 5.2 Backend-ready capabilities without an exact standalone current control

### Panel Overdrive

Backend is ready for read and mutation on FA707NV. Current Main only has the compound `120 Hz + OD` chip. Do not bind that compound action directly; first separate Panel Overdrive from refresh-rate selection.

### Aura read-only state

Current keyboard mode/color UI is local. `AsusAuraProvider` can supply current/supported mode, current effect, current RGB, brightness and supported brightness. Generic effect-mode mutation is not ready; only Static RGB mutation is.

### Wayland display output

Provider can read current compositor outputs/current modes/refresh. It cannot set refresh. It is useful for status/Diagnostics and for deriving observed state, not for making the 60/120 chips writable.

### MiniLED / Screen Auto Brightness

Providers/probes exist, but FA707NV proves both absent; they should remain hidden/unsupported on this machine rather than wired as controls.

### Config persistence infrastructure

`orbis-config` is usable infrastructure, but current Preferences/Automation semantics are only partially represented and no production UI load/save lifecycle is connected. It is a foundation, not proof that every Preferences control is backend-ready.

---

# 6. Preview-only UI

Major visual-only groups:

- Main menu/navigation/support UI and local startup checkbox.
- Fan profile Add/Rename/Remove.
- Fan Clamp/Hysteresis/Calibration visuals.
- All binding editor operations in Extra.
- Extra Auto clamshell / Always on top / “disable OD automation” local toggles.
- Automation shell/triggers/Save/Reset UI; action selectors are stronger-blocked as `SEMANTICS_NOT_PROVEN`.
- Most Preferences controls and Save/Reset.
- Diagnostics actions and current static badges/log text.
- Entire Updates action flow.
- All current dialog buttons.

A `PREVIEW_ONLY` label is not permission to infer a backend from a similarly named provider. Wiring requires exact domain semantics.

---

# 7. Functions that must not be connected on FA707NV

## 7.1 Confirmed absent

Task-supplied FA707NV evidence:

- `mini_led_mode` absent;
- `screen_auto_brightness` absent;
- AniMe absent;
- Slash absent.

Therefore UI controls for these capabilities must resolve to `UNSUPPORTED_ON_FA707NV`/hidden/disabled according to the final capability UX.

## 7.2 Power limits: files exist, safe control does not

The following attribute directories exist:

- `ppt_pl1_spl`;
- `ppt_pl2_sppt`;
- `ppt_pl3_fppt`;
- `nv_dynamic_boost`;
- `nv_temp_target`.

However `current/min/max/default` are empty because FA707NV is not present in the kernel DMI `power_limits` table. This is insufficient evidence for value semantics, units, safe bounds or defaults. The current percent-preview sliders must **not** be relabeled as W/°C or enabled as production controls.

Status: `SEMANTICS_NOT_PROVEN`.

## 7.3 Other controls that must not be wired by generic/raw fallbacks

- ACPI DEVS generic command/parameter sender;
- CPU/iGPU undervolt percentages;
- P/E core counts without a proven owner/range/read-back;
- generic Aura effect mode/speed writes;
- PCIe ASPM and Modern Standby policy toggles;
- Eco/Standard/Ultimate/Optimized mappings derived directly from MUX/access/power primitives.

A generic `SetArmouryAttribute(name,value)`, arbitrary sysfs path writer, raw ACPI/WMI command proxy, or sessiond privileged deputy would violate the current Hardware1 safety model.

---

# 8. Mutation ownership / security matrix

| Mutation | Current UI state | Caller path | Hardware1 method | Polkit action | Single mutation owner | Authoritative read-back | Result | Pending/reboot semantics |
|---|---|---|---|---|---|---|---|---|
| Performance profile | Fully wired | GUI → worker → AppService/composed provider → direct system bus | `SetPerformanceProfile` | `io.github.orbiscontrol.hardware.set-performance-profile` | kernel `platform_profile` writer in hardwared | hardwared fresh current symbol; AppService then fresh Session1 `PerformanceState` | `Applied` | None |
| Battery charge threshold | Fully wired | GUI → worker → AppService/composed provider → direct system bus | `SetChargeLimit` | `...set-charge-limit` | asusd setter; kernel is effective-state reader | `BatteryMutationReadback`: configured via asusd + effective via power_supply; then Session1 `ChargeLimit` | `Applied` | None |
| Fan custom curve | Code wired; UI blocked by read defect | GUI → worker → AppService → direct Hardware1 | `SetFanCurve` | `...set-fan-curve` | asusd `FanCurves` | fresh `FanCurveData(profile)` and complete selected-fan point match | `Applied` | None |
| Fan factory defaults | Code wired; UI blocked by read defect | GUI callback → worker FIFO (`WorkerCommand::ResetFanCurvesToDefaults`) → Hardware1 | `ResetFanCurvesToDefaults` | `...set-fan-curve` | asusd profile-wide reset | fresh `FanCurveData(profile)`, require recognized curves; UI then requests selected curve refresh; success is `Accepted`, not `Applied`, because independent default evidence is absent | `Accepted` | None; staging button itself performs no write |
| Panel Overdrive | Backend ready, UI not wired | Future GUI/application direct Hardware1 caller | `SetPanelOverdrive` | `...set-panel-overdrive` | asusd setter | fresh kernel `panel_overdrive/current_value`, exact bool match | `Applied` | None |
| Keyboard backlight | Backend ready, UI not wired | Future GUI/application direct Hardware1 caller | `SetKeyboardBacklight` | `...set-keyboard-backlight` | kernel LED class | fresh `max_brightness` validation + fresh `brightness` match after write | `Applied` | None |
| Aura Static RGB | Backend ready, UI not wired | Future GUI/application direct Hardware1 caller | `SetAuraStaticRgb` | `...set-aura-static-rgb` | asusd Aura | fresh `LedModeData` config-level RGB match | **`Accepted`** | None; **hardware RGB state is not authoritative because kernel `kbd_rgb_mode` is write-only** |
| Raw GPU backend mode | Deliberately disabled | Current product UI does not reach it in production | `SetGpuMode` | `...set-gpu-mode` | Would be supergfxd compatibility backend | Staged snapshot (`current_mode`, `pending_mode`, `pending_user_action`) | Backend contract supports applied/pending/user-action outcomes, but production backend currently returns Unsupported | May require logout/reboot at backend level; **must not be mapped to product Eco/Standard/Ultimate/Optimized until policy semantics are proven** |

Security invariant for all enabled interactive privileged mutations:

`original GUI/application system-bus caller` → `Hardware1` → polkit subject `system-bus-name` → one closed typed operation → authoritative read-back.

Sessiond is not an interactive privileged deputy.

---

# 9. Source-level defects / mismatches found

No defect is fixed by this audit.

## D1 — fan profile read composition is incompatible with the current UI command

Evidence:

- `crates/orbis-ui/src/composition.rs::build_production_runtime` constructs `SysfsFanCurveProvider`.
- `crates/orbis-ui/src/worker.rs::WorkerCommand::RefreshFanCurve` calls `fan_curve_for_profile`.
- `crates/orbis-sessiond/src/fans.rs::SysfsFanCurveProvider::fan_curve_for_profile` returns `Unsupported`.
- `crates/orbis-ui/src/main.rs` converts the error to `FanCurveHwState::Unavailable` + `fan_curve_error=true`.
- `crates/orbis-sessiond/src/fans.rs` separately contains typed `ZbusAsusdFanCurveSource::read_curves(profile)`, but current production UI composition does not use it.

Impact: fan read/edit/default UI is only partially wired despite real mutation backends.

## D2 — telemetry bypasses the intended Session1 read architecture

`crates/orbis-ui/src/composition.rs::build_production_runtime` directly constructs `SysfsTelemetryProvider` inside the application runtime. The data is real/read-only, but the caller path is currently GUI/application → sysfs, not GUI → Session1 → sessiond.

## D3 — fan active-curve read also lives directly in the GUI application composition

The application runtime directly instantiates `orbis_sessiond::fans::SysfsFanCurveProvider`. It is library reuse, not a Session1 RPC. This differs from the intended session-owned read boundary.

## D4 — `120 Hz + OD` combines two independent capabilities

`ui/audited/main-window.slint` represents 120 Hz and Panel Overdrive as one local chip. Backend source models display output/refresh and `PanelOverdrive` separately; only Panel Overdrive has a production mutation backend. Direct wiring of the compound control would create false semantics.

## D5 — Diagnostics collapses independent GPU concepts

The Diagnostics visual has one `GPU primitives` row. Production state has separate `GpuPower`, `GpuMux`, `GpuAccess`, and no `GpuProductPolicy`. A summary row is acceptable only if it does not imply a combined/product mode.

## D6 — AC power text has no current telemetry source

Main carries `power_ac_mw`, but `SysfsTelemetryProvider::snapshot()` deliberately sets `PowerTelemetry.ac=None`. The UI must not present a fabricated AC wattage.

---

# 10. Recommended next UI wiring slices

Ordering rule used here:

1. repair/use an already implemented backend with proven evidence;
2. then add low-risk read-only exposure;
3. only after that design genuinely new mutation semantics.

Each slice is intentionally small and independently reviewable.

## Slice 1 — restore authoritative profile-specific fan read

**UI controls:** Fans CPU/GPU selection, BIOS profile selector, graph read state; this unblocks existing curve edit/apply/defaults.  
**Existing backend:** typed asusd `FanCurveData(profile)` source already exists; Hardware1 fan mutations already exist.  
**New backend code:** **Yes, small unprivileged integration/provider work** is needed to make the production fan read service actually implement the profile-specific contract and route it through the intended read boundary. No new privileged mutation method.  
**Risk:** Low–medium; read-only composition change, but it gates safety-critical fan editor state.  
**Approximate scope:** one provider/composition/read-transport slice + tests for all four `AsusdFanProfile` values and CPU/GPU + UI error-state verification.

Acceptance condition: `RefreshFanCurve(Balanced/Performance/Quiet/LowPower, CPU/GPU)` returns authoritative profile data instead of `Unsupported`; no mutation path changes.

## Slice 2 — keyboard backlight brightness

**UI controls:** Extra → Backlight → Brightness.  
**Existing backend:** `AsusKeyboardBacklightProvider` + Hardware1 `SetKeyboardBacklight`; FA707NV keyboard backlight confirmed present.  
**New backend code:** No new privileged backend. **Unprivileged application/session capability transport + UI state/callback wiring is needed.**  
**Risk:** Low–medium; bounded hardware level, max read dynamically, authoritative read-back already implemented.  
**Approximate scope:** read model/capability probe integration, UiState fields, worker read, direct Hardware1 mutation provider/callback, tests.

## Slice 3 — split and wire Panel Overdrive

**UI controls:** replace/split the current `120 Hz + OD` compound so Panel Overdrive has its own stateful toggle; leave 60/120 refresh mutation disabled.  
**Existing backend:** `AsusArmouryPanelOverdriveProvider`, `probe_panel_overdrive`, Hardware1 `SetPanelOverdrive`; FA707NV confirmed present.  
**New backend code:** No new privileged backend. Unprivileged runtime/capability/UI integration is needed.  
**Risk:** Medium; the backend is narrow/proven, but current UI semantics must first be separated from refresh rate.  
**Approximate scope:** dedicated Panel OD UiState + read/capability + callback + existing Hardware1 provider + Applied/read-back tests.

## Slice 4 — Aura read + Static RGB

**UI controls:** Main Laptop Keyboard current mode/color display and `Color` action; do **not** enable generic effect-mode mutation.  
**Existing backend:** `AsusAuraProvider` read; Hardware1 `SetAuraStaticRgb`; FA707NV TUF Static RGB confirmed present.  
**New backend code:** No new privileged mutation backend. Needs application/read transport, capability gating and direct Hardware1 UI provider.  
**Risk:** Medium because success is `Accepted`, not hardware `Applied`; UI must use different confirmation wording/state.  
**Approximate scope:** Aura read UiState, supported-mode gating, RGB picker/value binding, `Accepted` result UX, tests that never label it hardware-confirmed.

## Slice 5 — read-only diagnostics + display-output observation

**UI controls:** Diagnostics capability/provider surfaces; optional Main screen observed refresh/status text, **not** the 60/120 setters.  
**Existing backend:** current capability registry + GPU primitive states + telemetry + `WaylandDisplayOutputProvider`; Panel/Keyboard/Aura read providers can join as they are integrated.  
**New backend code:** No privileged backend. Unprivileged application/session transport and diagnostics presentation are needed.  
**Risk:** Low; read-only. Main risk is semantic honesty (separate GPU primitives, no fake provider health, no guessed display control support).  
**Approximate scope:** typed diagnostics view model, read-only worker/session wiring, stale/unavailable states, no mutation callbacks.

### Later slices — explicitly after the top five

- Preferences/config persistence: connect only exact `UiConfig` semantics first (theme, then close/position where model is sufficient); do not pretend all visual Preferences already have config fields.
- Display refresh mutation: requires compositor/environment-specific production write backend; Wayland `wl_output` observation alone is insufficient.
- Automation: only after a scoped background-authorization/reconciliation design exists.
- Power limits: blocked on FA707NV semantic/range/default evidence.
- Product GPU modes: blocked until Eco/Standard/Ultimate/Optimized policy mapping is proven independently from physical MUX, dGPU access and runtime power.

---

# 11. Final classification by requested lists

## Really production-wired

- Silent / Balanced / Turbo.
- Battery charge limit slider / 100%.
- Real telemetry read surfaces.
- GPU MUX/access/runtime-power read paths internally, without product-mode synthesis.
- Fan custom/default mutation implementations are real, but current fan UI remains `WIRED_PARTIAL` because profile-specific read is broken in composition.

## `BACKEND_READY_NOT_WIRED`

Current interactive controls:
- Keyboard Color → Aura Static RGB.
- Extra Backlight Brightness → keyboard LED brightness.

Additional backend capabilities ready for a future UI slice:
- dedicated Panel Overdrive;
- Aura read state;
- Wayland output read-only;
- config storage foundation.

## `PREVIEW_ONLY`

- UI navigation/support/menu controls.
- fan profile CRUD, clamp/hysteresis/calibration.
- bindings editor.
- several Extra UI/system toggles.
- Automation shell/triggers/save/reset.
- most Preferences.
- Diagnostics actions/static preview.
- Updates shell.
- dialog actions.

## `UNSUPPORTED_ON_FA707NV`

- AniMe controls.
- Slash controls.
- MiniLED capability.
- Screen Auto Brightness capability.

## `SEMANTICS_NOT_PROVEN`

- product GPU Eco/Standard/Ultimate/Optimized;
- compound `120 Hz + OD`;
- flicker-free dimming;
- all current power-limit sliders/actions;
- undervolt/advanced fan-power controls;
- Aura device-state matrix;
- XG Mobile and GPU automation/lifecycle options;
- ASPM / Modern Standby / UMA / CPU cores / raw ACPI DEVS / optimal-brightness policies;
- privileged Automation actions until background security/reconciliation is designed.

---

# 12. Audit boundary and final HEAD check

The backend branch was fetched again immediately before this document commit and remained:

`chatgpt/production-hardening-20260818`  
`a745f559e1dc3aeefc665b31f740242bc4cb7348`

The UI branch was also rechecked and remained:

`agent/light-theme-toggle`  
`4a63c6b8f36688a8b81fc2f87a94eb2216d8ee56`

No source changes from either branch were copied into this audit branch. The intended diff is exactly one new file:

`docs/ui-backend-wiring-matrix.md`
