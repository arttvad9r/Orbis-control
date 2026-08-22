# Orbis Control — profile-specific fan read remediation plan

**Audit date:** 2026-08-18  
**Production source:** `chatgpt/production-hardening-20260818` @ `a745f559e1dc3aeefc665b31f740242bc4cb7348`  
**Audit source:** PR #5, `docs/ui-backend-wiring-matrix.md` @ `93cf63dbc2a25492c4a38db736ef2c843251964d`  
**Research branch:** `agent/fan-profile-read-remediation`  
**Scope:** research/documentation only. No Rust, Slint, Nix, CI, deployment, or hardware changes are made by this branch.

The hardening HEAD was fetched again immediately before this document commit and remained `a745f559e1dc3aeefc665b31f740242bc4cb7348`.

---

## 1. Executive summary

PR #5 identified a real composition defect in the production fan editor. The UI requests a **profile-specific** curve using the lossless `AsusdFanProfile`, but `build_production_runtime()` supplies `SysfsFanCurveProvider` as the read side of `SessionHardwareFanCurveProvider`. `SysfsFanCurveProvider` is intentionally an **active-curve-only** provider: kernel `asus_custom_fan_curve` exposes the currently active curve, not independent stored curves for Balanced / Performance / Quiet / LowPower. Its `fan_curve_for_profile()` therefore correctly returns `ProviderError::Unsupported` rather than pretending the active curve belongs to the requested profile.

The required data source already exists. `orbis-sessiond/src/fans.rs` contains the typed, read-only `ZbusAsusdFanCurveSource`, which calls `xyz.ljones.FanCurves.FanCurveData(profile)` with `CacheProperties::No`, parses CPU/GPU curves into typed temperatures/raw PWM, and preserves the four lossless `AsusdFanProfile` wire values.

The recommended remediation is **not** to replace the sysfs provider with a direct asusd call in the GUI. That would fix the immediate symptom but retain the composition-level read-boundary deviation already documented by PR #5. The architecture-correct minimal fix is:

1. keep `SysfsFanCurveSource` as the authoritative source for **active curve** probing;
2. use the existing `ZbusAsusdFanCurveSource` as the authoritative source for **profile-specific stored curves**;
3. expose those read-only fan operations through `Session1`/`orbis-sessiond`;
4. make the GUI-side fan read provider a Session1 client;
5. leave `SessionHardwareFanCurveProvider`'s mutation side, Hardware1, polkit, typed asusd mutation backend, and hardwared post-write read-back unchanged.

This preserves the intended Orbis boundary: **read-only state through sessiond/Session1; privileged mutation directly from the GUI process to Hardware1 so original caller identity is preserved**.

No new privileged write is required.

---

# 2. Root cause

## 2.1 Current production composition

`crates/orbis-ui/src/composition.rs::build_production_runtime()` currently constructs the fan service approximately as this semantic graph:

`SysfsFanCurveSource`
→ `SysfsFanCurveProvider`
→ `SessionHardwareFanCurveProvider<read, Hardware1 mutation>`
→ `AppService`
→ `ApplicationRuntime.fan`

The mutation half is valid. The read half is not valid for the command the UI actually sends.

`SessionHardwareFanCurveProvider` does not add profile semantics itself. Its `FanProvider` implementation delegates:

- `fan_curve_for_profile(profile, fan)` → inner read provider;
- `active_curve(fan)` → inner read provider.

Therefore the semantics of the inner read provider are decisive.

## 2.2 Sysfs provider contract is intentionally active-only

`crates/orbis-sessiond/src/fans.rs::SysfsFanCurveProvider` correctly documents that kernel `asus_custom_fan_curve` represents only the active curve. It intentionally returns `Unsupported` from both profile-oriented methods rather than returning an active curve under a false profile label.

That behavior must **not** be changed. Making `SysfsFanCurveProvider::fan_curve_for_profile()` return `active_curve()` would hide the defect and create false data.

## 2.3 UI requires a profile-specific read

The fan UI carries a lossless profile selector:

- index 0 → `AsusdFanProfile::Balanced`;
- index 1 → `AsusdFanProfile::Performance`;
- index 2 → `AsusdFanProfile::Quiet`;
- index 3 → `AsusdFanProfile::LowPower`.

CPU/GPU tab changes and profile changes send:

`WorkerCommand::RefreshFanCurve { profile, fan }`.

The worker then executes:

`runtime.fan.fan_curve_for_profile(profile, fan)`.

The current resulting production path is therefore:

`FansWindow`
→ callback selects lossless `AsusdFanProfile` + `FanId`
→ `WorkerCommand::RefreshFanCurve`
→ `FanServiceRuntime::fan_curve_for_profile`
→ `AppService::fan_curve_for_profile`
→ `SessionHardwareFanCurveProvider::fan_curve_for_profile`
→ `SysfsFanCurveProvider::fan_curve_for_profile`
→ `ProviderError::Unsupported`.

`main.rs` converts that read failure to `FanCurveHwState::Unavailable` and `fan_curve_error=true`. No default curve is substituted.

## 2.4 The correct backend already exists but is not composed

`ZbusAsusdFanCurveSource` already performs a fresh typed call to:

- service: `xyz.ljones.Asusd`;
- interface: `xyz.ljones.FanCurves`;
- object path: `/xyz/ljones`;
- method: `FanCurveData(profile)`.

Its output is `AsusdFanCurveSet` containing the requested lossless profile plus CPU and GPU `AsusdFanCurve` values. Each curve contains:

- `FanId`;
- 8 typed temperatures;
- 8 raw `FanPwm` values in the 0..255 domain;
- `enabled`.

The source performs no write.

The defect is therefore a **composition/transport wiring defect, not missing backend support**.

---

# 3. Current full flow

## 3.1 Profile read — defective path

| Layer | Current behavior |
|---|---|
| Slint/Rust UI | CPU/GPU tab or fan-profile selector chooses `FanId` + lossless `AsusdFanProfile`. |
| UI callback | Sends `WorkerCommand::RefreshFanCurve { profile, fan }`. |
| Worker | Calls `runtime.fan.fan_curve_for_profile(profile, fan)`. No fallback/default. |
| Application | `AppService::fan_curve_for_profile` delegates to `FanProvider`. |
| Composed fan provider | `SessionHardwareFanCurveProvider` delegates reads to its `session`/read provider. |
| Production read provider | Currently `SysfsFanCurveProvider<SysfsFanCurveSource>`. |
| Backend | Kernel `asus_custom_fan_curve`, active curve only. |
| Result | `Unsupported` for profile-specific read. |
| UI failure representation | `FanCurveHwState::Unavailable`, fan error flag; no mock/default curve. |

## 3.2 Active curve — valid path

`SysfsFanCurveProvider::active_curve(fan)` is valid and authoritative for the currently active curve.

The capability probe `probe_fan_curve()` explicitly calls `active_curve(fan)` and discards the returned points after proving the read contract. This is why the sysfs provider still has value even after profile-specific reads move away from it.

The active-curve provider should remain unchanged and read-only.

## 3.3 Custom mutation — valid path, must remain unchanged

Current custom curve mutation is conceptually:

`FansWindow Apply`
→ `WorkerCommand::SetFanCurve { profile, fan, curve }`
→ `FanServiceRuntime::set_fan_curve`
→ `AppService::set_fan_curve`
→ `SessionHardwareFanCurveProvider::set_fan_curve`
→ GUI-owned `ZbusHardwareFanCurveSource`
→ system bus `Hardware1.SetFanCurve`
→ fan polkit action
→ `AsusdFanCurveMutationBackend`
→ typed `xyz.ljones.FanCurves.SetFanCurve`
→ fresh `FanCurveData(profile)` read-back inside hardwared
→ exact requested fan curve comparison
→ `ApplyResult::Applied` only after confirmation.

This path is correct for the remediation scope and must not be changed.

## 3.4 Factory Defaults — valid mutation plus currently correct refresh intent

Factory Defaults is a separate explicit mutation path. The GUI routes it through the worker FIFO (`WorkerCommand::ResetFanCurvesToDefaults`); hardwared uses `AsusdFanCurveMutationBackend::reset_curves_to_defaults(profile)`, then performs a fresh `FanCurveData(profile)` observation. Because no independent default-evidence source exists, the honest success result is `ApplyResult::Accepted` (not `Applied`).

After `ApplyResult::Accepted`, current `main.rs` already queues:

`WorkerCommand::RefreshFanCurve { profile, fan }`.

Today that refresh reaches the wrong sysfs profile-read provider and fails. Once the profile read composition is corrected, the existing post-reset refresh becomes useful without changing the privileged mutation path.

---

# 4. Recommended read architecture

## 4.1 Recommendation: profile-specific fan reads should go through Session1

Yes. The profile-specific read should be a read-only Session1 operation.

Reasons:

1. ADR 0004 establishes Session1 as the authoritative read-only session boundary and explicitly prefers fresh getter operations over hidden stale caches.
2. Battery, Performance and GPU primitive production reads already follow GUI → Session1 → sessiond ownership.
3. `SessionHardwareFanCurveProvider` is already designed/documented as a composed **session read + direct Hardware1 mutation** provider.
4. PR #5 already classifies the direct GUI fan read composition as an architecture deviation.
5. `FanCurveData(profile)` is read-only. Moving it behind Session1 does not move privileged mutation into sessiond and does not require polkit.
6. `orbis-sessiond` already opens/owns a system-bus connection during bootstrap, so `ZbusAsusdFanCurveSource` can reuse that connection without another process boundary or privileged service.

### Recommended production data flow

Profile-specific read:

`FansWindow`
→ worker `RefreshFanCurve(profile, fan)`
→ application `FanServiceRuntime`
→ unchanged `SessionHardwareFanCurveProvider`
→ new GUI-side Session1 fan read provider
→ Session1 read-only fan method
→ `SessionService`
→ sessiond fan read provider
→ existing `ZbusAsusdFanCurveSource`
→ `xyz.ljones.Asusd / xyz.ljones.FanCurves.FanCurveData(profile)`
→ typed curve
→ reverse path to UI.

Active-curve capability read:

`Capability refresh`
→ `probe_fan_curve(..., fan, mutation_status)`
→ same GUI-side Session1 fan read provider `.active_curve(fan)`
→ Session1 read-only active-curve method
→ sessiond fan read provider
→ existing `SysfsFanCurveSource`
→ kernel `asus_custom_fan_curve`
→ capability metadata only.

Mutation remains:

`GUI process`
→ `Hardware1`
→ polkit
→ hardwared typed asusd mutation
→ hardwared fresh read-back.

Sessiond is never a privileged mutation deputy.

## 4.2 Reuse `ZbusAsusdFanCurveSource`

Yes. Reuse it on the **sessiond side**.

Do not duplicate the proven `FanCurveData` D-Bus proxy contract in UI code. A new profile-aware `FanProvider` adapter can wrap `AsusdFanCurveSource` and convert the selected `AsusdFanCurve` into the existing domain `FanCurve` expected by `FanProvider::fan_curve_for_profile`.

The adapter should not cache values and should perform exactly one fresh `read_curves(profile)` per requested read.

## 4.3 Keep sysfs active-curve authority separate

`SysfsFanCurveSource`/`SysfsFanCurveProvider` should remain the active-curve authority because that is the semantics the kernel ABI actually proves.

There are two acceptable implementation shapes:

### Preferred shape

One sessiond-side read provider composes two read-only sources:

- profile source: `ZbusAsusdFanCurveSource`;
- active source: `SysfsFanCurveSource`.

Its `FanProvider` semantics are:

- `fan_curve_for_profile(AsusdFanProfile, fan)` → asusd profile source;
- `active_curve(fan)` → sysfs active source;
- mutations → always `Unsupported` on this read provider.

Session1 exposes both read operations, and the GUI-side `SessionFanCurveProvider` implements the same `FanProvider` surface. The existing outer `SessionHardwareFanCurveProvider` then remains unchanged.

### Acceptable but less clean shape

Expose only profile-specific reads through Session1 and retain a local GUI-side composite provider that delegates `active_curve()` to `SysfsFanCurveProvider` for capability probing.

This is fewer Session1 operations, but the process still performs a direct sysfs fan read. Since PR #5 already identifies direct fan reads as architecture debt, the preferred shape is to move **both** fan read operations behind Session1 while retaining sysfs as the actual active-curve source in sessiond.

## 4.4 Rejected shortcuts

### Shortcut A — make sysfs pretend to be profile-specific

Reject. `active_curve()` is not evidence for the selected stored profile. Returning it from `fan_curve_for_profile()` would be false state.

### Shortcut B — instantiate `ZbusAsusdFanCurveSource` directly in `orbis-ui`

It would fix the immediate UI defect but preserve the known architecture deviation and duplicate system-bus read ownership in the GUI. Not recommended as the production remediation.

### Shortcut C — synthesize Quiet/LowPower from `PerformanceProfile::Silent`

Reject. The domain explicitly declares this mapping ambiguous.

### Shortcut D — move fan mutation through Session1

Reject. Mutation must remain GUI → Hardware1 so the original system-bus caller is preserved for polkit authorization.

---

# 5. Profile semantics

## 5.1 Canonical asusd fan profile mapping

The proven lossless mapping is:

| `AsusdFanProfile` | asusd wire | Semantic |
|---|---:|---|
| `Balanced` | 0 | balanced |
| `Performance` | 1 | performance |
| `Quiet` | 2 | quiet |
| `LowPower` | 3 | low-power |

The fan editor already uses this four-state type and must continue doing so.

## 5.2 Relationship to Silent / Balanced / Turbo

`PerformanceProfile` is a different three-state product/domain concept.

Current domain mapping is:

| `PerformanceProfile` | Fan-profile interpretation |
|---|---|
| `Balanced` | unambiguously `AsusdFanProfile::Balanced` |
| `Turbo` | unambiguously `AsusdFanProfile::Performance` |
| `Silent` | **ambiguous:** could be `Quiet` or `LowPower`; automatic conversion is rejected by the domain |

Therefore remediation must use `fan_curve_for_profile(AsusdFanProfile, fan)` and must **not** route the fan editor through legacy `fan_curve(PerformanceProfile, fan)`.

Quiet and LowPower must remain distinguishable through UI → worker → Session1 → sessiond → asusd.

## 5.3 CPU/GPU selection

The existing asusd parser recognizes wire names exactly:

- `"CPU"` → `FanId::Cpu`;
- `"GPU"` → `FanId::Gpu`.

Unknown fan names are currently treated as malformed backend payload (`Internal`), not silently ignored or mapped.

The UI only exposes CPU/GPU tabs. The Session1 fan wire contract should therefore accept only canonical CPU/GPU fan IDs. Invalid future/user wire input should be rejected rather than normalized.

The existing `ZbusAsusdFanCurveSource` models a full set containing both CPU and GPU and treats a missing required curve as `Internal`. The minimal remediation can reuse this behavior; it must not invent a zero/default curve when either required entry is missing. If Orbis later supports genuine one-fan devices through asusd, that should be a separate source-contract change with explicit optional-fan semantics rather than part of this defect fix.

---

# 6. Error and failure semantics

## 6.1 Required behavior

Profile read must preserve the distinction between:

- structurally unsupported operation/interface;
- backend/service unavailable;
- permission denied;
- malformed/inconsistent payload;
- successful authoritative read.

No error class may produce a fabricated curve.

## 6.2 Existing `ZbusAsusdFanCurveSource` limitation

The source currently wraps proxy/method failures as `ProviderError::Dbus(...)`. It does not directly emit `BackendUnavailable`, `Unsupported`, or `PermissionDenied` variants for every zbus error.

That does not block reusing the source, because Orbis already has D-Bus detail classification for capability probing. However the remediation tests should explicitly cover the relevant D-Bus names and ensure they remain distinguishable at the provider/probe boundary where the current architecture supports that distinction.

Recommended classifications for read-side evidence:

| Evidence | Semantic result |
|---|---|
| `ServiceUnknown` / `NameHasNoOwner` | backend unavailable / backend missing evidence |
| `UnknownMethod` / `UnknownInterface` / `NotSupported` | `Unsupported` |
| `AccessDenied` | `PermissionDenied` |
| timeout / no reply / disconnect | temporarily unavailable |
| malformed profile/fan/curve payload | `Internal` |

A protocol-wide redesign of Session1 error taxonomy is **not** required for this remediation. Current Session1 maps backend failures through standard D-Bus errors; UI can still represent an unreadable curve as `Unavailable`. Do not expand this focused fix into a general error-protocol migration.

## 6.3 No optimistic/default curves

The corrected path must retain all current fail-closed behavior:

- no default curve when asusd is absent;
- no active sysfs curve substituted for a requested profile;
- no zero-filled production curve on failed read;
- no fallback from Quiet to LowPower or vice versa;
- no stale profile cache presented as a fresh read;
- no success derived from the request itself.

`ZbusAsusdFanCurveSource` already uses `CacheProperties::No` and performs a fresh method call.

The UI may retain last-known displayed data if that is its general error policy, but it must mark the fan state unavailable/stale rather than re-label it as authoritative for the failed request.

---

# 7. Apply and Factory Defaults refresh semantics

## 7.1 Custom Apply

Current custom mutation already has authoritative confirmation inside hardwared:

1. validate request;
2. call typed asusd `SetFanCurve` for the selected fan;
3. fresh `FanCurveData(profile)` read-back;
4. compare requested temperatures/PWM for the selected fan;
5. return `ApplyResult::Applied` only if they match.

Therefore current UI success is **not optimistic**, even though the GUI does not issue a second independent `RefreshFanCurve` command immediately after custom Apply. The displayed edited points are exactly the values hardwared confirmed.

For the minimal root-cause remediation, do **not** change this mutation flow.

A separate follow-up may choose to issue a Session1 `RefreshFanCurve` after confirmed Apply for uniform UI convergence. That is optional, not required to make success authoritative, and should not be bundled into the first read-composition fix unless the event model is changed carefully. Blindly queueing refresh before knowing whether mutation succeeded could overwrite the user's dirty/error state after a failed apply.

## 7.2 Factory Defaults

Factory Defaults is different because the resulting default curve is not known from the request.

Current behavior is correct in principle:

1. explicit user Apply triggers the worker FIFO `ResetFanCurvesToDefaults` → Hardware1 reset;
2. hardwared performs asusd reset and fresh `FanCurveData(profile)` observation;
3. only on `ApplyResult::Accepted`, GUI queues `RefreshFanCurve(profile, selected_fan)` (refresh applies to `Accepted`; `Unconfirmed` never triggers a refresh);
4. corrected profile-specific read must then load the actual default points into the editor.

This post-reset refresh must remain and is a key targeted regression test.

---

# 8. Mutation/security boundary — explicitly unchanged

The remediation must not modify:

- `Hardware1.SetFanCurve`;
- `Hardware1.ResetFanCurvesToDefaults`;
- fan polkit action or authorization policy;
- original-caller authorization design;
- `AsusdFanCurveMutationBackend` ownership;
- typed asusd `SetFanCurve`/`SetCurvesToDefaults` calls;
- hardwared validation;
- hardwared fresh `FanCurveData` post-write/read-back requirement;
- `ApplyResult::Applied` semantics.

The new/changed operations in this remediation are read-only only.

No new root/system service is required. No generic D-Bus command execution or filesystem path API is required.

---

# 9. Proposed Session1 fan read contract

This document does not implement the protocol, but the future implementation should keep the contract narrow and typed.

Required read operations:

1. **profile-specific fan curve** — inputs: canonical `AsusdFanProfile` + canonical CPU/GPU fan; output: one authoritative curve;
2. **active fan curve** — input: canonical CPU/GPU fan; output: one authoritative active curve for capability/read diagnostics.

The protocol should carry:

- profile identity where applicable;
- fan identity;
- exactly 8 temperature points for the asusd profile contract;
- exactly 8 raw PWM values (0..255, never percent).

Do not expose:

- arbitrary D-Bus destination/path/interface/method names;
- arbitrary sysfs paths;
- arbitrary curve length without validation;
- mutation operations on Session1.

The Session1 client boundary must strictly decode/validate the wire payload and reject unknown fan/profile wire values instead of coercing them.

---

# 10. Minimal implementation plan

## Commit 1 — profile-aware read provider in sessiond

**Goal:** turn the already-existing sources into one honest read-only `FanProvider`.

Likely files:

- `crates/orbis-sessiond/src/fans.rs`

Work:

- add a small read-only provider/adapter that owns:
  - `AsusdFanCurveSource` for profile-specific reads;
  - `FanCurveSource`/sysfs for active reads;
- `fan_curve_for_profile(profile, fan)` performs fresh `read_curves(profile)` and selects CPU/GPU;
- `active_curve(fan)` delegates to sysfs active source;
- mutation methods remain Unsupported on this read provider;
- preserve raw PWM and typed temperatures;
- preserve lossless `AsusdFanProfile` input;
- do not synthesize `PerformanceProfile::Silent`.

Risk: low/medium. Read-only, but source selection/error semantics must be exact.

Privileged writes: **zero**.

## Commit 2 — expose fan reads through Session1

**Goal:** make fan reads use the existing authoritative session boundary.

Likely files:

- `crates/orbis-session-protocol/src/lib.rs`
- `crates/orbis-sessiond/src/service.rs`
- `crates/orbis-sessiond/src/server.rs`
- `crates/orbis-sessiond/src/composition.rs`
- `crates/orbis-sessiond/src/bootstrap.rs`
- `crates/orbis-session-client/src/lib.rs`
- existing session protocol/client P2P tests

Work:

- add narrow read-only Session1 fan operation(s);
- add optional/read-only fan provider ownership to `SessionService`;
- construct server-side fan provider with:
  - existing `ZbusAsusdFanCurveSource` using the already-open system connection;
  - existing `SysfsFanCurveSource`;
- add strict GUI-side Session1 fan source/provider implementing `FanProvider` read methods;
- preserve standard Session1 error mapping and fresh-read behavior;
- no Session1 mutation API.

Risk: medium because it extends a stable D-Bus read contract; still zero hardware writes.

Privileged writes: **zero**.

## Commit 3 — switch production UI composition to Session1 fan reads

**Goal:** correct the defect without touching mutation.

Likely files:

- `crates/orbis-ui/src/composition.rs`
- composition/worker tests only as required

Work:

- replace `SysfsFanCurveProvider` as the inner read side of the production `SessionHardwareFanCurveProvider` with the new Session1 fan read provider;
- continue constructing `ZbusHardwareFanCurveSource(system_connection)` exactly as today for mutation;
- capability probe still calls `active_curve`, now routed through Session1 to the same sysfs source;
- profile refresh now routes through Session1 to asusd `FanCurveData(profile)`.

Expected resulting composition:

`SessionFanCurveProvider(session_connection)`
→ unchanged `SessionHardwareFanCurveProvider<read, Hardware1 mutation>`
→ `AppService`
→ worker/UI.

Risk: low once Commit 2 is proven by P2P tests.

Privileged writes: **zero new writes; existing mutation path unchanged**.

## Optional follow-up — uniform post-Apply UI refresh

Not required for the root defect.

If product policy later requires every successful custom Apply to be reloaded through the same Session1 read path, implement it as a separate commit with explicit success ordering and profile/fan context. Do not queue a refresh unconditionally before mutation success is known.

Factory Defaults already has the required post-`Accepted` refresh and does not need this follow-up. A `Unconfirmed` reset result never triggers a refresh and is never escalated to success.

---

# 11. Targeted tests

## 11.1 sessiond fan provider tests

Add hermetic tests for the new read provider:

1. `Balanced` requests wire/profile 0 and returns exact 8 CPU points.
2. `Performance` requests wire/profile 1 and returns exact 8 GPU points.
3. `Quiet` remains distinct from `LowPower`.
4. `LowPower` remains distinct from `Quiet`.
5. CPU request returns CPU curve, GPU request returns GPU curve.
6. unknown/invalid fan input is rejected; never mapped to CPU/GPU.
7. malformed temp/PWM payload is `Internal`.
8. missing required curve is an error; no zero/default curve.
9. asusd read error propagates; no sysfs fallback for profile read.
10. `active_curve` uses sysfs source and never invokes asusd profile read.
11. profile read uses asusd source and never substitutes sysfs active curve.
12. sequential calls observe sequential source values; no cache.

## 11.2 profile mapping tests

Retain/add explicit tests that:

- `AsusdFanProfile::{Balanced,Performance,Quiet,LowPower}` wire values are exactly 0/1/2/3;
- `Balanced` → product `Balanced` and `Performance` → product `Turbo` are unambiguous;
- `Quiet` and `LowPower` both map read-only to product `Silent`;
- reverse `PerformanceProfile::Silent` → `AsusdFanProfile` remains an error;
- UI profile index mapping 0/1/2/3 remains lossless.

No remediation code should make Silent choose Quiet or LowPower automatically.

## 11.3 Session1 protocol/P2P tests

Add P2P tests proving:

1. profile + CPU request round-trips exact curve values;
2. profile + GPU request round-trips exact curve values;
3. active curve round-trips exact values;
4. raw PWM >100 is preserved as raw PWM, not percent;
5. unsupported backend becomes an unsuccessful read, not an empty curve;
6. AccessDenied remains a denied read rather than Unsupported/success;
7. backend/service failure remains an unavailable/failure result;
8. malformed wire length/value is rejected at client boundary;
9. each call is fresh/no property cache;
10. no mutation method appears in Session1.

## 11.4 GUI/application composition tests

Add a targeted composition test with distinct sentinel data:

- sysfs active CPU curve = A;
- asusd Balanced CPU curve = B;
- asusd Quiet CPU curve = C.

Assertions:

1. capability `probe_fan_curve` sees active curve A;
2. `RefreshFanCurve(Balanced, CPU)` returns B, never A;
3. `RefreshFanCurve(Quiet, CPU)` returns C;
4. changing CPU/GPU selects the requested fan only;
5. `ProviderError` does not install a default curve;
6. outer `SessionHardwareFanCurveProvider` mutation backend object is unchanged.

This single sentinel test is the clearest regression guard for the PR #5 defect.

## 11.5 Apply/defaults tests

Preserve mutation tests proving:

- Hardware1 custom mutation calls exactly one selected-fan setter;
- hardwared performs fresh post-write `FanCurveData` read-back;
- mismatched read-back does not return `Applied`;
- PermissionDenied/error remains an error;
- factory reset is profile-wide and requires fresh readable `FanCurveData` afterwards.

Add/retain UI orchestration test:

- Factory Defaults queues `RefreshFanCurve(profile, fan)` **only after** `ApplyResult::Accepted`;
- `Unconfirmed` factory reset preserves UI state and does not set a definitive error;
- the refresh receives the profile curve from the new Session1/asusd read path.

For custom Apply, retain the current authoritative `Applied` test. A second UI refresh is optional and should be tested only if implemented as the separate follow-up described above.

---

# 12. Files likely touched by the future fix

Minimum architecture-correct implementation set:

| File | Reason |
|---|---|
| `crates/orbis-sessiond/src/fans.rs` | Compose typed asusd profile read + sysfs active read into honest `FanProvider`. |
| `crates/orbis-session-protocol/src/lib.rs` | Add narrow read-only fan wire contract. |
| `crates/orbis-sessiond/src/service.rs` | Serve fan reads from `SessionService`. |
| `crates/orbis-sessiond/src/server.rs` | Inject fan read provider into service. |
| `crates/orbis-sessiond/src/composition.rs` | Thread fan provider through session composition. |
| `crates/orbis-sessiond/src/bootstrap.rs` | Instantiate/reuse `ZbusAsusdFanCurveSource` and sysfs source with production connections. |
| `crates/orbis-session-client/src/lib.rs` | Decode Session1 fan reads into `FanProvider`. |
| `crates/orbis-ui/src/composition.rs` | Swap production read side from direct sysfs provider to Session1 fan provider. |
| existing fan/session P2P/unit test files | Regression coverage. |

Files that should **not** need semantic changes for this defect:

- `crates/orbis-hardwared/src/fans.rs`;
- Hardware1 fan methods;
- fan polkit policy;
- Slint files;
- Nix/systemd packaging;
- CI configuration.

`crates/orbis-ui/src/main.rs` and `worker.rs` should not require functional changes for the root fix if their current commands/events remain compatible. Their existing refresh behavior should be covered by tests.

---

# 13. Acceptance criteria

The defect is remediated only when all of the following are true:

1. Initial `RefreshFanCurve(Balanced, CPU)` succeeds in production when asusd exposes the curve.
2. Changing fan profile reads the selected stored profile, not the active sysfs curve.
3. Quiet and LowPower remain distinct end-to-end.
4. Changing CPU/GPU reads the selected fan.
5. Capability probing still uses the active sysfs curve authority.
6. asusd absence/read failure produces unavailable/error state, never a default curve.
7. unsupported and permission-denied evidence is not converted to successful data.
8. Custom Apply still uses Hardware1 + polkit + typed asusd mutation + mandatory hardwared read-back.
9. Factory Defaults still refreshes the selected profile/fan after confirmed `Accepted`; `Unconfirmed` never refreshes and never claims success.
10. No Session1 mutation method is introduced.
11. No new privileged write path exists.
12. No hardware write is required to validate the read-side implementation tests.

---

# 14. Final recommendation

Treat PR #5 D1 as a **read-source ownership mismatch**:

- sysfs is correct for `active_curve`;
- asusd `FanCurveData(profile)` is correct for profile-specific stored curves;
- Session1 is the correct production transport/ownership boundary for both read-only operations;
- Hardware1 remains the exclusive privileged mutation boundary.

The smallest production-quality sequence is three commits:

1. build the profile-aware/active read provider from the two already-proven sources;
2. expose that read contract through Session1 and add P2P coverage;
3. switch production UI composition to the Session1 fan read provider and add the sentinel regression test.

Do not fix the bug by returning the active sysfs curve for a profile request, by guessing Silent → Quiet/LowPower, or by moving mutation into sessiond.
