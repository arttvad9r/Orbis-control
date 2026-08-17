# Similar applications audit

## Scope, evidence rules, and exclusions

This dossier examines complete end-user control applications, with emphasis on
product shape, UI, application state, capability presentation, persistence and
background behavior. It is not a kernel ABI audit and it does not make an Orbis
architecture decision.

Claims are marked as follows:

- **SOURCE FACT** — directly visible in the pinned source or repository docs.
- **INFERENCE** — a bounded interpretation of source facts.
- **UNKNOWN** — not established by the inspected material.

No application was launched, no system service was started, no system/session
bus was contacted, and no hardware mutation was performed. Repository size is
reported as the number of tracked files in the shallow checkout, not as a
benchmark or LOC measurement.

## Pinned repositories

| Application | Repository | Revision | License evidence | Tracked files |
|---|---|---|---|---:|
| G-Helper | `https://github.com/seerge/g-helper` | `b9ba417f1b822110ce41a1e152031a84bfa6ad80` | GPL-3.0, repository `LICENSE` / GitHub repository metadata | 313 |
| TUXEDO Control Center | `https://github.com/tuxedocomputers/tuxedo-control-center` | `1a4d39ba5795e08e4e1ff6245ac908e2712262bf` | GPL notice in `src/service-app/classes/TuxedoControlCenterDaemon.ts:4-17` and repository `COPYING` | 436 |
| LACT | `https://github.com/ilya-zlobintsev/LACT` | `fabf7e4e7e111fc57db9c19825d93a879eaf3d9f` | MIT, repository `LICENSE` / GitHub repository metadata | 3859 |
| Lenovo Legion Linux | `https://github.com/johnfanv2/LenovoLegionLinux` | `2b5d6b4c9a3f3ea2702c38c9b1040c8d5d0a8e67` | GPL-2.0, repository `LICENSE` / GitHub repository metadata | 158 |
| CoreCtrl | `https://gitlab.com/corectrl/corectrl` | `21c017f4ce59797b27fd81047348f3855d3d41d0` | GPL-3.0-or-later, `src/qml/main.qml:1` | 788 |
| ROG Control Center (official ASUS Linux experimental app) | `https://gitlab.com/asus-linux/rog-control-center-slint` | `58c201efc651050fbdc71644418b806a153a911b` | MPL-2.0, repository `LICENSE` / GitLab metadata | 25 |
| G-Linux (ASUS Linux control-center application) | `https://github.com/NeuroMarshal/g-linux` | `99d2ec46f1bf45b9777feebbe5f55261f78f1d57` | GPL-3.0, GitHub repository metadata | 117 |
| ASUS Linux Control Center (community app) | `https://github.com/OsamaAlhasanat/asusctl-control-center` | `c32ea83e02e2fdda33de1ac14fe918053605257e` | GPL-3.0, GitHub repository metadata | 76 |

### Selection notes

**SOURCE FACT.** The official `rog-control-center-slint` repository is marked
archived and experimental by GitLab; it has 29 commits and no tags. It is
included because it is the named ASUS Linux control-center application, but its
small size and archived status limit what can be established from it.

**SOURCE FACT.** G-Linux is a separate, fuller current application in the
ASUS/asusd ecosystem. Its documentation describes a one-way graph from drivers,
asusd and sysfs through adapters, core policy, application services, a Qt facade
and QML/Kirigami (`docs/architecture.md:3-24`). It is therefore treated as the
primary current ASUS Linux application report, while the official archived app
is reported separately.

**SOURCE FACT.** LACT is primarily a GPU configuration and monitoring product,
but has a complete GUI, daemon, profiles, telemetry, fan control and service
lifecycle. CoreCtrl is also GPU/CPU-focused but has full profiles, telemetry UI
and system-tray behavior. Lenovo Legion Linux has a GUI and automation daemon,
but its repository describes the GUI as simple and also documents CLI/script
usage. These are included as adjacent full applications, not as equivalent
whole-laptop products.

**UNKNOWN.** This set does not establish that no other substantial comparable
open-source application exists for MSI, Acer, Dell/Alienware, Framework or
System76.

---

# G-Helper

## Product

**Repository:** `seerge/g-helper`  
**Upstream:** `https://github.com/seerge/g-helper`  
**Revision:** `b9ba417f1b822110ce41a1e152031a84bfa6ad80`  
**License:** GPL-3.0 (repository metadata and `LICENSE`)  
**Platform:** Windows; ASUS ROG, TUF, Zephyrus, Strix, Scar, ProArt,
Vivobook, Zenbook, Expertbook, ROG Ally and related devices.  
**Language/framework:** C#/.NET WinForms; native Windows APIs and ASUS ACPI/
HID paths. `app/GHelper.csproj` and the source tree contain WinForms designer
files and C# feature modules.  
**Approximate size:** 313 tracked files in this checkout; the repository page
reported 4,204 commits at inspection time.  
**Actively maintained:** **SOURCE FACT:** the pinned checkout has commit date
2026-08-11 and the repository page reported 4,204 commits, 72 open pull
requests and current releases. This is maintenance evidence only, not a
prediction.

## What the application is

G-Helper presents itself as a small Armoury Crate alternative for ASUS laptops.
It is a desktop application with a tray-first operating model rather than a
Linux daemon plus GUI. The README describes the app as a remote control for
manufacturer-defined BIOS/ACPI modes rather than a real-time hardware runtime.

**SOURCE FACT.** Startup creates a `SettingsForm`, mode and GPU controllers,
hardware overlay, ACPI object, tray icon, input dispatcher and power/session
event subscriptions (`app/Program.cs:21-45,97-105,115-181`). It can keep a tray
icon visible and hide the settings window rather than exit (`:136-152`; the
close-to-tray behavior is also represented by the application event flow).

## User-visible feature inventory

**SOURCE FACT.** Repository README feature inventory includes:

- Silent, Balanced and Turbo performance modes;
- per-mode fan curves, power limits and CPU boost selection;
- Eco, Standard, Ultimate and Optimized GPU modes;
- GPU MUX and dGPU enable/disable behavior;
- screen refresh rate, display overdrive, visual modes, flicker-free dimming,
  HDR-related controls and Mini-LED multi-zone selection;
- battery charge limit and full-charge override;
- CPU/GPU temperature, fan speed, battery status and GPU utilization/power
  monitoring;
- NVIDIA GPU clock/memory offsets, power and temperature controls;
- AMD CPU undervolting and temperature limits;
- AniMe Matrix and Slash lighting, animated GIF/clock/audio visualizer modes;
- Aura/RGB backlight modes and colors;
- Fn-lock, hotkeys, OSD and keyboard-related controls;
- XG Mobile control;
- ASUS mouse/peripheral controls;
- BIOS and driver update discovery;
- in-game overlay with FPS, CPU/GPU temperatures, usage and power;
- ROG Ally controller-related bindings.

Evidence: repository `README.md`, feature sections and screenshots; source
modules are visible in `app/AnimeMatrix`, `app/Battery`, `app/Display`,
`app/Fan`, `app/Gpu`, `app/Input`, `app/Mode`, `app/Overlay`, `app/Peripherals`,
`app/USB`, plus `app/Matrix.cs`, `app/Slash.cs`, `app/Updates.cs` and
`app/Handheld.cs`.

## UI / UX structure

**SOURCE FACT.** The main form is a compact dashboard-like WinForms surface.
`Settings.cs` names primary sections and controls for performance, GPU mode,
screen, keyboard, AniMe Matrix, battery, updates, fans/power and ROG Ally
(`app/Settings.cs:59-129,163-190`). The designer declares those panels and
buttons as the main visual structure (`app/Settings.Designer.cs:31-70`).

**SOURCE FACT.** Primary controls are direct mode buttons: Silent, Balanced,
Turbo, Fans/Power, Eco, Standard, Ultimate, Optimized, display presets, battery
slider/full-charge button, keyboard and Matrix controls (`Settings.cs:70-99,
163-201`). Advanced settings are opened as separate forms: `Fans`, `Extra`,
`Matrix`, `Slash`, `Updates` and `Handheld` (`Settings.cs:39-44`).

**SOURCE FACT.** The fan/power view contains separate chart series for CPU, GPU,
Mid and XGM fans; interactive chart mouse/keyboard handlers; power sliders;
GPU tuning sliders; hysteresis controls; Apply/Reset controls and calibration
(`Fans.cs:13-20,45-85,90-146,148-215`). This is a graph editor rather than a
plain numeric settings list.

**SOURCE FACT.** The UI does not use a general dashboard capability registry in
the inspected files. It hides GPU controls when ACPI GPU/MUX values are absent
(`Gpu/GPUModeControl.cs:24-29,48-67`), and labels unsupported custom fan curves
when writes fail (`Mode/ModeControl.cs:245-268`).

**UNKNOWN.** The inspected sources do not establish a single formal UX state
taxonomy for all unavailable, pending and failed controls.

## Application architecture

```text
WinForms settings/tray/overlay
  -> feature controllers and AppConfig
  -> AsusACPI / HID / Windows native APIs / vendor GPU APIs
  -> BIOS/ACPI, ASUS services, NVIDIA/AMD APIs
```

**SOURCE FACT.** `Program` owns global controller instances and subscribes to
Windows power, session, suspend/resume and device events (`Program.cs:21-45,
156-185`). Feature code is split by user area: `BatteryControl`, `ModeControl`,
`GPUModeControl`, `FanSensorControl`, display, USB, peripherals and overlay.

**SOURCE FACT.** G-Helper has no separate backend daemon in the inspected
repository. It can interact with ASUS Windows services and explicitly stops or
restarts named ASUS/Armoury services (`Helpers/AsusService.cs:7-33,60-118`),
but the application itself owns the orchestration.

**INFERENCE.** The privilege boundary is mixed: ordinary app operations use
the desktop process, while some operations reschedule or invoke elevated
helpers/services. `BatteryControl.SetAsusChargeLimit` checks administrator
status before registry mutation (`Battery/BatteryControl.cs:50-63`).

## Capability handling and state

**SOURCE FACT.** Device/model checks are distributed through `AppConfig` and
feature controllers. Fan maximum defaults contain model-specific tables and
fallback defaults (`Fan/FanSensorControl.cs:38-87`). GPU initialization reads
Eco and MUX values, hides modes when both are absent, and persists the detected
mode (`Gpu/GPUModeControl.cs:24-70`).

**SOURCE FACT.** GPU mutation distinguishes immediate Eco/Standard changes from
Ultimate/MUX changes requiring restart. It asks for confirmation, locks controls,
waits for mode application, may kill GPU applications, writes Eco/MUX values,
refreshes state, restarts NVIDIA services, recreates GPU control and reapplies
performance (`GPUModeControl.cs:80-160,164-225`).

**INFERENCE.** G-Helper visibly models current mode, a control-locked/applying
period, and restart-required paths, but the model is represented by controller
flags and UI methods rather than one public state enum.

**SOURCE FACT.** Battery limit uses a configured limit, clamps some models to
60/80/100 behavior, writes ACPI and persists the selected value in `AppConfig`
(`Battery/BatteryControl.cs:65-86`).

**SOURCE FACT.** Performance mode application is asynchronous and cancellation-
aware. It updates the selected mode, cancels the previous task, applies ACPI
mode, GPU clocks, fans and power in sequence, and logs cancellation or failure
(`Mode/ModeControl.cs:120-203`).

**UNKNOWN.** A durable “failed” or “rollback” state visible to the user after
all asynchronous failures is not established by the inspected code.

## Mutation UX

| Control | Observed behavior |
|---|---|
| Performance | Button selection updates the form and starts an asynchronous sequence; a toast can be emitted; mode-toggle hotkeys use a delay timer (`ModeControl.cs:120-203,215-228`). |
| Fan curve | Chart editing changes points; Apply controls exist; mode application writes curves and falls back to a BIOS/base mode when custom curves fail (`Fans.cs:100-175`; `ModeControl.cs:231-294`). |
| Battery | Limit is validated/clamped in code and written immediately by the control method; configuration is saved through `AppConfig` (`BatteryControl.cs:65-86`). |
| GPU Eco/Standard | Asynchronous apply, temporary lock, refresh and service recreation (`GPUModeControl.cs:164-225`). |
| GPU MUX/Ultimate | Confirmation dialog, immediate restart command (`shutdown /r /t 1`) after queueing the MUX change (`GPUModeControl.cs:97-158`). |

**SOURCE FACT.** A display mode change also uses a message box stating that a
reboot is required (`Display/ScreenControl.cs:144`).

## Profiles and automation

**SOURCE FACT.** The README describes Silent/Balanced/Turbo as BIOS-backed modes,
with configurable fan curves and power limits per mode. `ModeControl` reads
per-mode settings, applies fan/power/GPU tuning and can run a mode command
(`ModeControl.cs:92-101,120-203`).

**SOURCE FACT.** Automation covers AC/battery performance mode, Optimized GPU
mode, screen refresh and keyboard timeout according to the repository README.
The application subscribes to power and session events (`Program.cs:163-181`).

**SOURCE FACT.** The main config is JSON under `%APPDATA%/GHelper/config.json`,
with startup/common-data fallbacks, lenient parsing, broken-config recovery,
debounced writes, atomic replacement and `.bak` backup (`AppConfig.cs:8-16,
27-43,46-108`).

**INFERENCE.** G-Helper profiles are primarily a collection of keyed settings
inside one application config, not an independent profile object graph with a
separate profile service.

## Telemetry and background behavior

**SOURCE FACT.** GPU temperature/clock/utilization reads use GPU helpers; fan
sensor calibration uses a 1-second timer and records measured maxima
(`Fan/FanSensorControl.cs:22-35,137-180`). Other feature modules use timers such
as the 300 ms Ally loop and 1-second peripheral/Aura loops (for example
`Ally/AllyControl.cs:318-362`; `USB/Aura.cs:123-133,274`).

**SOURCE FACT.** The app has a tray icon, can remain in the background, handles
power/session/suspend events and shows an OSD/overlay (`Program.cs:136-181`).

**UNKNOWN.** A historical telemetry database or chart history beyond live UI
and overlay is not established for G-Helper.

## Code organization

```text
app/
  Settings.cs / Settings.Designer.cs   main dashboard
  Program.cs                            startup, tray, events
  Mode/                                 performance and power
  Battery/                              charge limit
  Fan/                                  sensors and fan support
  Gpu/                                  GPU mode and vendor API
  Display/                              refresh, brightness, visual modes
  AnimeMatrix/, USB/, Peripherals/      device-specific features
  Helpers/, Input/, Overlay/, UI/       background, hotkeys, presentation
```

The organization is feature-oriented rather than layered into explicit GUI,
application, provider and daemon crates.

## Notable implementation/product ideas

- **SOURCE FACT:** one compact primary surface combines quick performance, GPU,
  screen, battery and lighting controls (`Settings.cs:70-99`).
- **SOURCE FACT:** advanced fan/power editing is moved to a dedicated chart-
  based form (`Fans.cs:45-85,90-146`).
- **SOURCE FACT:** GPU mode changes that require restart use confirmation and a
  visible locked/applying phase (`GPUModeControl.cs:97-158,164-225`).
- **SOURCE FACT:** configuration recovery parses surviving key/value pairs from
  malformed JSON and writes atomically with a backup (`AppConfig.cs:46-108`).
- **SOURCE FACT:** model-specific limits and fallbacks are distributed through
  feature code (`FanSensorControl.cs:50-87`).

---

# ROG Control Center / ASUS Linux applications

## Official archived ROG Control Center (Slint)

**Repository:** `asus-linux/rog-control-center-slint`  
**Revision:** `58c201efc651050fbdc71644418b806a153a911b`  
**License:** MPL-2.0  
**Platform:** Linux; ASUS Linux ecosystem; experimental and archived.  
**Language/framework:** Rust + Slint; `rog_dbus` proxies.  
**Approximate size:** 25 tracked files in the pinned checkout.  
**Actively maintained:** **SOURCE FACT:** GitLab marks it archived, with 29
commits and no tags. No further maintenance conclusion is made.

### Product and features

**SOURCE FACT.** The UI pages are Home/Supported, Profile & Fan Control,
Keyboard, AniMe and System (`ui/main.slint:1-5,81-100`). The System page binds
controls for post sound, dedicated graphics/GSYNC, panel overdrive, LEDs, AniMe
and background-run/startup flags (`main.slint:30-64`).

**SOURCE FACT.** The profile/fan page is explicitly titled “Profile & Fan
Control” (`ui/pg_profile_fans.slint:4-25`). The System page is driven by a
supported-function response. Unsupported features receive the literal text
“This feature is unsupported by your hardware” in `src/main.rs:136-149` and
capability booleans control individual fields (`:151-220`).

### UI / architecture / state

```text
Slint window and page bindings
  -> Rust signal handlers and setup_system_page
  -> rog_dbus::DbusProxies / Signals
  -> asusd-family D-Bus services
```

**SOURCE FACT.** The app creates D-Bus proxies and a blocking signal-receive
thread, then starts signal watchers (`src/main.rs:83-104`). It uses a FIFO in a
temporary directory for single-instance/background GUI signaling (`:26-54,
72-80,114-133`). Config is a small JSON object with `run_in_background` and
`startup_in_background`, stored at the XDG config directory under
`rog/control-center.cfg` (`src/config.rs:8-15,17-75`).

**SOURCE FACT.** `setup_system_page` reads supported functions, reads current
values, sets enabled flags and attaches callbacks that immediately call D-Bus
methods (`src/main.rs:136-220`). Error handling in these callback examples is
mostly `.ok()` and the code comments identify warnings as TODO.

**UNKNOWN.** The archived source does not establish a complete telemetry model,
profile persistence model, fan-curve editor behavior, reboot UX or durable
mutation state.

## G-Linux current ASUS Linux application

**Repository:** `NeuroMarshal/g-linux`  
**Revision:** `99d2ec46f1bf45b9777feebbe5f55261f78f1d57`  
**License:** GPL-3.0  
**Platform:** Linux desktop, ASUS ROG, Qt 6/QML/Kirigami.  
**Language/framework:** C++20 core/backend/application and Qt/QML UI.  
**Approximate size:** 117 tracked files in the pinned checkout.  
**Actively maintained:** **SOURCE FACT:** pinned commit date is 2026-08-12
according to the shallow checkout; repository source includes tests and
documentation. Maintenance trajectory beyond that evidence is UNKNOWN.

### What the application is and feature inventory

**SOURCE FACT.** The documented QML pages are Dashboard, Fans and Power,
Peripherals, ASUS Devices, Automation and Diagnostics (`docs/code-map.md:55-67`).
The capability enum includes daemon, platform/power profiles, battery and
charge limit, GPU Standard/Eco/Ultimate/Optimized, Aura, AniMe, Slash lighting,
panel overdrive, boot sound, CPU core control, power tuning, EPP, GPU clock
offsets, fan curves, display refresh, Adaptive Sync, Mini-LED, eGPU,
automatic profiles and peripherals (`src/core/device/capability.hpp:10-40`).

**SOURCE FACT.** The documented application areas include Aura, display refresh
and Adaptive Sync, fan orchestration, GPU firmware modes and NVIDIA tuning,
ASUS platform devices, power profiles/charge/EPP/tuning, diagnostics, hotkeys,
tray and ASUS USB devices (`docs/architecture.md:62-89`; `docs/code-map.md:39-52`).

### UI / UX

**SOURCE FACT.** `Main.qml` starts with Dashboard and loads Fans, Peripherals,
Devices, Automation or Diagnostics pages conditionally through
`NavigationPolicy.js` (`qml/Main.qml:18-55`). The window changes size by page;
Dashboard is compact at 430×760, while Fans and Peripherals get wider layouts
(`:53-75`). Closing can hide to tray (`:83-90`).

**SOURCE FACT.** Dashboard presents profile mode buttons, CPU temperature and
fan status, GPU confirmation dialogs, eGPU confirmation, keyboard color dialogs
and an inline error message when asusd is unavailable (`qml/pages/DashboardPage.qml:24-83,138-175`).
The GPU confirmation explicitly says restart and describes session close and
verification (`:50-66`).

**SOURCE FACT.** Fans page keeps draft power limits, clock offsets, EPP and fan
editor dirty state; it has Apply sequencing, waits for GPU apply completion and
then applies fan drafts (`qml/pages/FansPage.qml:7-23,52-161`).

**SOURCE FACT.** Diagnostics is a page with hardware report, application-log
tail, refresh and copy controls (`qml/pages/DiagnosticsPage.qml:7-71`).
Automation exposes AC and battery profile selectors only when the capability is
available (`qml/pages/AutomationPage.qml:20-64`).

### Application architecture

```text
QML / Kirigami
  -> SystemController Qt facade
  -> application services
  -> core policies and typed capability registry
  -> backend adapters (IAsusdClient D-Bus, ILinuxSystem snapshots,
     ISystemPower/logind, KAuth NVIDIA helper)
  -> asusd / sysfs / desktop services / firmware
```

**SOURCE FACT.** This graph is stated in `docs/architecture.md:3-24`. The
documentation says QML contains no D-Bus or sysfs paths (`:22-24`), core is
independent of Qt/KDE/D-Bus/filesystem (`:26-37`), and backend adapters own
their respective wire/system contracts (`:45-60`).

**SOURCE FACT.** The code map separates `glinux-core`, asusd backend, Linux
backend, system backend, application services, production QML and a narrow
NVIDIA KAuth helper (`docs/code-map.md:3-22`).

### Capability and state model

**SOURCE FACT.** Capabilities have `Access` values Unavailable, ReadOnly,
Writable and Queued, plus `Evidence` values None, VerifiedQuirk,
ExactDeviceConfig and Live (`src/core/device/capability.hpp:42-74`). Queued
means restart-required (`:70-73`). Device specification carries identity,
capability registry, platform profiles, Aura, Mini-LED, tuning ranges and
display information (`src/core/device/devicespecification.hpp:11-57`).

**SOURCE FACT.** The GPU service has explicit apply statuses including Busy,
ExistingQueue, UnsupportedValue, state-read/write/verification failures,
reboot-request failure and rollback failure (`src/application/gpu/gpumodeservice.hpp:15-36`).
It snapshots previous values, tracks queued attributes and has rollback state
(`:51-68`).

**SOURCE FACT.** The documented GPU flow validates live choices, snapshots the
current firmware value, writes and verifies a queued value, requests reboot and
clears or restores the queue if reboot request fails (`docs/code-map.md:83-97`;
`docs/architecture.md:75-79`).

**SOURCE FACT.** Missing live interfaces disable the feature; product family
names alone do not enable controls (`docs/architecture.md:91-100`). The QML
navigation policy also removes unavailable pages (`qml/Main.qml:20-49`).

### Profiles, telemetry, background and persistence

**SOURCE FACT.** Profiles aggregate performance profile, charge, EPP, fan,
GPU, display, lighting and automation-related capabilities according to the
Dashboard, Fans and Automation page composition (`qml/pages/DashboardPage.qml`,
`FansPage.qml`, `AutomationPage.qml`). AC/battery profile selection is explicit
in Automation (`AutomationPage.qml:24-64`).

**SOURCE FACT.** Telemetry is represented in Dashboard status labels and live
snapshots; Diagnostics exposes a hardware report and log tail. The code map
describes `SystemController::refresh` reading asusd managed objects, Linux
snapshots and domain reads before publishing Qt properties/signals
(`docs/code-map.md:69-81`).

**SOURCE FACT.** A tray manager and hotkey manager exist in the production
source map (`docs/code-map.md:47-52`); exact startup service persistence is not
established by the inspected excerpts.

**UNKNOWN.** Exact config file format and migration/versioning were not
established from the inspected G-Linux files.

### Mutation UX and notable ideas

- **SOURCE FACT:** GPU mode has a confirmation dialog that names restart and
  explains session close (`DashboardPage.qml:50-66`).
- **SOURCE FACT:** custom fan/power edits use draft state and dirty flags rather
  than writing every slider movement (`FansPage.qml:7-23,79-145`).
- **SOURCE FACT:** capability access distinguishes unavailable, read-only,
  writable and queued (`capability.hpp:42-74`).
- **SOURCE FACT:** GPU failures include verification, reboot and rollback
  categories (`gpumodeservice.hpp:15-36`).
- **SOURCE FACT:** QML page availability is capability-driven and asusd absence
  appears as an inline disabled-controls message (`Main.qml:37-49`;
  `DashboardPage.qml:144-151`).

## ASUS Linux Control Center community app

**Repository:** `OsamaAlhasanat/asusctl-control-center`  
**Revision:** `c32ea83e02e2fdda33de1ac14fe918053605257e`  
**License:** GPL-3.0  
**Platform:** Linux desktop; PyQt6; ASUS Linux ecosystem.  
**Language/framework:** Python 3 + PyQt6 widgets; `asusctl` and optional
`supergfxctl` CLI backends.  
**Approximate size:** 76 tracked files.  
**Actively maintained:** **SOURCE FACT:** pinned checkout commit date is
2026-08-11 and the repository contains tests/docs. GitHub metadata reports a
small repository and no meaningful public activity count beyond that snapshot.

### Product and UI

**SOURCE FACT.** The app has Overview, Performance, Hardware, Diagnostics and
Settings pages. `MainWindow` is a thin shell with a fixed 180-pixel sidebar,
stacked pages, scroll wrappers, status bar and toast overlay
(`src/.../ui/main_window.py:1-5,58-78,84-113,115-220`).

**SOURCE FACT.** Overview shows device identity, services, active profile,
capability indicators, warnings and refresh/copy-diagnostics actions
(`ui/pages/overview.py:28-58,60-138`). The capability panel names profiles, fan
curves, keyboard brightness, Aura, battery charge limit and graphics switching
(`:94-113`).

**SOURCE FACT.** An unavailable feature is represented by an explicit dashed
placeholder with “Not available” and an explanation, not only by hiding the
page (`ui/widgets/unavailable_notice.py:1-54`).

### Architecture and capability model

```text
PyQt6 pages and MainWindow
  -> ControlCenterController / ControlService
  -> AsusCtlBackend / SupergfxCtlBackend / read-only firmware backend
  -> CLI subprocesses, optional D-Bus-backed daemons, read-only sysfs
```

**SOURCE FACT.** The project architecture document explicitly lists PyQt6,
`asusctl` CLI, optional `supergfxctl` CLI, read-only sysfs inspection and an
optional Node wrapper (`docs/ARCHITECTURE.md:3-10`). It separates models,
settings, detection, controller, diagnostics, command runner and UI modules
(`:12-57`).

**SOURCE FACT.** `ControlService.build_snapshot` constructs integration state
from binary/service/bus checks and then reads profiles, fan curve, battery,
keyboard, Aura and graphics state (`services/detection.py:23-85`). The models
contain `supported`, messages, active/current values, pending graphics mode and
pending action (`models.py:44-126`).

**SOURCE FACT.** Warnings explain missing `asusd`, unavailable battery support,
model-dependent fan curves and a non-ready optional `supergfxd`; the graphics
warning says switching requires daemon, system-bus policy and root-level
installation (`services/detection.py:88-129`).

### Mutation UX, profiles and persistence

**SOURCE FACT.** Profile and fan curve can be applied as one service operation,
but the implementation reports a partial result when profile succeeds and curve
application fails (`services/detection.py:142-164`).

**SOURCE FACT.** Settings are JSON-backed with XDG-aware paths and include last
page, selected fan/profile, custom curves, presets, Aura settings, window size
and theme (`models.py:140-153`; `docs/ARCHITECTURE.md:27-33`).

**INFERENCE.** This app makes integration health and capability status first-
class dashboard content. It does not treat all failures as a generic disabled
button.

**UNKNOWN.** The inspected files do not establish a background daemon owned by
this application; its backend dependencies are external services/CLIs.

---

# TUXEDO Control Center

## Product

**Repository:** `tuxedocomputers/tuxedo-control-center`  
**Revision:** `1a4d39ba5795e08e4e1ff6245ac908e2712262bf`  
**License:** GPL-3.0 notice and `COPYING`  
**Platform:** Linux TUXEDO laptops.  
**Language/framework:** Angular renderer + Electron main process + Node 24
root daemon; TypeScript; system D-Bus.  
**Approximate size:** 436 tracked files.  
**Actively maintained:** **SOURCE FACT:** pinned commit date is 2026-08-10,
repository page reported 2,907 commits and 45 pull requests. This records
activity evidence only.

## What the application is and feature inventory

**SOURCE FACT.** README describes control of CPU cores, fan speed, performance,
energy, fan and comfort settings. The source tree includes dashboard/system
monitor, profile manager, charging settings/profiles, CPU and GPU information,
fan charts/custom fan chart, keyboard backlight, display brightness/refresh,
webcam settings, tools, global settings, shutdown timer, Prime graphics mode,
TUXEDO-specific devices and diagnostics/support pages.

**SOURCE FACT.** The profile model aggregates display brightness/refresh/
resolution, CPU cores/frequency/governor/EPP/turbo, webcam status, fan profile
and custom curve, ODM profile/power limits and NVIDIA power control
(`src/common/models/TccProfile.ts:23-57,61-109`).

## UI / UX structure

**SOURCE FACT.** Angular routing separates `cpu-dashboard`, profile manager,
support, info, tools, keyboard backlight, camera settings, global settings,
Aquaris control, Tomte GUI and webcam preview (`src/ng-app/app/app-routing.module.ts:53-129`).

**SOURCE FACT.** Dashboard is a “System monitor” card with active profile and
gauges for CPU temperature, CPU frequency, CPU fan, CPU power and conditional
iGPU/dGPU sections (`dashboard/dashboard.component.html:20-167`). Missing
values mark gauges `not-available` and attach a compatibility tooltip
(`:53-57,99-103,123-127`).

**SOURCE FACT.** The repository README includes screenshots for Systemmonitor,
Dark Theme, Tools, mains/battery, Profiles, Profile Settings and About. The
source includes dedicated fan chart and custom fan chart components, plus chart
themes.

**SOURCE FACT.** The application contains profile conflict, waiting, choice,
confirmation and error-oriented components, and compatibility service methods
such as `hasFanControl`, `hasCpuPower`, `hasDGpuFan`, `hasODMProfileControl` and
`hasODMPowerLimitControl` (`src/ng-app/app/compatibility.service.ts` search
results and component tree). The exact visual presentation for every case is
not fully reconstructed here.

## Application architecture

```text
Angular UI
  -> Electron preload/backend APIs and D-Bus client
  -> system D-Bus / tccd API
  -> root Node tccd daemon
  -> worker classes, tuxedo-drivers, sysfs, native TuxedoIO and desktop APIs
```

**SOURCE FACT.** README project structure explicitly separates `ng-app` Angular
GUI, `e-app` Electron main, `service-app` Node daemon and common models/classes.
The setup instructions identify `tccd.service`, a system D-Bus policy file and
separate GUI startup.

**SOURCE FACT.** The daemon requires root (`TuxedoControlCenterDaemon.ts:90-100`),
loads settings/profiles/webcam/fan-table paths, creates workers for charging,
state switching, display, CPU, webcam, fan, GPU info, power, Prime, D-Bus,
ODM profile/limits and listeners (`:61-87,107-130`). Each worker runs at its
own interval (`:137-157`).

**SOURCE FACT.** The D-Bus service requests `com.tuxedocomputers.tccd`, exports
`/com/tuxedocomputers/tccd`, reports initialization/export/name errors and
unexports on exit (`classes/TccDBusService.ts:26-89`).

## Capability model and state

**SOURCE FACT.** `CompatibilityService` checks individual fan, sensor, power,
GPU, ODM and profile conditions, with a generic “feature currently not
available” compatibility message when data is absent. Dashboard conditionally
renders iGPU/dGPU areas and marks unavailable gauges.

**SOURCE FACT.** `DaemonWorker` stores `previousProfile` and `activeProfile`,
updates the active profile and records the previous value after each worker
operation (`classes/DaemonWorker.ts:23-57`). The daemon has explicit state
switching, charging worker and per-worker timers.

**INFERENCE.** TCC distinguishes configured profile, active profile and prior
profile at worker level, while UI capability checks independently decide what
can be shown.

**UNKNOWN.** A single application-wide enum covering requested/applying/
effective/pending/failed is not established in the inspected source.

## Mutation UX, profiles and background behavior

**SOURCE FACT.** The route tree and profile model show a profile-centric product:
profile manager, profile details editing, active profile on dashboard and
settings that span CPU, fan, display, webcam, ODM and NVIDIA controls.

**SOURCE FACT.** Fan UI has chart components and a fan worker. The inspected
worker/service architecture runs independently of the Electron window through
`tccd.service`; the daemon starts workers and continues interval work after GUI
startup details are separate.

**SOURCE FACT.** Configuration ownership is daemon-side through `ConfigHandler`
and paths for settings, profiles, webcam, V4L2 names and fan tables
(`TuxedoControlCenterDaemon.ts:61-87`).

**UNKNOWN.** The exact commit/apply/rollback semantics for every Angular control
were not fully traced in this audit.

## Notable implementation/product ideas

- **SOURCE FACT:** a single profile carries display, CPU, webcam, fan, ODM and
  NVIDIA settings (`TccProfile.ts:23-57`).
- **SOURCE FACT:** dashboard combines active profile, CPU gauges and conditional
  GPU gauges (`dashboard.component.html:20-167`).
- **SOURCE FACT:** individual daemon workers have independent intervals and
  profile update hooks (`TuxedoControlCenterDaemon.ts:107-157`; `DaemonWorker.ts:23-57`).
- **SOURCE FACT:** the product exposes a service setup dialog and connection/
  service/version status fields (`service_setup.ts:24-110`).
- **SOURCE FACT:** capability absence is reflected in compatibility state and
  “not available” gauge presentation rather than assumed support.

---

# LACT

## Product

**Repository:** `ilya-zlobintsev/LACT`  
**Revision:** `fabf7e4e7e111fc57db9c19825d93a879eaf3d9f`  
**License:** MIT  
**Platform:** Linux desktop; AMD, NVIDIA and Intel GPU systems.  
**Language/framework:** Rust; GTK4/libadwaita/Relm4 GUI, Rust daemon/client,
newline-delimited JSON IPC.  
**Approximate size:** 3,859 tracked files.  
**Actively maintained:** **SOURCE FACT:** pinned commit date is 2026-08-12,
repository page reported 1,223 commits and 11 pull requests.

## User-visible feature inventory

**SOURCE FACT.** README lists detailed GPU identity/VBIOS/VRAM/Vulkan reporting,
historical power/thermal/frequency charts, throttling information and CSV
export; power cap and AMD power states; AMD/NVIDIA fan curves; AMD firmware
thermal/acoustic targets; GPU/VRAM clocks, AMD voltage offset and NVIDIA VF
curve undervolting; profiles with process/GameMode automatic activation; and an
OpenTelemetry metrics exporter.

**SOURCE FACT.** The GUI source has pages for Information, Overclocking,
Thermals, Software and Displays, plus graphs, process monitor, preferences,
profiles, GPU selector, overdrive dialog and service setup
(`lact-gui/src/app.rs:1-13,105-137`; `lact-gui/src/app/pages` tree).

## UI / UX

**SOURCE FACT.** The main window uses an Adwaita `NavigationSplitView`, toast
overlay and a sidebar; it owns GPU selector, profile selector, information,
overclocking, thermals, software, displays and crash-page controllers
(`lact-gui/src/app.rs:105-137,171-190`).

**SOURCE FACT.** Thermals page includes GPU stats, thermal threshold sections,
fan-control section, automatic/curve/static modes and a `FanCurveFrame`
(`lact-gui/src/app/pages/thermals_page.rs:42-59,101-160`).

**SOURCE FACT.** Profiles are a popover with auto-switch checkbox, list, add
and import buttons, with profile edit/export/rename messages
(`lact-gui/src/app/profiles.rs:29-47,49-136`).

**SOURCE FACT.** The README screenshots document GPU info, overclocking, fan
control, software info and historical data. Exact pixel layout beyond source
and screenshots is not inferred.

## Application architecture

```text
GTK4/libadwaita/Relm4 GUI
  -> lact-client request/response connection
  -> lactd Unix socket (optional unauthenticated TCP mode)
  -> daemon Handler and GPU controllers
  -> AMD sysfs / NVML / Intel GPU interfaces / desktop D-Bus
```

**SOURCE FACT.** The daemon creates a Unix listener, optionally TCP listener,
handler, socket permissions and optional metrics exporter
(`lact-daemon/src/server.rs:35-67`). Unix clients receive peer credentials and
the client PID in context; TCP clients do not (`:70-119`).

**SOURCE FACT.** Requests are newline-delimited JSON and malformed requests
produce structured error responses (`server.rs:131-164`). Client connections
report disconnects and support configurable reconnect (`lact-client/src/lib.rs:42-109`).

**SOURCE FACT.** The README says the system service does not depend on a
graphical session, can run headless with a config file, and reloads settings on
system resume using logind D-Bus. It also explicitly warns that enabled TCP
remote management has no authentication or encryption.

## Capability, state and persistence

**SOURCE FACT.** `FanControlSettings` includes mode, static speed, temperature
key, interval, curve, spindown delay, change threshold and automatic threshold
(`lact-schema/src/config.rs:186-227`). `Profile` contains per-GPU configs,
process/GameMode rule and activation/deactivation hooks (`:12-33`). GPU config
contains fan control, firmware thermal options, power cap, clocks, power mode
and power states (`:35-56`).

**SOURCE FACT.** GUI tracks reconnecting/UI sensitivity, daemon client, selected
GPU/profile, stats task and service setup (`lact-gui/src/app.rs:105-137`).
Profiles and config types are serialized/deserialized Rust structures.

**INFERENCE.** LACT has explicit transport connection states and profile rule
state. A general hardware mutation state machine is not visible in the cited
UI excerpts.

**SOURCE FACT.** Socket location changes for root/user, existing sockets are
rejected, umask is set and optional admin user/group ownership is applied
(`lact-daemon/src/socket.rs:19-87`).

## Background, automation and notable ideas

- **SOURCE FACT:** daemon independence from the GUI is explicit in README.
- **SOURCE FACT:** process/GameMode profile rules are represented in schema and
  profile UI (`config.rs:12-27`; `profiles.rs:37-47`).
- **SOURCE FACT:** telemetry has configurable historical charts and CSV export
  in README; GUI has graph-window and plot components.
- **SOURCE FACT:** a service setup dialog can connect, start, restart or stop
  the daemon and show service status/logs (`service_setup.rs:24-49,58-110`).
- **SOURCE FACT:** on Unix sockets the daemon captures peer credentials, while
  README warns TCP mode has no authentication or encryption.

---

# Lenovo Legion Linux

## Product

**Repository:** `johnfanv2/LenovoLegionLinux`  
**Revision:** `2b5d6b4c9a3f3ea2702c38c9b1040c8d5d0a8e67`  
**License:** GPL-2.0  
**Platform:** Linux Lenovo Legion laptops.  
**Language/framework:** Python + PyQt6 GUI/CLI, shell tooling, external kernel
module and optional `legiond` service.  
**Approximate size:** 158 tracked files.  
**Actively maintained:** **SOURCE FACT:** pinned commit date is 2026-08-07,
repository page reported 946 commits and 8 pull requests.

## Product and feature inventory

**SOURCE FACT.** README lists power modes, custom fan curves up to ten points,
CPU/GPU/IC temperature inputs, RPM and PWM values, acceleration/deceleration,
minimum hysteresis temperatures, fan presets, fan-controller lock, battery
conservation and rapid charging, Fn lock, touchpad/camera controls, display
overdrive, LEDs, hybrid/GSync mode, CPU/GPU overclocking and telemetry.

**SOURCE FACT.** The project also documents quiet/balanced/performance modes,
custom CPU/GPU power control, AC/battery fan presets and automatic profile
switching through `legiond`.

## UI / UX and mutation flow

**SOURCE FACT.** `legion_gui.py` uses a PyQt6 `QMainWindow`, tabs, system tray,
groups, combo boxes, sliders, checkboxes, charts/widgets and a thread pool
(`python/.../legion_gui.py:12-17`).

**SOURCE FACT.** The fan GUI has explicit “Read from HW”, “Apply to HW”, preset
load/save and Apply semantics in README. Loading a preset only changes displayed
values; hardware changes after Apply. Presets are YAML under
`/root/.config/legion_linux/` according to README.

**SOURCE FACT.** The fan GUI starts as root according to README. The GUI source
uses a `MonitorWorker` with a 10-second loop and desktop notifications
(`legion_gui.py:76-107`). Enum controls read available values, set the feature,
and later verify/update the UI (`:149-180`).

## Architecture and capability handling

```text
PyQt6 GUI / CLI / legiond
  -> LegionModelFacade and FileFeature abstractions
  -> sysfs/debugfs/hwmon and legion kernel module
  -> EC/ACPI firmware
```

**SOURCE FACT.** README describes a kernel module implementing sysfs, debugfs and
hwmon interfaces, plus standard tools such as psensor. It separately describes
the Python GUI, CLI and `legiond` automation daemon.

**SOURCE FACT.** The GUI imports `LegionModelFacade`, `FanCurve`,
`FileFeature`, `IntFileFeature`, `GsyncFeature` and diagnostics types
(`legion_gui.py:21-24`). The repository documents allowlists and models whose
fan control is not working, with other features potentially working.

**INFERENCE.** Capability handling is feature/file existence and model/BIOS
compatibility driven. It is more explicit in the README and feature wrappers
than in one central capability registry.

**SOURCE FACT.** Fan mutation is staged: read hardware, edit in GUI, Apply to
HW. The README says hardware may reset curves on power-mode change, suspend or
restart; the user must apply again.

**UNKNOWN.** A unified requested/effective/pending state model is not
established.

---

# CoreCtrl

## Product

**Repository:** `corectrl/corectrl`  
**Revision:** `21c017f4ce59797b27fd81047348f3855d3d41d0`  
**License:** GPL-3.0-or-later  
**Platform:** Linux systems with AMD GPU/CPU support and related GPU controls.
  
**Language/framework:** C++/Qt Quick QML.  
**Approximate size:** 788 tracked files.  
**Actively maintained:** **SOURCE FACT:** revision and repository metadata were
recorded; long-term maintenance state is UNKNOWN from the inspected source.

## Product and UI

**SOURCE FACT.** The main window is a two-tab application: `Profiles` and
`System`, with a footer TabBar and system-tray close behavior
(`src/qml/main.qml:11-58`).

**SOURCE FACT.** Profiles use a `PROFILE_MANAGER` object with profile add/remove/
save/change/active/manual-toggle signals; QML tracks `unappliedSettings` and
`unsavedSettings` separately (`src/qml/Profiles.qml:10-33,154-179`).

**SOURCE FACT.** GPU view contains a sensor graph and dynamically added controls;
the graph split height is persisted as a UI setting (`src/qml/GPUForm.qml:14-45,
47-93`). The QML tree includes CPU forms, GPU form, curve control, sensor graph,
frequency/voltage controls and profile info dialogs.

## Architecture, state and persistence

**SOURCE FACT.** CoreCtrl uses QML controls backed by C++ `UIComponents` such
as `PROFILE_MANAGER` and `GPU`; the inspected snippets do not show a separate
root daemon or IPC boundary.

**SOURCE FACT.** Profile UI distinguishes selected, active, manually toggled,
unsaved and unapplied settings (`Profiles.qml:27-33,148-179`).

**UNKNOWN.** Exact privilege model, daemon model, hardware capability display,
fan apply transaction and full config location were not established from the
limited cited files.

---

# Cross-application comparison

## Product / feature matrix

Legend: **Yes** = evidence found; **Partial** = feature exists but narrower or
conditional; **No** = inspected application explicitly lacks it; **UNKNOWN** =
not established.

| Application | Platform | Performance | Battery | Fans | GPU/MUX | Telemetry | Profiles | Automation | Hardware features |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| G-Helper | Windows ASUS | Yes | Yes | Yes | Yes / Yes | Yes | Yes | Yes | Yes, extensive ASUS |
| G-Linux | Linux ASUS | Yes | Yes | Yes | Yes / Yes | Yes | Yes | Yes | Yes, capability-gated ASUS |
| ASUS Control Center | Linux ASUS | Yes | Yes | Yes | Partial / optional | Partial | Yes | UNKNOWN | Keyboard/Aura; read-only firmware |
| Official ROG CC Slint | Linux ASUS | Partial | UNKNOWN | Partial | Partial | UNKNOWN | Partial | Yes, background flags | Keyboard/AniMe/system toggles |
| TUXEDO CC | Linux TUXEDO | Yes | Yes | Yes | Partial Prime/GPU | Yes | Yes | Partial | Display, webcam, keyboard, device tools |
| LACT | Linux multi-GPU | Partial | No laptop battery evidence | Yes | GPU, no MUX evidence | Yes, historical | Yes | Yes, process/GameMode | GPU-centric |
| Lenovo Legion Linux | Linux Lenovo | Yes | Yes | Yes | Partial Hybrid/GSync | Yes | Yes | Yes | LEDs, Fn, touchpad, camera, display |
| CoreCtrl | Linux AMD-focused | Yes | UNKNOWN | Yes GPU | GPU, no MUX evidence | Yes | Yes | Partial/UNKNOWN | GPU/CPU tuning |

Evidence: G-Helper README and cited feature modules; G-Linux capability enum
and code map; ASUS CC models/service; TCC profile/routing/daemon sources; LACT
README/schema/UI; Lenovo README/GUI; CoreCtrl QML.

## UI comparison matrix

| Application | First view | Navigation | Dashboard/cards | Fan editor | Telemetry charts/gauges | GPU selector | Profile selector | Conditional unsupported UX |
|---|---|---|---:|---:|---:|---:|---:|---|
| G-Helper | Compact main form/tray | Main form + separate forms | Compact panels | Chart form | Live labels/overlay; chart fan view | Direct buttons | Mode buttons | Hide controls / labels |
| G-Linux | Dashboard | Conditional pages/back toolbar | Sections/cards | Graph editor | Dashboard + diagnostics | Confirmation dialog | Dashboard buttons | Hide pages/sections + messages |
| ASUS CC | Overview | Sidebar + stacked pages | Device/service/capability cards | Custom widget | Status panels | Performance page/optional gfx | Overview/performance | Explicit placeholder + warnings |
| Official ROG CC | Home | Animated bottom icon bar | List/page shell | Profile/fan page | UNKNOWN | UNKNOWN | Profile/fan page | “Unsupported by hardware” text |
| TUXEDO CC | CPU dashboard | Routed Angular pages | Cards + gauges | Fan/custom chart | Animated gauges/charts | Prime conditional | Profile manager | Not-available gauges/tooltips |
| LACT | Main navigation split | Sidebar navigation | Sections | FanCurveFrame | Historical graph window | GPU selector | Popover/profile list | Sensitive/optional sections |
| Lenovo Legion Linux | Tabbed GUI | Tabs | Group boxes | Apply-to-HW chart | 10-second monitor/notifications | Combo/features | Preset dropdown | Errors/disabled controls |
| CoreCtrl | Profiles tab | Footer tabs | Form/graph panes | CurveControl | SensorGraph | GPU forms | Profile manager | UNKNOWN |

## Architecture matrix

| Application | UI | State/application layer | IPC/backend boundary | Background process | Configuration |
|---|---|---|---|---|---|
| G-Helper | C# WinForms | Feature controllers/global app state | Direct ACPI/HID/native/vendor APIs | Same process + tray | JSON, debounced atomic save, backup |
| G-Linux | QML/Kirigami | Core policies + application services + facade | Typed asusd D-Bus, sysfs snapshot, logind, KAuth | Tray/hotkeys; service details UNKNOWN | UNKNOWN in cited files |
| ASUS CC | PyQt6 | `SystemSnapshot` + `ControlService` | CLI subprocesses, optional service checks, read-only sysfs | External asusd/supergfxd | XDG JSON settings |
| Official ROG CC | Slint | Rust setup/signal handlers | `rog_dbus` proxies/signals | Same process; background flags | XDG JSON |
| TUXEDO CC | Angular/Electron | Electron APIs + common models + root workers | System D-Bus and worker APIs | Independent root `tccd.service` | Daemon ConfigHandler files |
| LACT | GTK4/Relm4 | `AppModel`, page controllers, client | JSON Unix socket; optional TCP | Independent `lactd` | `/etc/lact/config.yaml` and serialized profiles |
| Lenovo Legion Linux | PyQt6 | `LegionModelFacade`/feature wrappers | sysfs/debugfs/hwmon/kernel module | Optional `legiond` | YAML presets and daemon config |
| CoreCtrl | QML/C++ | C++ QML objects/profile manager | UNKNOWN; no IPC in cited files | Tray object; daemon UNKNOWN | UI settings/profile manager, exact path UNKNOWN |

## State and mutation matrix

| Application | Requested/draft | Current/effective | Applying/busy | Pending/restart | Failure/rollback |
|---|---:|---:|---:|---:|---:|
| G-Helper | Partial | Yes | Controller/task/locks | Yes for MUX/display | Logs/fallback, unified rollback UNKNOWN |
| G-Linux | Yes, fan/power drafts | Yes | Yes | Yes, `Queued` access | Explicit status and rollback |
| ASUS CC | Snapshot + ActionOutcome | Yes | Controller busy | Graphics pending fields | Partial result; rollback UNKNOWN |
| Official ROG CC | UNKNOWN | Reads current values | UNKNOWN | Background flags only | Mostly ignored callback errors |
| TUXEDO CC | Profile model | Active + previous profile | Worker intervals | UNKNOWN | Dialogs/errors exist; full semantics UNKNOWN |
| LACT | Profile/config model | Daemon responses | Reconnecting/UI sensitivity | UNKNOWN | Structured IPC errors; GPU recovery docs |
| Lenovo Legion Linux | Fan draft before Apply | Read from hardware | Thread worker | Hardware reset documented | Error marking; rollback UNKNOWN |
| CoreCtrl | `unappliedSettings` | Active profile | UNKNOWN | UNKNOWN | UNKNOWN |

## Background and lifecycle matrix

| Application | GUI required for active control | Tray/background | Startup/restore | Suspend/resume | Notifications/logging |
|---|---|---:|---:|---:|---|
| G-Helper | No for tray-owned app operation; same process | Yes | Startup option and event handlers | Subscribed | Toast/OSD/overlay/log |
| G-Linux | GUI plus external asusd; daemon details separate | Tray/hotkeys | UNKNOWN | UNKNOWN | Inline errors/diagnostics |
| ASUS CC | External asusd/supergfxd | UNKNOWN | External service state | UNKNOWN | Toast/status/warnings |
| Official ROG CC | Same process, background flags | Yes, if enabled | Explicit config flags | UNKNOWN | Signal watcher |
| TUXEDO CC | No; root `tccd` service | Service independent | systemd service setup | Workers/service hooks | Logs, dialogs, status |
| LACT | No; `lactd` service | Daemon independent | Service setup | README says reload on resume | Toast, logs, metrics exporter |
| Lenovo Legion Linux | No for `legiond` automation | Optional daemon | systemd/OpenRC | README documents resets | Notifications/logging |
| CoreCtrl | UNKNOWN | System tray object | UNKNOWN | UNKNOWN | UNKNOWN |

---

# Patterns recurring across applications

These are observations, not decisions for Orbis.

1. **A GUI and a long-lived control process are frequently separate.** Found in
   TUXEDO (`tccd.service` and Angular/Electron), LACT (`lactd` and GTK GUI),
   Lenovo Legion Linux (`legiond`), and G-Linux’s documented service/backend
   boundaries. G-Helper and the archived ROG CC instead show same-process
   desktop/background models in the inspected sources.
2. **Runtime capability discovery controls presentation.** Found explicitly in
   G-Linux’s `CapabilityRegistry`, ASUS CC’s snapshot/warnings, TCC compatibility
   service, official ROG CC supported-function data, G-Helper GPU visibility
   checks and Lenovo feature existence/model checks.
3. **Profiles aggregate multiple controls.** Found in G-Helper per-mode settings,
   G-Linux AC/battery profiles, TCC’s profile model, LACT per-GPU profiles,
   Lenovo fan presets/AC-battery profiles and CoreCtrl profile manager.
4. **Fan curves are treated as a specialized editor.** Found in G-Helper chart
   forms, G-Linux `FanCurveEditor`, TCC fan/custom chart components, LACT
   `FanCurveFrame` and Lenovo’s explicit Read/Apply GUI.
5. **GPU changes can be staged around restart.** Found especially in G-Linux
   queued capability and `GpuModeService`, G-Helper MUX confirmation/restart,
   and the official ASUS app’s D-Bus-driven GPU/GSYNC control surface.
6. **Telemetry is commonly colocated with the product’s first page.** Found in
   G-Linux Dashboard, TCC System monitor, LACT GPU stats/graphs and Lenovo’s
   monitor worker. G-Helper also exposes telemetry in the compact form and OSD.
7. **Errors are presented at several layers rather than one universal error
   type.** Examples include G-Linux inline messages/statuses, ASUS CC
   `ActionOutcome`, TCC compatibility tooltips/dialogs, LACT structured IPC
   responses and Lenovo red-marked widgets/logging.

# Interesting patterns unique to individual applications

- **G-Helper:** a single compact WinForms dashboard plus tray and in-game OSD;
  model-specific fan calibration and per-mode BIOS/ACPI customization.
- **G-Linux:** typed capability access has `ReadOnly` and `Queued` states with
  evidence quality; GPU service names rollback and queue-verification failures.
- **ASUS CC:** the Overview page shows service integration health alongside
  hardware capabilities; unavailable controls use an explanatory placeholder.
- **TUXEDO CC:** one profile includes display, CPU, webcam, fan, ODM and NVIDIA
  settings; daemon workers apply profile state at independent intervals.
- **LACT:** the GUI includes a service setup dialog and the daemon can run
  headless; the same product exposes GUI, CLI and JSON API-oriented boundaries.
- **Lenovo Legion Linux:** fan presets are explicitly read/display/apply, and
  separate AC/battery fan profiles are assigned by `legiond`.
- **CoreCtrl:** profile UI explicitly distinguishes unapplied from unsaved
  changes (`Profiles.qml:27-33,154-179`).
- **Official ROG CC:** a tiny FIFO/single-instance protocol keeps the GUI and
  background mode in one process, while D-Bus signal reception runs in a thread.

# Feature comparison: broader inventory

| Feature area | G-Helper | G-Linux | ASUS CC | TCC | LACT | Lenovo | CoreCtrl |
|---|---:|---:|---:|---:|---:|---:|---:|
| Performance modes/profiles | Yes | Yes | Yes | Yes | Partial | Yes | Yes |
| CPU power/limits/EPP | Yes | Yes | Partial | Yes | No laptop evidence | Yes | Yes |
| Battery charge limit | Yes | Yes | Yes | Yes | No laptop evidence | Yes | UNKNOWN |
| Fan RPM telemetry | Yes | Yes | Yes | Yes | Yes GPU | Yes | GPU-focused |
| Fan curve editor | Yes | Yes | Yes | Yes | Yes | Yes | Yes GPU |
| GPU power cap/clocks | Yes | Yes | UNKNOWN | Partial | Yes | Yes | Yes |
| GPU mode switching | Yes | Yes | Optional | Prime/partial | No MUX evidence | Hybrid/partial | No MUX evidence |
| MUX / restart UX | Yes | Yes | Pending fields | UNKNOWN | UNKNOWN | UNKNOWN | UNKNOWN |
| Historical charts | UNKNOWN | UNKNOWN | UNKNOWN | Charts | Yes | No evidence | Sensor graph |
| Dashboard telemetry | Yes | Yes | Yes | Yes | Yes | Monitor UI | System/GPU |
| AC/battery automation | Yes | Yes | UNKNOWN | Profile/state workers | Process/GameMode | Yes | UNKNOWN |
| Per-process/game profiles | UNKNOWN | UNKNOWN | UNKNOWN | UNKNOWN | Yes | UNKNOWN | Yes |
| Keyboard/RGB/Aura | Yes | Yes | Yes | Keyboard | No | LEDs | No evidence |
| Display controls | Yes | Yes | Partial | Yes | Displays page | Overdrive | No evidence |
| Webcam/control devices | Yes, ASUS peripherals | ASUS peripherals | Keyboard/Aura | Webcam/Aquaris/Tomte | No evidence | Camera/touchpad | No evidence |
| Diagnostics/reporting | Logs/updates | Diagnostics | Diagnostics | Support/info | Debug snapshot/export | Logging | UNKNOWN |
| External GPU/eGPU | XG Mobile | eGPU | UNKNOWN | Prime/GPU | GPU selection | Hybrid/GSync | UNKNOWN |

---

# Things Orbis reviewers may want to inspect

This list identifies source areas for later architectural review; it does not
state that any item must be copied or excluded.

- G-Helper’s compact primary dashboard, tray/background behavior and profile
  composition: `app/Settings.cs`, `app/Program.cs`, `app/Mode/ModeControl.cs`.
- G-Helper’s GPU confirmation/restart and temporary lock flow:
  `app/Gpu/GPUModeControl.cs`.
- G-Helper’s atomic config recovery and backup behavior: `app/AppConfig.cs`.
- G-Linux’s `CapabilityRegistry` access/evidence model and `DeviceSpecification`:
  `src/core/device/capability.hpp`, `devicespecification.hpp`.
- G-Linux’s explicit GPU queue, reboot and rollback statuses:
  `src/application/gpu/gpumodeservice.hpp`, `docs/code-map.md`.
- G-Linux’s draft/Apply behavior for fan and power controls:
  `qml/pages/FansPage.qml`.
- ASUS CC’s service-health dashboard and unavailable placeholder:
  `ui/pages/overview.py`, `ui/widgets/unavailable_notice.py`.
- TUXEDO’s profile model, system-monitor dashboard, compatibility service and
  independent worker model: `TccProfile.ts`, `dashboard.component.html`,
  `compatibility.service.ts`, `TuxedoControlCenterDaemon.ts`.
- LACT’s profile rules, historical telemetry UI, service setup and reconnecting
  client: `lact-gui/src/app/profiles.rs`, `lact-gui/src/app/graphs_window`,
  `lact-gui/src/service_setup.rs`, `lact-client/src/lib.rs`.
- Lenovo’s explicit read/edit/apply fan workflow and AC/battery `legiond`
  profiles: repository README and `python/.../legion_gui.py`.
- CoreCtrl’s distinction between active, unapplied and unsaved profile state:
  `src/qml/Profiles.qml`.
- Official ROG Control Center’s supported-function response, signal thread and
  background FIFO: `src/main.rs`, `ui/main.slint`.

## Limits of this dossier

The reports use pinned source snapshots and do not claim live behavior, distro
policy behavior, firmware compatibility, complete repository coverage or
complete feature parity. Backend references are included only where needed to
explain application boundaries, UI state, capability handling or lifecycle.
No Orbis code, Orbis documentation or existing research file was changed.
