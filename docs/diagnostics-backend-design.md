# Orbis Production Diagnostics Backend Design

**Research date:** 2026-08-18  
**Research type:** source audit and architecture design only  
**Production backend source:** `chatgpt/production-hardening-20260818`  
**Audited hardening SHA:** `a745f559e1dc3aeefc665b31f740242bc4cb7348`  
**Diagnostics UI source:** `agent/light-theme-toggle`  
**Audited UI SHA:** `4a63c6b8f36688a8b81fc2f87a94eb2216d8ee56`  
**Base `main` SHA:** `b9bfdf5bb08fecea58589bcd204215b49384d9ef`  
**Privileged writes performed:** none

## 1. Executive summary

The current `DiagnosticsWindow` is a visual preview, not a production diagnostics client. Its Kernel and Platform values are literals, all capability/provider badges say `Preview`, the log area contains static preview strings, and Copy/Export only update a local status label. The only value passed into the window is `version`, and that value is currently initialized to the literal `0.1.0` in the UI state rather than read from Cargo package metadata.

The production backend is much closer to supporting a real Diagnostics window than the UI implies. Orbis already has:

- a canonical typed capability model in `orbis-core`, including per-operation read/write status;
- an immutable `CapabilityRegistrySnapshot` wired into the production application runtime;
- six production capability entries already aggregated: Performance, ChargeLimit, GpuPower, GpuMux, GpuAccess, and FanCurves;
- authoritative read-only GPU primitives through Session1;
- direct in-process read-only telemetry through `SysfsTelemetryProvider`;
- read-only ASUS Armoury providers and probes for Panel Overdrive, MiniLED, and Screen Auto Brightness;
- a read-only Wayland output provider with exact millihertz mode data;
- read-only keyboard-backlight and Aura providers;
- read-only Hardware1 mutation-status properties that do not require polkit authorization and do not perform mutations;
- exact backend-specific supergfxd staged-state types distinct from product `GpuMode`.

The recommended architecture is **application/runtime aggregation**: the GUI process should build one immutable typed diagnostics snapshot from sources it already owns or may safely query read-only, then pass a presentation DTO to Slint. `DiagnosticsWindow` should not independently poll providers, and Session1 should not become a generic diagnostics aggregation service.

This preserves the existing Orbis boundaries:

- Session1 remains the read-only service for the capabilities it already owns;
- Hardware1 remains the privileged mutation boundary, while Diagnostics may read only its existing status metadata;
- local sysfs telemetry and Wayland observation remain in the unprivileged application process;
- optional daemons remain capability-local rather than becoming global Orbis dependencies.

The proposed contract deliberately keeps **service health, capability support, observed values, and mutation availability as different facts**. A running service does not prove a capability; a supported read does not prove a write; and a missing optional provider must not turn into a single global red/green “Backend OK” state.

### Source-readiness count

For the source-mapping inventory in section 4, this document counts **47 diagnostic items**. The count is by semantic item, not by visual text label or by every scalar inside a compound domain object.

| Readiness class | Count | Meaning |
|---|---:|---|
| Direct existing production source | **16** | Existing production runtime/domain data can be wired without implementing a new backend/provider |
| Aggregation / small backend wiring required | **26** | A safe source or provider already exists, or the information is available from a narrow OS/D-Bus query, but a typed aggregation path is not wired yet |
| No honest production source today | **5** | The audited source cannot currently provide the fact without adding a new evidence source or product-state authority |
| **Total** | **47** | Contract inventory used for the final report |

The five items with no honest production source today are: embedded build revision, generic compositor identity, authoritative requested product GPU mode, CPU package power draw, and the current preview “Power limits” capability row.

---

## 2. Current Diagnostics UI audit

### 2.1 Window lifecycle

UI source: `ui/audited/diagnostics-window.slint` on `agent/light-theme-toggle`.

The production UI entry point in `crates/orbis-ui/src/main.rs` creates a new `DiagnosticsWindow`, sets only its `version` property from `UiState`, applies the current theme, and shows the window. There is no diagnostics worker request, provider snapshot, capability model, service-health model, timer, or refresh callback connected to the window.

This means the current refresh lifecycle is effectively **none**: values are fixed at window creation except for local UI-only status strings.

### 2.2 Displayed UI audit

| Current UI field / row / action | Current state | Desired production semantic | Existing source | Proposed contract mapping | Refresh policy | Failure representation |
|---|---|---|---|---|---|---|
| `Kernel: Linux` | Static preview literal | Exact kernel release, not generic OS family | No Orbis field today; safe local OS query is available | `system.kernel_release` | On window open; manual refresh sufficient | `Unknown`, never literal `Linux` as a substitute |
| `Platform: ASUS` | Static preview literal | Device vendor/product/board, with privacy-safe identity | `DeviceIdentity` domain type exists, but no production reader is wired | `hardware.identity` | On open; effectively static for process lifetime | `Unknown` / absent identity |
| `Orbis: v<version>` | Window input, but source `UiState.version` is hardcoded `0.1.0` | Running application package version | Cargo workspace/package metadata exists | `application.version` | Build-time constant | `Unknown` only if build pipeline fails to expose it |
| Orbis detail `dev` | Static literal | Build channel/revision only if actually embedded | No embedded revision found | `application.build` optional | Build-time constant | Omit field when unavailable |
| `Performance` capability | `Preview` | Canonical Performance capability, including read/write operation states | Existing production registry | `capabilities[Performance]` | Registry generation changes; on open/manual refresh | Preserve canonical `CapabilityStatus` |
| `Battery threshold` capability | `Preview` | ChargeLimit capability, not generic battery health | Existing production registry | `capabilities[ChargeLimit]` | Registry generation changes | Preserve canonical status/reason |
| `GPU primitives` capability | One coarse preview row | Three independent capabilities: MUX, access, runtime power | Existing production registry and Session1 providers | Separate `GpuMux`, `GpuAccess`, `GpuPower` rows | Registry + independent value refresh | Per-primitive failure; never collapse to one GPU mode |
| `Fan curves` capability | `Preview` | FanCurves read/write capability | Existing production registry | `capabilities[FanCurves]` | Registry generation changes | Canonical status; write status separate |
| `Power limits` capability | `Preview` | Power-limit support only if a production source proves it | Domain trait/types exist, but no production runtime/probe source found in audited composition | `capabilities.power_limits` only after a real source exists | Not applicable today | `Unknown`/Not connected; do not claim Supported |
| `Display / Lighting` capability | One coarse preview row | Separate panel/display/lighting capabilities | Multiple typed providers/probes exist, but are not one capability | Separate PanelOverdrive, MiniLed, ScreenAutoBrightness, KeyboardBacklight, Aura, DisplayOutput | Per provider/registry refresh | Per-capability status |
| `[preview] Session1 status not queried` | Static preview log string | Session service presence/health | Session bus name is known; app already has session-bus connection | `services.sessiond` | On open + manual refresh; optionally slow periodic refresh | `Running`, `Activatable`, `Unavailable`, `Unknown` as applicable |
| `[preview] Hardware1 status not queried` | Static preview log string | Hardware service presence/health | System bus name is known; app already has system-bus connection | `services.hardwared` | On open + manual refresh | Separate service state from mutation statuses |
| `[preview] No diagnostics log transport...` | Static preview string | No generic log dump should be introduced | No safe log transport exists | Replace with structured snapshot metadata/errors, not journal text | Snapshot-driven | Sanitized error class + short message only |
| `asusd` provider row | `Preview` | Whether the asusd D-Bus service is currently owned/activatable, plus capability-local effects | Known service `xyz.ljones.Asusd`; several providers already use it | `services.asusd` | On open/manual/slow refresh | Service unavailable; affected capabilities fail separately |
| `supergfxd` provider row | `Preview` | Exact supergfxd service presence and optional staged backend state | `org.supergfxctl.Daemon` typed read source already exists | `services.supergfxd`, optional `gpu.supergfxd` | On open + slow refresh; staged state on explicit refresh | Unavailable without poisoning MUX/access |
| `Hardware1` provider row | `Preview` | Same underlying hardwared service, not a second backend identity | `io.github.orbiscontrol.Hardware` | Reuse `services.hardwared`; UI may label protocol `Hardware1` | On open/manual | No duplicate health truth |
| `Copy Summary` | Preview-only status change | Copy a sanitized, generated summary from the typed snapshot | No implementation today | Derived from safe export view | Explicit user action | Report generation error, no hidden fallback |
| `Open Logs` | Toggles static preview text | Should not become a generic journal dump | No safe generic log source | Prefer “Details” over structured diagnostic errors/events; otherwise leave unimplemented | User action | No shell/journal command fallback |
| `Export Report` | Preview-only status change | Explicit safe text/JSON export | No implementation today | `DiagnosticExportReport` allowlist | Explicit user action; fresh snapshot first | Export failure only; no partial secret-bearing dump |
| Local status footer | UI-local preview text | Snapshot timestamp / refresh state / export status | Future collector state | Presentation-only | On refresh/action | `Refreshing`, last success time, sanitized error |

### 2.3 Existing UI state that Diagnostics should reuse semantically, not structurally

`UiState` already receives production telemetry and the six production registry capabilities. It also tracks the three independent GPU primitive values. Diagnostics should **not** copy those formatted strings as its backend contract. The correct source remains the typed runtime data:

- `Telemetry` rather than `UiState.cpu_temp`, `gpu_temp`, etc.;
- `CapabilityRegistrySnapshot` rather than `CapabilityAvailability` presentation enums;
- `GpuMuxState`, `GpuAccessPolicy`, and `GpuPowerState` rather than integer display values.

The Diagnostics presentation layer may format those typed fields, but the aggregate snapshot must not be a second string cache.

---

## 3. Proposed typed data model

### 3.1 Naming and placement

`DiagnosticsSnapshot` is an acceptable name and matches the semantics: an immutable, point-in-time aggregate assembled from independently sampled sources.

The domain type should live in or near `orbis-core::diagnostics`, but it must **not** directly depend on `orbis-capabilities::CapabilityRegistrySnapshot`, because `orbis-capabilities` already depends on `orbis-core`. To avoid a dependency cycle, the domain aggregate should carry the core-owned capability data plus registry metadata:

- capability generation;
- capability checked timestamp;
- `DeviceCapabilities` from `orbis-core`.

The existing generic `DiagnosticEntry` and `DiagnosticReport` are useful as supplementary key/value diagnostics but are not sufficiently typed for the production UI contract. They should not become the primary window model.

### 3.2 Conceptual aggregate

The following is a schema description, not implementation code:

- `DiagnosticsSnapshot`
  - `schema_version`
  - `generated_at`
  - `application`
  - `system`
  - `hardware`
  - `services`
  - `capabilities`
  - `gpu`
  - `telemetry`
  - `display`

#### Application diagnostics

- `version`: exact package version of the running `orbis-control` binary.
- `build_revision`: optional; absent until a revision is explicitly embedded by the build.
- `build_channel` or profile: optional; include only if a deterministic build-time source is deliberately added.

Do not infer a Git SHA from the working tree at runtime and do not run `git` from Diagnostics.

#### System diagnostics

- `kernel_release`: exact kernel release from a narrow local OS API.
- `architecture`: normalized architecture of the running build/system.
- `session_type`: normalized session type, e.g. Wayland/X11/TTY/Unknown.
- `display_protocol`: Wayland/X11/Unknown, kept separate if needed from login-session classification.
- `compositor`: optional/unknown unless a reliable compositor-specific source is added.

`XDG_CURRENT_DESKTOP` is not a compositor identity and must not be relabeled as one.

Only a small allowlist of environment variables may be inspected for session classification. The snapshot and export must contain normalized values, never the full environment or raw environment dump.

#### Hardware diagnostics

Use the existing privacy-aware `DeviceIdentity` shape when a production DMI reader is added/wired:

- vendor;
- product;
- board;
- BIOS version;
- BIOS date.

The existing domain type intentionally excludes serial number. Preserve that property.

#### Service diagnostics

Service state must use a dedicated service-presence model rather than `CapabilityStatus`.

Recommended states:

- `Running` — the well-known D-Bus name currently has an owner;
- `Activatable` — no current owner, but D-Bus reports the name as activatable;
- `Unavailable` — neither owned nor known as activatable where activation applies;
- `PermissionDenied` — service-presence check itself is denied;
- `Unknown` — transport/query failure prevents a reliable classification.

Recommended service records:

- stable service ID;
- bus scope (`system` or `session`);
- availability state;
- criticality;
- last checked timestamp;
- optional sanitized failure class/message.

A non-activating D-Bus presence check should prefer `org.freedesktop.DBus` ownership/activatable-name metadata. It should not start daemons merely to make a green badge.

#### Capability diagnostics

Do **not** create a new diagnostics capability enum. Reuse the canonical Orbis model:

- `CapabilityStatus::Supported`;
- `SupportedWithRequirement`;
- `ReadOnly`;
- `TemporarilyUnavailable`;
- `Unsupported`;
- `BackendMissing`;
- `PermissionDenied`;
- `Experimental`;
- `Conflicted`;
- `Unknown`.

Preserve `CapabilityOperations.read` and `.write` separately. The Diagnostics UI may display “Writable” only as a derived presentation fact when the write operation status is `Supported` or `SupportedWithRequirement`.

A feature-level `Supported` must not be treated as proof that every operation is writable.

For safe UI/export, capability reason data should be normalized. Backend identity and requirement/risk are useful; arbitrary `endpoint` strings should not be exported by default because they may reveal filesystem or transport paths without user value.

#### GPU diagnostics

Keep five concepts separate:

1. authoritative physical MUX state;
2. authoritative dGPU access policy;
3. authoritative runtime dGPU power state;
4. authoritative requested **product** `GpuMode`, if one exists;
5. backend-specific staged/pending state and required user action.

The first three already exist in production. The fourth does not have an authoritative production source in the audited runtime. The fifth can be represented using exact `SupergfxdSnapshot` semantics, but must be labeled as **supergfxd backend state**, not product `GpuMode`.

#### Telemetry diagnostics

Recommended shape:

- provider availability / last collection result;
- latest `Telemetry` snapshot, if available;
- `last_attempt_at`;
- `last_success_at`;
- computed age from `Telemetry.ts`;
- freshness classification derived from age relative to the configured polling interval;
- optional sanitized last error.

Do not create another copy of CPU/GPU temperature, fans, battery, and power fields inside the diagnostics domain. The existing typed `Telemetry` payload already contains those values.

A reasonable presentation freshness policy is:

- `Fresh`: age is within an explicit multiple of the runtime polling interval;
- `Stale`: a last successful snapshot exists but exceeds that threshold;
- `Unknown`: no successful sample or time comparison is possible.

The exact multiplier should be one documented policy constant rather than scattered UI logic.

#### Display diagnostics

Use `DisplayOutputSnapshot` as the observed display payload:

- output runtime ID/name;
- current mode;
- exact width/height;
- exact refresh in millihertz;
- presentation-formatted Hz derived from millihertz;
- observed available modes;
- optional preferred mode only when the compositor published the corresponding evidence.

No `can_modeset`, `can_change_refresh`, or equivalent writable flag may be inferred from observation. Wayland output observation remains read-only.

Panel Overdrive is a separate ASUS panel capability and must not be folded into display refresh diagnostics.

---

## 4. Source mapping

### 4.1 Readiness classes

- **DIRECT** — the production runtime/domain already has the needed fact or payload. Future UI work is wiring/presentation, not a new backend.
- **AGGREGATE** — a safe underlying source/provider exists, or a narrow standard OS/D-Bus query is sufficient, but the diagnostics aggregate does not currently collect it.
- **NO SOURCE** — no authoritative production source exists in the audited architecture for the semantic being requested.

### 4.2 Complete diagnostic item inventory

| # | Diagnostic item | Source / exact location | Readiness | Proposed field | Refresh | Failure representation |
|---:|---|---|---|---|---|---|
| 1 | Orbis version | Cargo workspace version; `orbis-ui` uses `version.workspace = true` | **DIRECT** | `application.version` | Build constant | Unknown/absent, never a hardcoded UI substitute |
| 2 | Build revision / Git SHA | No embedded revision found | **NO SOURCE** | `application.build_revision` optional | Build constant if later added | Omit/Unknown |
| 3 | Kernel release | Narrow local OS query; no Orbis domain source today | **AGGREGATE** | `system.kernel_release` | On open | Unknown |
| 4 | Architecture | Narrow local process/OS metadata | **AGGREGATE** | `system.architecture` | On open | Unknown |
| 5 | Session type | Whitelisted normalized session metadata | **AGGREGATE** | `system.session_type` | On open + session reopen | Unknown |
| 6 | Wayland/X11 protocol | Whitelisted display/session metadata plus Wayland source evidence | **AGGREGATE** | `system.display_protocol` | On open | Unknown |
| 7 | Generic compositor identity | No reliable generic source in audited Orbis; desktop name is not compositor | **NO SOURCE** | `system.compositor` optional | N/A | Unknown/omit |
| 8 | Privacy-safe hardware identity | `DeviceIdentity` exists in `orbis-core/src/identity.rs`; production reader not wired | **AGGREGATE** | `hardware.identity` | On open | Unknown/absent |
| 9 | sessiond availability | Session bus `io.github.orbiscontrol.Session` | **AGGREGATE** | `services.sessiond` | On open + manual/slow refresh | Service availability state |
| 10 | hardwared availability | System bus `io.github.orbiscontrol.Hardware` | **AGGREGATE** | `services.hardwared` | On open + manual/slow refresh | Service availability state |
| 11 | asusd availability | System bus `xyz.ljones.Asusd` used by existing providers | **AGGREGATE** | `services.asusd` | On open + manual/slow refresh | Service availability state |
| 12 | ASUS Armoury availability | `/sys/class/firmware-attributes/asus-armoury` typed providers/probes | **AGGREGATE** | `services.armoury` or backend record | On open + manual refresh | Available/Unavailable/PermissionDenied/Unknown |
| 13 | supergfxd availability | System bus `org.supergfxctl.Daemon` | **AGGREGATE** | `services.supergfxd` | On open + manual/slow refresh | Service availability state |
| 14 | UPower availability | System bus `org.freedesktop.UPower` | **AGGREGATE** | `services.upower` | On open + manual/slow refresh | Service availability state |
| 15 | Performance capability | Existing production `CapabilityRegistrySnapshot` | **DIRECT** | `capabilities[Performance]` | Registry generation | Canonical Capability |
| 16 | ChargeLimit capability | Existing production registry | **DIRECT** | `capabilities[ChargeLimit]` | Registry generation | Canonical Capability |
| 17 | GPU MUX capability | Existing production registry | **DIRECT** | `capabilities[GpuMux]` | Registry generation | Canonical Capability |
| 18 | dGPU access capability | Existing production registry | **DIRECT** | `capabilities[GpuAccess]` | Registry generation | Canonical Capability |
| 19 | runtime dGPU power capability | Existing production registry | **DIRECT** | `capabilities[GpuPower]` | Registry generation | Canonical Capability |
| 20 | Fan curves capability | Existing production registry | **DIRECT** | `capabilities[FanCurves]` | Registry generation | Canonical Capability |
| 21 | Panel Overdrive capability | `AsusArmouryPanelOverdriveProvider` + `probe_panel_overdrive`; Hardware1 read-only panel mutation status exists | **AGGREGATE** | `capabilities[PanelOverdrive]` | Registry refresh | Canonical Capability |
| 22 | MiniLED capability | Existing ASUS Armoury provider + `probe_mini_led_mode` | **AGGREGATE** | `capabilities[MiniLed]` | Registry refresh | Canonical Capability; write remains ReadOnly absent a writer |
| 23 | Screen Auto Brightness | Existing ASUS Armoury provider + `probe_screen_auto_brightness` | **AGGREGATE** | `capabilities[ScreenAutoBrightness]` | Registry refresh | Canonical Capability |
| 24 | Keyboard backlight capability | `AsusKeyboardBacklightProvider`; Hardware1 read-only mutation-status property exists; no current registry probe | **AGGREGATE** | `capabilities[KeyboardBacklight]` | Registry refresh | Canonical Capability; no test mutation |
| 25 | Aura capability | `AsusAuraProvider`; Hardware1 read-only Aura mutation-status property exists; no current registry probe | **AGGREGATE** | `capabilities[Aura]` | Registry refresh | Canonical Capability |
| 26 | Wayland output observation capability | `WaylandDisplayOutputProvider` + `probe_display_output`; not in current runtime registry | **AGGREGATE** | `capabilities[DisplayOutput]` | Registry/display refresh | Canonical Capability, write ReadOnly |
| 27 | Physical MUX observed value | Production `GpuPrimitiveServices` / Session1 `GpuMuxState` | **DIRECT** | `gpu.mux` | Existing GPU refresh | Provider error distinct from domain Unknown |
| 28 | dGPU access observed value | Production Session1 `GpuAccessPolicy` | **DIRECT** | `gpu.access_policy` | Existing GPU refresh | Provider error distinct from domain Unknown |
| 29 | Runtime dGPU power observed value | Production Session1/supergfxd `GpuPowerState` | **DIRECT** | `gpu.power_state` | Existing GPU refresh | Provider error distinct from domain Unknown/Stale |
| 30 | Requested product `GpuMode` | Production primitive runtime intentionally does not provide product mode | **NO SOURCE** | `gpu.requested_product_mode` optional | N/A | Unavailable/absent; never synthesize |
| 31 | supergfxd current backend mode | Existing `SupergfxdSnapshot.current_mode` source | **AGGREGATE** | `gpu.supergfxd.current_mode` | On diagnostics refresh | Exact backend enum/Unknown(raw) |
| 32 | supergfxd pending backend mode | Existing `SupergfxdSnapshot.pending_mode` | **AGGREGATE** | `gpu.supergfxd.pending_mode` | On diagnostics refresh | Exact backend enum/Unknown(raw) |
| 33 | supergfxd pending action / reboot | Existing `SupergfxdSnapshot.pending_user_action` | **AGGREGATE** | `gpu.supergfxd.pending_user_action` | On diagnostics refresh | Exact backend enum; label as backend state |
| 34 | CPU temperature | Existing production `Telemetry.cpu_temp` | **DIRECT** | `telemetry.latest.cpu_temp` | Existing telemetry poll | `None` + provider metadata |
| 35 | GPU temperature | Existing `Telemetry.gpu_temp` | **DIRECT** | `telemetry.latest.gpu_temp` | Existing telemetry poll | `None` + provider metadata |
| 36 | Fan RPM | Existing `Telemetry.fans` | **DIRECT** | `telemetry.latest.fans` | Existing telemetry poll | Missing fan entry, not zero |
| 37 | Battery telemetry | Existing `Telemetry.battery` and `ac_online` | **DIRECT** | `telemetry.latest.battery` | Existing telemetry poll | `None`/partial fields |
| 38 | GPU power draw | Existing `Telemetry.power.gpu` | **DIRECT** | `telemetry.latest.power.gpu` | Existing telemetry poll | `None`, not 0 W |
| 39 | CPU package power draw | No CPU power field/source in production Telemetry | **NO SOURCE** | `telemetry.cpu_power` only after a real provider exists | N/A | Omit/Unknown |
| 40 | Telemetry freshness/age | Existing `Telemetry.ts` + runtime poll interval | **DIRECT** | `telemetry.age`, `freshness` | Every UI render/refresh | Unknown if no successful sample |
| 41 | Telemetry provider availability | Snapshot success/failure exists but no typed diagnostics record | **AGGREGATE** | `telemetry.provider` | Every telemetry attempt | Available/Degraded/Unavailable + sanitized error |
| 42 | Wayland output id/name | Existing `DisplayOutputSnapshot` source | **AGGREGATE** | `display.outputs[].id` | On open + display refresh | No output / provider error |
| 43 | Current resolution/mode | Existing `DisplayMode` current observation | **AGGREGATE** | `display.outputs[].current_mode` | Display refresh | Missing current mode if compositor gave none |
| 44 | Exact refresh mHz/Hz | Existing `RefreshMilliHz` | **AGGREGATE** | current mode refresh | Display refresh | Preserve 0/unknown semantics; no invented Hz |
| 45 | Observed available modes | Existing Wayland provider observation | **AGGREGATE** | `display.outputs[].available_modes` | Display refresh | Empty means unobserved/none published, not proof only one mode exists |
| 46 | Preferred/non-current mode evidence | Existing provider preserves optional preferred + observed mode list | **AGGREGATE** | `preferred`, observed modes | Display refresh | `None` if compositor did not publish evidence |
| 47 | Preview `Power limits` capability | `PowerLimitProvider` trait/domain types exist but no production runtime/probe source found in audited composition | **NO SOURCE** | Do not publish as confirmed capability yet | N/A | Unknown/Not connected |

### 4.3 Directly connectable production data

The 16 DIRECT items require no new hardware backend:

- application version from build/package metadata;
- six existing registry capability entries;
- three GPU primitive values;
- CPU temperature;
- GPU temperature;
- fan RPM telemetry;
- battery telemetry;
- GPU power telemetry;
- telemetry timestamp/freshness.

They still require Diagnostics-specific UI/worker wiring, but not a new provider implementation or privileged API.

### 4.4 Small aggregation work, not new hardware support

Most of the 26 AGGREGATE items already have a typed provider or can be obtained from a deliberately narrow read-only query. The work is primarily:

- add existing probes/providers to the registry/runtime;
- add read-only Hardware1 status decoding for the capability families not yet represented in `orbis-session-client` helpers;
- collect D-Bus service ownership/activation metadata;
- add a tiny system metadata source;
- place the existing Wayland provider in the application runtime;
- retain telemetry collection status alongside the last typed `Telemetry` value.

This should not be described as “implementing new hardware support.”

---

## 5. GPU primitive model

### 5.1 Production facts that exist now

The production runtime uses `GpuPrimitiveServices`, not a full product-mode `GpuProvider`. Therefore the following three values are authoritative and independently readable:

| Fact | Domain type | Production source | Meaning |
|---|---|---|---|
| Physical MUX | `GpuMuxState` | Session1 ASUS Armoury/kernel path | Which GPU path the physical MUX currently represents |
| dGPU access | `GpuAccessPolicy` | Session1 ASUS Armoury/kernel path | Whether applications may access dGPU; not physical routing |
| Runtime dGPU power | `GpuPowerState` | Session1 supergfxd read path | Active/Suspended/Off/Stale/Unknown runtime state |

A domain `Unknown` value is a successful observation with unknown semantic state. A provider error is different and must be represented as unavailable/error evidence rather than converting it to domain `Unknown`.

### 5.2 Product `GpuMode` must remain absent

`GpuMode::{Eco, Standard, Ultimate, Optimized}` exists in the domain, but the production primitive runtime intentionally does not claim an authoritative selected/requested product mode and intentionally does not support product-mode mutation.

Diagnostics must therefore **not** infer:

- Eco from `access=Blocked`;
- Ultimate from `mux=Discrete`;
- Standard from `mux=Integrated + access=Unblocked`;
- Optimized from any combination of primitives.

Those combinations are not equivalent to an authoritative product policy state.

`requested_product_mode` should be absent/Unavailable until a real product-policy authority is implemented.

### 5.3 supergfxd staged state

Orbis already has exact backend-specific types:

- `SupergfxdMode`;
- `SupergfxdUserAction`;
- `SupergfxdSnapshot`;
- `SupergfxdStagedState`.

These preserve the actual backend concepts, including `Reboot`, `Logout`, and unknown future wire values.

Diagnostics may show these in an explicitly labeled subsection such as **supergfxd backend state**:

- current backend mode;
- pending backend mode;
- pending user action;
- supported backend modes;
- runtime power state.

This must not be renamed “Current Orbis GPU mode.”

`classify_supergfxd_state` requires a requested backend mode. Diagnostics should not invoke it with a synthesized product request. If no authoritative requested backend mode exists, show the raw staged facts instead.

---

## 6. Telemetry and display model

### 6.1 Telemetry: values plus metadata

Diagnostics should show **both** useful current values and collection metadata.

Only showing provider health would make a support report less useful. Only duplicating raw values would hide whether those values are stale or why fields are missing.

Recommended presentation:

- CPU temperature: current value if present;
- GPU temperature: current value if present;
- CPU/GPU/other fan RPM from `Telemetry.fans`;
- battery percentage/capacity/cycles/state when present;
- AC online state when present;
- GPU power draw when present;
- AC/battery/total power only under their actual names if shown;
- sample timestamp;
- age/freshness;
- provider collection status.

### 6.2 What must not be relabeled

`PowerTelemetry` contains `ac`, `battery`, `total`, and `gpu`. There is no CPU package power field.

Therefore:

- `power.gpu` may be shown as GPU power draw;
- `power.ac` may be shown as AC-side power only if its source semantics are understood/preserved;
- `power.total` is not CPU power;
- no existing field may be renamed “CPU power.”

CPU power remains unavailable until a real source is added.

### 6.3 Partial telemetry is valid

The sysfs provider discovers sources independently. Diagnostics must preserve per-field absence:

- missing GPU temperature does not make CPU temperature invalid;
- missing battery capacity does not make battery percentage invalid;
- missing GPU fan does not imply 0 RPM;
- missing power source does not imply 0 W.

The UI should use an em dash/“Unavailable” presentation for absent values but retain `None` in the typed snapshot.

### 6.4 Display-output observation

The Wayland provider already uses compositor-published `wl_output` state and is explicitly read-only.

For each output, Diagnostics may show:

- `DisplayOutputId` / compositor-provided output name;
- current width and height;
- exact current refresh in mHz;
- formatted Hz derived from the exact mHz value;
- observed available modes;
- preferred mode only when the compositor supplied preferred evidence.

Important limitations:

- `DisplayOutputId` is runtime/session-local, not a permanent hardware ID;
- the observed mode list may be incomplete on modern compositors/protocol behavior;
- absence of non-current modes does not prove no other modes exist;
- output observation does not imply a modesetting/write capability;
- Diagnostics must not add refresh-rate controls or mutation flags from this data.

### 6.5 Panel Overdrive is independent

Panel Overdrive comes from ASUS Armoury capability/backend evidence. It must be a separate capability row from:

- current refresh rate;
- available display modes;
- Wayland output provider health.

A 144 Hz/165 Hz current mode neither proves nor disproves Panel Overdrive support.

---

## 7. Backend and service health semantics

### 7.1 No global “Backend OK” boolean

A single green/red backend summary is misleading for Orbis because different features have independent authorities.

Examples:

- supergfxd may be absent while Performance, ChargeLimit, MUX and telemetry remain usable;
- asusd may be absent while kernel platform profile and local telemetry still work;
- ASUS Armoury attributes may be absent on a non-ASUS or different ASUS model without implying an Orbis process failure;
- hardwared may be unavailable while Diagnostics itself and all unprivileged read-only sources still work.

The UI should show individual service/backend rows and capability-local consequences.

### 7.2 Recommended service criticality

| Backend/service | Audited role | Diagnostics criticality | Failure scope |
|---|---|---|---|
| `orbis-sessiond` | Session1 authoritative reads for Battery/Performance/GPU primitives | **Core read-path dependency in current production architecture** | Session1-backed capabilities unavailable; local telemetry/display diagnostics should still be renderable by the app collector |
| `orbis-hardwared` | Hardware1 privileged mutation boundary + read-only mutation status metadata | **Capability-local for Diagnostics** | Mutation/write evidence becomes Unknown/Unavailable; Diagnostics must not fail globally |
| `xyz.ljones.Asusd` | Battery configured state, fan/Aura and some mutation backends | **Optional / capability-local** | Only dependent capabilities degrade |
| ASUS Armoury kernel ABI | MUX/access/panel/MiniLED/auto-brightness attributes | **Optional / capability-local** | Unsupported/BackendMissing per feature/device |
| `org.supergfxctl.Daemon` | dGPU runtime power and staged backend diagnostics | **Optional / capability-local** | GpuPower/supergfxd backend fields unavailable; do not poison MUX/access |
| `org.freedesktop.UPower` | Current sessiond Battery discovery/read path | **Effectively sessiond-startup-critical at audited SHA; architecturally Battery-local after planned remediation** | Current hardening can lose sessiond startup; target architecture should degrade Battery only |

The UPower row must reflect the audited reality while also documenting the intended post-remediation architecture. Diagnostics design must not encode UPower as a permanent global dependency.

### 7.3 Service presence is not capability support

Service health and feature status are two layers:

- `services.asusd = Running` says the D-Bus name is present;
- `capabilities[Aura] = Unsupported` may still be correct if the required Aura object/interface is absent;
- `services.supergfxd = Running` does not guarantee a meaningful dGPU power value;
- `services.hardwared = Running` does not prove a particular mutation backend is supported.

The capability probes remain authoritative for feature support.

### 7.4 ProviderHealth limitations

The generic `Provider` trait exposes `ProviderHealth::{Healthy, Degraded, Unavailable}`, but several concrete production providers return `Healthy` structurally and surface actual backend failure only when a read is attempted.

Therefore Diagnostics must not use `provider.health() == Healthy` as the sole live-service health signal. Pair provider identity with actual probe/read results or service-presence evidence.

### 7.5 Failure taxonomy

Keep these concepts separate:

- service unavailable;
- capability `BackendMissing`;
- capability `Unsupported` on this hardware;
- `TemporarilyUnavailable`;
- `PermissionDenied`;
- domain value `Unknown` after a successful read;
- collector/transport `Unknown` because no evidence could be obtained.

Do not map all of these to a single “Unavailable” badge internally, even if the compact UI uses fewer colors.

---

## 8. Security and privacy

### 8.1 Read-only invariant

The entire Diagnostics collection path must be read-only.

Allowed source classes include:

- Session1 property/method reads already defined as read-only;
- Hardware1 **status properties/getters only**;
- D-Bus service ownership/activation metadata queries;
- existing ASUS Armoury sysfs readers;
- existing keyboard LED sysfs reader;
- existing Aura read-only D-Bus provider;
- `SysfsTelemetryProvider` reads;
- Wayland output observation;
- narrow local system metadata reads.

Diagnostics must never call:

- `SetPerformanceProfile`;
- `SetChargeLimit`;
- GPU mutation methods;
- fan setters/reset;
- Panel Overdrive setter;
- keyboard backlight setter;
- Aura setter;
- arbitrary sysfs write paths.

### 8.2 Polkit

Diagnostics should not require polkit authorization merely to render.

Existing Hardware1 mutation-status properties are capability metadata and are designed to be read without authorization. The future collector should use these status getters rather than “testing” a mutation to discover writability.

If a read-only external backend independently denies a read, record `PermissionDenied`; do not request elevation automatically.

### 8.3 No generic command execution

Do not implement diagnostics by running arbitrary commands such as:

- shell pipelines;
- `systemctl` with user-supplied arguments;
- `journalctl` dumps;
- `env`;
- arbitrary `cat` paths;
- `git` commands.

Use typed APIs and fixed/narrow sources.

### 8.4 Environment privacy

The application may need a few environment values to classify the graphical session, but the contract must enforce an allowlist and normalization.

Never store/export the complete environment. Environment values can contain tokens, socket paths, usernames, home paths, SSH agent paths, proxy credentials, and application secrets.

### 8.5 Path privacy

Capability reasons and provider errors may contain filesystem or D-Bus endpoint paths. The interactive detailed UI may show a sanitized fixed Orbis/backend identifier, but export should not blindly serialize arbitrary `CapabilityReason.endpoint`, raw `ProviderError` debug output, or generic `DiagnosticEntry` values.

Use an allowlisted export projection.

---

## 9. Safe diagnostic export schema

### 9.1 Separate export model

Do not serialize the complete in-memory `DiagnosticsSnapshot` blindly.

Create a separate future `DiagnosticExportReport` projection that is explicitly allowlisted and privacy-reviewed. Text and JSON exporters should derive from the same sanitized report so their semantics do not diverge.

### 9.2 Recommended top-level sections

1. `schema`
2. `generated_at`
3. `application`
4. `system`
5. `hardware`
6. `services`
7. `capabilities`
8. `gpu`
9. `telemetry`
10. `display`

### 9.3 Safe default fields

#### Schema/application

Include:

- export schema version;
- generation timestamp in UTC/ISO-8601 or another stable machine-readable format;
- Orbis application version;
- build revision only if deliberately embedded and considered public build metadata.

#### System

Include normalized:

- kernel release;
- architecture;
- session type;
- display protocol;
- compositor only if reliably known.

Do not include arbitrary environment values.

#### Hardware

Safe default candidates:

- vendor;
- product/model;
- board/model identifier;
- BIOS version/date.

Explicitly exclude by default:

- chassis/system serial number;
- motherboard serial number;
- disk serials;
- MAC addresses;
- machine-id;
- host SSH keys;
- username;
- home directory.

If a future support workflow ever needs a unique identifier, it should be a separate explicit opt-in, not part of normal export.

#### Services

Include:

- stable Orbis-defined service ID;
- normalized availability state;
- criticality;
- last checked timestamp.

Avoid exporting unique D-Bus connection names, PIDs, executable paths, or full process metadata by default.

#### Capabilities

Include:

- `FeatureId`;
- canonical feature status;
- read operation status;
- write operation status;
- requirement/risk when represented by existing safe enums;
- backend ID if known;
- checked timestamp.

Exclude/redact by default:

- arbitrary endpoint/path strings;
- unbounded free-form provider debug dumps.

A short sanitized reason code/message may be included if it has passed the same privacy policy.

#### GPU

Include separately:

- MUX state;
- dGPU access policy;
- runtime dGPU power state;
- requested product mode only when authoritative;
- supergfxd backend current/pending/action under a clearly named backend subsection.

Never export a synthesized product mode.

#### Telemetry

For a default support report, include both availability and the latest values because temperatures/RPM/battery/power are directly useful for hardware diagnostics. Also include:

- sample timestamp;
- age/freshness;
- which values are absent.

Do not include unbounded time series unless a future explicit capture feature is designed separately.

#### Display

Include:

- runtime output name/id;
- current resolution;
- exact refresh mHz;
- observed mode list;
- preferred mode only if observed.

Clearly document that output IDs are session-local and mode lists are observations, not guaranteed full modesetting inventories.

### 9.4 Explicitly excluded export content

Default export must not contain:

- complete environment;
- arbitrary filesystem contents;
- secrets/tokens/credentials;
- machine-id;
- username/home path;
- serial numbers;
- generic journal dump;
- arbitrary process list;
- arbitrary command output;
- raw configuration files;
- raw D-Bus introspection XML;
- uncontrolled provider debug strings.

### 9.5 Timestamps

Use at least three time concepts where applicable:

- report `generated_at`;
- capability/service `checked_at`;
- telemetry sample timestamp.

This prevents a freshly exported file from making a stale telemetry sample look current.

---

## 10. Recommended architecture and data flow

### 10.1 Option A — DiagnosticsWindow polls providers directly

**Reject.**

Problems:

- Slint becomes aware of provider/runtime/transport details;
- duplicate polling lifecycle appears beside the existing worker/runtime;
- capability and observed-value semantics can drift from the main application;
- cancellation/window-close handling becomes UI-specific;
- difficult to test without a live UI;
- encourages stringly typed error handling.

The window should remain presentation-only.

### 10.2 Option B — application/runtime builds one typed snapshot

**Recommended.**

The application process already owns the relevant boundaries:

- session-bus connection;
- system-bus connection;
- Session1-backed services;
- Hardware1 status metadata connection;
- production capability registry;
- direct telemetry runtime;
- unprivileged local environment/system context;
- the correct process/session context for Wayland output observation.

A collector can combine those sources into one immutable `DiagnosticsSnapshot` and publish it through the existing async worker/UI boundary.

Benefits:

- one contract for UI and future export;
- no new privileged service;
- no new write API;
- service/capability/value failures remain independent;
- no transport details in Slint;
- hermetic collector tests can inject fake read-only sources;
- refresh/cancellation follows the existing application worker model.

### 10.3 Option C — add Diagnostics aggregation to Session1

**Do not use as the primary design.**

Session1 is currently a read-only session service for a bounded set of authoritative capability reads. Making it the owner of all diagnostics would create unnecessary coupling:

- local GUI telemetry already reads directly in-process;
- Wayland output observation belongs to the graphical client/session context;
- application build version belongs to the GUI binary;
- system/session service presence is broader than Session1’s domain;
- hardwared mutation-status metadata can be read by the original application process without moving privileged facts through sessiond.

A new Session1 diagnostics API would also make sessiond failure prevent Diagnostics from explaining that sessiond itself is unavailable.

Session1 should remain one input to the application collector, not the collector itself.

### 10.4 Exact recommended data flow

Conceptually:

`Session1 read providers`
→ existing application services
→ diagnostics collector

`Hardware1 read-only status getters`
→ capability write-status evidence
→ capability registry / diagnostics collector

`ASUS Armoury + keyboard + Aura read-only providers`
→ existing/new read-only probes
→ capability registry / diagnostics collector

`CapabilityRegistrySnapshot`
→ core `DeviceCapabilities` + registry generation/timestamp
→ diagnostics collector

`SysfsTelemetryProvider`
→ existing `Telemetry`
→ diagnostics collector with freshness/provider metadata

`WaylandDisplayOutputProvider`
→ `DisplayOutputSnapshot`
→ diagnostics collector

`org.freedesktop.DBus` name metadata`
→ service-presence records
→ diagnostics collector

`narrow system/build metadata`
→ application/system/hardware sections
→ diagnostics collector

`DiagnosticsSnapshot`
→ existing async worker boundary
→ Slint presentation DTO
→ `DiagnosticsWindow`

Separately:

`DiagnosticsSnapshot`
→ privacy allowlist/redaction projection
→ `DiagnosticExportReport`
→ text or JSON exporter

No arrow in this flow invokes a Hardware1 setter or another privileged mutation.

### 10.5 Refresh lifecycle

Recommended lifecycle:

1. Opening Diagnostics requests a new snapshot asynchronously.
2. Immediately show `Refreshing` while preserving a previous successful snapshot if one exists.
3. Fast local/direct values and existing cached registry snapshot may appear first only if the UI model explicitly supports partial publication; otherwise publish one complete aggregate after bounded parallel reads.
4. Independent source failures are captured in their own section and must not abort the entire snapshot.
5. Telemetry should reuse the latest production telemetry sample rather than launch a second high-frequency poller.
6. Capability registry should use its existing generation/check timestamp; explicit refresh may trigger the existing safe probe refresh path where designed.
7. Service presence and display observation may be refreshed on window open and on explicit Refresh; an optional slow interval is acceptable, but no high-rate polling is needed.
8. Window close cancels UI interest; the global production telemetry/runtime continues its normal lifecycle.
9. Export should request or confirm a reasonably fresh snapshot before writing the report.

---

## 11. Ordered implementation slices

All slices below have a hard invariant: **privileged hardware writes = zero**.

### Slice 1 — typed diagnostics domain contract

**Goal**

Add typed snapshot/service/telemetry-metadata/export-facing domain structures without I/O.

**Likely files**

- `crates/orbis-core/src/diagnostics.rs`
- possibly `crates/orbis-core/src/lib.rs` exports

**New interfaces/types**

- `DiagnosticsSnapshot`;
- application/system/hardware sections;
- service availability + criticality types;
- capability-registry metadata wrapper around core `DeviceCapabilities`;
- telemetry collection metadata/freshness;
- GPU diagnostics wrapper that keeps product mode optional and supergfxd backend state separate;
- display diagnostics wrapper only if existing `DisplayOutputSnapshot` cannot be embedded directly.

**Tests**

- equality/serialization where appropriate;
- canonical status preservation;
- no product-mode inference helpers;
- freshness classification boundaries;
- schema-version behavior if serialization is added.

**Risk**

Low. Main risk is duplicating existing capability/telemetry types. Prefer composition/reuse.

**Privileged writes**

MUST be zero; this slice is pure domain code.

### Slice 2 — service/capability snapshot aggregation

**Goal**

Build the read-only collector in the application/runtime process using existing connections and registry data.

**Likely files**

- `crates/orbis-ui/src/composition.rs`
- `crates/orbis-ui/src/worker.rs`
- possibly `crates/orbis-application/src/lib.rs` for transport-neutral orchestration abstractions
- `crates/orbis-session-client/src/lib.rs` for additional Hardware1 status decoders only
- `crates/orbis-providers/src/probes.rs` for missing read-only capability probes

**Work**

- expose the six existing registry capabilities directly;
- add PanelOverdrive, MiniLed, ScreenAutoBrightness, KeyboardBacklight, Aura to safe probe/registry aggregation;
- decode existing Hardware1 status properties for Panel/Keyboard/Aura without invoking setters;
- add D-Bus service-presence source for sessiond/hardwared/asusd/supergfxd/UPower;
- add privacy-safe system/build/device metadata source;
- retain per-source errors rather than failing whole snapshot.

**Tests**

- fake system/session D-Bus presence data;
- service owned / activatable / missing / transport failure;
- capability statuses preserved exactly;
- missing optional daemon changes only dependent capability/service rows;
- no setter method called in any diagnostics path;
- snapshot still publishes when sessiond or hardwared is unavailable.

**Risk**

Medium because aggregation crosses several existing runtime sources. Risk is semantic coupling, not hardware mutation.

**Privileged writes**

MUST be zero.

### Slice 3 — Wayland/display diagnostics

**Goal**

Wire the existing read-only Wayland display provider into diagnostics/application runtime.

**Likely files**

- `crates/orbis-ui/src/composition.rs`
- `crates/orbis-providers/src/wayland_output.rs` only if a small adapter is needed, not for modesetting
- `crates/orbis-providers/src/probes.rs`

**Work**

- construct/use `WaylandDisplayOutputProvider` in the graphical application context;
- publish `DisplayOutputSnapshot`;
- add `FeatureId::DisplayOutput` capability probe to runtime registry;
- retain exact mHz;
- preserve optional preferred/non-current evidence.

**Tests**

- no Wayland display;
- one/multiple outputs;
- current-only mode stream;
- preferred mode present/absent;
- duplicate/unknown output naming rules already supported by provider tests;
- prove no modesetting/write API is invoked or introduced.

**Risk**

Low-to-medium. The main risk is compositor variability and accidentally overstating completeness of mode observations.

**Privileged writes**

MUST be zero.

### Slice 4 — Diagnostics UI wiring

**Goal**

Replace preview literals with a presentation model derived from `DiagnosticsSnapshot`.

**Likely files**

- `ui/audited/diagnostics-window.slint` or its post-integration equivalent
- `crates/orbis-ui/src/main.rs`
- `crates/orbis-ui/src/controller.rs`
- `crates/orbis-ui/src/worker.rs`

**Work**

- typed snapshot → Slint DTO mapping;
- explicit Refresh action;
- separate GPU primitive rows;
- separate service rows;
- separate display/lighting capabilities;
- render canonical capability states and operation writability without inventing support;
- replace preview log area with structured details or remove it;
- no direct provider calls from Slint callbacks.

**Tests**

- snapshot-to-UI mapping for every canonical capability status;
- provider/service partial failures;
- semantic domain Unknown vs unavailable error;
- requested product GPU mode absent;
- telemetry missing fields;
- no stale preview literals in production mode.

**Risk**

Medium presentation risk; low backend risk.

**Privileged writes**

MUST be zero.

### Slice 5 — safe diagnostic export

**Goal**

Implement explicit user-triggered text/JSON export from an allowlisted privacy projection.

**Likely files**

- `crates/orbis-core/src/diagnostics.rs` for export schema if domain-owned
- or a small application/export module with no generic filesystem reader
- `crates/orbis-ui/src/worker.rs` for explicit file-write request
- Diagnostics UI callback wiring

**Work**

- `DiagnosticExportReport` allowlist;
- deterministic text and JSON rendering;
- UTC timestamp;
- redaction/omission policy;
- user-selected destination only;
- no journal command, no environment dump, no arbitrary path collection.

**Tests**

- golden/structured schema test;
- serial/machine-id/username absent;
- arbitrary environment absent;
- endpoint/free-form path data absent or sanitized;
- unknown capability values preserved;
- export contains timestamps and source freshness;
- text and JSON represent the same sanitized facts.

**Risk**

Medium privacy risk; implementation should be security-reviewed before release.

**Privileged writes**

MUST be zero. Writing the user-requested report file is ordinary user-owned output, not a hardware/privileged write.

---

## 12. What can and cannot be claimed after this design

### Can be connected without a new hardware backend

- Orbis package version;
- Performance capability;
- ChargeLimit capability;
- GPU MUX/access/runtime-power capability and values;
- FanCurves capability;
- CPU/GPU temperature;
- fan RPM;
- battery telemetry;
- GPU power telemetry;
- telemetry sample time/freshness.

### Requires small aggregation/wiring

- explicit service availability;
- system/session metadata;
- privacy-safe device identity reader;
- Panel Overdrive capability;
- MiniLED capability;
- Screen Auto Brightness capability;
- Keyboard backlight capability;
- Aura capability;
- Wayland output capability/data;
- supergfxd backend current/pending/action details;
- telemetry provider collection metadata.

These do not justify a new privileged service.

### Must remain unknown/unavailable today

- embedded source revision unless the build explicitly supplies one;
- generic compositor identity unless a reliable source is introduced;
- requested/selected Orbis product GPU mode in current production primitive runtime;
- CPU package power draw;
- preview Power limits capability until a production provider/probe path is actually wired and validated.

### Data that must never be presented as confirmed by inference

- Eco/Standard/Ultimate/Optimized synthesized from MUX/access/power;
- modesetting capability inferred from Wayland output observation;
- Panel Overdrive inferred from refresh rate;
- service health inferred only from `ProviderHealth::Healthy`;
- writable capability inferred only from read support;
- zero temperature/RPM/power inferred from missing telemetry;
- compositor name inferred from desktop-environment strings.

---

## 13. Final recommendation

Implement Diagnostics as a **typed application-owned snapshot**, not as a second control plane.

The first useful production milestone does not require new privileged backend work. Wire the 16 DIRECT items and the existing registry first, then add safe aggregation for service presence and already-existing read-only providers. The UI should become truthful incrementally: an unavailable row is preferable to a preview value or inferred state.

Recommended implementation order:

1. typed diagnostics domain contract;
2. application/runtime collector using the six existing registry entries, GPU primitives, telemetry, version, and service presence;
3. remaining existing capability probes plus Wayland display aggregation;
4. DiagnosticsWindow presentation wiring and refresh lifecycle;
5. privacy-reviewed safe export.

Do not add a Session1 diagnostics umbrella API unless a future requirement demonstrates data that must be owned by sessiond. Nothing identified in this audit requires such an API.

No Diagnostics implementation should call a mutation method for discovery, health checking, capability probing, or export generation.