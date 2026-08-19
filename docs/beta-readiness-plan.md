# Orbis Control — beta readiness plan

**Plan date:** 2026-08-18  
**Scope:** audit/documentation only  
**Latest production source re-fetched immediately before commit:** `chatgpt/production-hardening-20260818` @ `ea286e32d7e86f7965ba101865d2feee5dcf0342`  
**Base branch:** `main` @ `b9bfdf5bb08fecea58589bcd204215b49384d9ef`  
**Research branch:** `agent/beta-readiness-plan`  
**Production changes in this branch:** none  
**Hardware writes performed by this task:** none

## Source set

This plan consolidates the findings and designs from:

- PR #5 — `docs/ui-backend-wiring-matrix.md`, commit `93cf63dbc2a25492c4a38db736ef2c843251964d`;
- PR #6 — `docs/nixos-production-deployment-audit.md`, commit `4f19f5c0c835e33f9a1885209625f5a32ca03d6c`;
- PR #7 — `docs/nixos-deployment-remediation-plan.md`, commit `d888232ac55900bc87ed2268f4328bb3d7ebe71e`;
- PR #8 — `docs/diagnostics-backend-design.md`, commit `1d1a1eaa7bd245da46d585f97579d29dbabcc3e2`;
- PR #9 — `docs/fan-profile-read-remediation.md`, commit `f36973d828aeac8cfaefadb0012fbe80d92b2cc2`;
- PR #10 — `docs/preferences-persistence-design.md`, commit `c7467094bfe1b8245e14a648af84178dc35b1122`.

PRs #5–#9 were originally audited against hardening `a745f559e1dc3aeefc665b31f740242bc4cb7348`. PR #10 was audited against `ea286e32d7e86f7965ba101865d2feee5dcf0342`. Because the hardening branch moved substantially after the earlier audits, this consolidation does **not** blindly assume every old finding still exists. The highest-priority findings were rechecked directly against the latest hardening source before this plan was committed.

At `ea286e32...`, the following remain present:

- sessiond startup still calls `discover_battery()` before Session1 is built;
- the NixOS module still does not enable `services.upower` or `security.polkit`;
- hardwared still has `/sys` read-only with only `platform_profile` reopened writable;
- keyboard mutation status still returns `Supported` unconditionally;
- production fan composition still uses `SysfsFanCurveProvider` for a UI path that calls `fan_curve_for_profile()`;
- theme state is still process-local and initialized from `THEME_LIGHT`, not persisted preferences;
- Diagnostics and most Preferences/Automation surfaces remain preview/local UI rather than production persistence/data clients.

This document therefore treats those findings as current, not merely historical.

---

# 1. Readiness policy

## 1.1 Four readiness buckets

Every backlog item is assigned exactly one primary scheduling bucket:

1. **MUST before first packaged acceptance test** — a defect that makes a fresh packaged NixOS test non-self-contained, misleading, or structurally incapable of exercising an advertised mutation path correctly.
2. **MUST before beta** — the first packaged test may begin without it, but the defect must be closed before the beta build is presented as a usable Orbis beta.
3. **SHOULD before beta** — high-value production polish or observability that should land if time permits, but may be deferred if the corresponding UI remains explicitly preview-only/disabled and no production claim is made.
4. **post-beta** — work that is either architecture cleanup, semantically unproven functionality, or optional product expansion.

## 1.2 Preview-only rule

A preview-only control is **not** automatically a beta blocker.

If Orbis clearly presents a surface as preview-only/unavailable and it performs no hidden mutation, then missing backend wiring is not a blocker merely because the visual control exists. This applies especially to product GPU modes, display mutation controls, several lighting controls, Automation execution, Updates actions, and Diagnostics actions while they remain honest previews.

A preview surface becomes a blocker if it starts claiming persistence, support, successful execution, or authoritative state that the backend does not actually provide.

## 1.3 Severity scale

- **Critical** — prevents a self-contained packaged lifecycle or can make a core service unavailable as a whole.
- **High** — breaks an advertised production path, creates false capability state, or prevents a core beta feature from functioning correctly.
- **Medium** — meaningful usability/diagnostic gap with bounded failure and no silent privileged side effect.
- **Low** — polish, architecture debt, optional platform coverage, or explicitly deferred preview functionality.

## 1.4 Implementation-size scale

- **XS** — one small file or operational/configuration change with narrow tests.
- **S** — one subsystem, usually a few files and focused tests.
- **M** — cross-crate or protocol/composition change with targeted integration tests.
- **L** — broad aggregation/product work spanning multiple subsystems and UI presentation.

## 1.5 Hardware-write risk scale

This column describes the implementation slice itself, not the later manual acceptance test.

- **None** — no hardware mutation is needed or introduced.
- **Read-only** — implementation adds/probes reads only.
- **Existing bounded path** — no new mutation semantics, but the change affects deployment/availability of an already-existing typed mutation path; later acceptance may deliberately exercise it.
- **Deferred privileged design** — implementation must not proceed until a separate authorization/executor design exists.

---

# 2. Unified backlog

| ID / blocker | Bucket | Severity | User impact | Affected subsystem | Implementation size | Dependency | Hardware-write risk |
|---|---|---|---|---|---|---|---|
| **DEP-1 — Sessiond UPower resilience** | MUST before first packaged acceptance test | Critical | On a fresh/minimal system, missing/unavailable/no-battery UPower evidence can stop the entire Session1 service instead of degrading only Battery | `orbis-sessiond`, Battery composition, NixOS integration | M | None; NixOS UPower default should land with/after resilient provider | Read-only |
| **DEP-2 — Enable PolicyKit authority in full NixOS module** | MUST before first packaged acceptance test | Critical | Action XML may be installed while `org.freedesktop.PolicyKit1` is absent; all Hardware1 mutations then fail closed on fresh install | NixOS module, Hardware1 authorization lifecycle | XS | None; should be serialized with other `module.nix` edits | None |
| **DEP-3 — Keyboard LED sandbox exception** | MUST before first packaged acceptance test | High | Hardware1 exposes keyboard mutation but the production mount namespace keeps its exact sysfs write path read-only | NixOS hardwared systemd sandbox | XS | None; same `module.nix` conflict surface as DEP-2/UPower integration | Existing bounded path |
| **DEP-4 — Keyboard mutation capability honesty** | MUST before first packaged acceptance test | High | UI/client can be told keyboard mutation is supported even when LED ABI is absent/unreadable or deployment is unusable | `orbis-hardwared` keyboard backend/status | S | Independent of DEP-3 for implementation; acceptance meaning improves when both are present | Read-only probe |
| **OPS-1 — Remove standalone dev hardwared before packaged test** | Acceptance-environment prerequisite | High | A stale `/usr/local`/standalone service can compete for the same Hardware1 D-Bus name and invalidate packaged-test conclusions | Test/deployment environment | XS | Before any full packaged acceptance run | None |
| **FAN-1 — Profile-specific fan read composition** | MUST before beta | High | Fan editor profile refresh returns `Unsupported`; normal profile editing/default refresh cannot be trusted as a complete production UI flow | Session1/sessiond fan reads, session client, UI composition | M | Prefer after DEP-1 because both touch sessiond composition/bootstrap surfaces | Read-only |
| **CFG-1 — Production preferences storage boundary** | MUST before beta | High | Existing config mixes UI preferences, automation policy, desired hardware intent, and experimental gates; migration/corruption/atomicity are only partial | `orbis-config` | M | None; prerequisite for UI-1 | None |
| **UI-1 — Persist Dark/Light theme before first render** | MUST before beta | Medium | Theme is a real cross-window feature but resets each process; persisted Light would otherwise risk a visible Dark→Light flash if loaded too late | UI lifecycle + preferences | S | CFG-1 | None |
| **START-1 — Real user-level Run on Startup integration** | SHOULD before beta; not a gate if explicitly preview-only | Medium | Visible startup toggles currently change local booleans only; storing a bool would falsely imply real desktop integration | XDG Autostart, UI, package desktop metadata | M | Prefer after CFG-1/UI-1; needs stable packaged command/desktop-entry semantics | None |
| **DIAG-1 — Typed DiagnosticsSnapshot aggregation and window wiring** | SHOULD before beta; not a gate if explicitly preview-only | Medium | Diagnostics currently shows preview/static facts rather than authoritative runtime/capability/service state | core/application runtime, diagnostics UI | L | Prefer after FAN-1 and deployment fixes so Diagnostics observes final boundaries rather than temporary ones | Read-only |
| **DIAG-2 — Safe diagnostics copy/export/service-presence model** | SHOULD before beta if Diagnostics is promoted to production | Medium | Copy/Export/log actions are local previews; unsafe generic dumps would risk secrets while fake health badges would mislead | diagnostics export/presentation | M | DIAG-1 | Read-only |
| **DESKTOP-1 — Packaged desktop metadata/discoverability** | SHOULD before beta | Medium | Package has GUI binary but no `.desktop`, icon/AppStream integration; also constrains clean startup integration | packaging/application integration | S/M | Can be implemented together with START-1 package-data portion | None |
| **AUTO-1 — Automation policy persistence only** | post-beta unless product wants saved inert rules in beta | Medium | Current Automation UI is local preview; config fields exist but no production executor/security lifecycle exists | `orbis-config`, Automation UI | M | CFG-1; must remain separate from any future executor | None |
| **AUTO-2 — Privileged automation executor/security model** | post-beta | High if enabled prematurely | Background rules could otherwise turn sessiond into a privileged deputy or bypass explicit authorization/reconciliation semantics | automation architecture, Hardware1 authorization | L | Separate threat/security design; not granted by AUTO-1 | Deferred privileged design |
| **ARCH-1 — Move direct telemetry/read architecture debt behind consistent read boundary where justified** | post-beta | Low/Medium | Current telemetry is real but GUI-direct; inconsistency complicates architecture/diagnostics but does not make values fake | telemetry/session architecture | M | After beta-critical Session1 work settles | Read-only |
| **GPU-1 — Product Eco/Standard/Ultimate/Optimized semantics** | post-beta while unavailable/disabled | Medium | UI visuals exist, but current production evidence only proves MUX/access/runtime-power primitives, not product mode authority | GPU product policy | L | Requires independent semantic proof; do not infer from primitives | Existing/possible future privileged path; defer |
| **DISPLAY-1 — Display refresh mutation backend** | post-beta while preview-only | Low/Medium | 60/120/Auto controls lack a production write backend; current Wayland observation is read-only | display backend/UI | L | Compositor/environment-specific mutation design | Deferred until backend proven |
| **POWER-1 — Power-limit support on FA707NV** | post-beta while unsupported/preview | Medium | Attributes exist but safe current/min/max/default metadata is not proven; exposing writes would be unsafe | power-limit provider/UI | L | Device-specific kernel evidence | Deferred privileged design |
| **LIGHT-1 — Panel/keyboard/Aura UI wiring beyond current beta core** | post-beta or opportunistic SHOULD | Low/Medium | Several providers/mutations are source-ready but not current production UI flows | lighting/display UI composition | M | After deployment truthfulness and beta core are stable | Existing bounded paths if/when enabled |
| **UPD-1 — Updates backend** | post-beta while preview-only | Low | Updates window is a shell; Check/Install do not perform package/network operations | updater/product integration | L | Separate update strategy for NixOS/package ownership | None until designed |
| **X11-1 — Explicitly prove X11 runtime closure** | post-beta if beta is Wayland-scoped; MUST before claiming general X11 support | Medium | Nix package explicitly proves Wayland dlopen closure better than X11 | packaging/runtime compatibility | M | Scope decision | None |
| **DEV-1 — stale `mockDevice`/`readOnlyEmpty` NixOS module options** | post-beta | Low | Development/dead options can confuse deployment expectations | NixOS module | S | After production module behavior stabilizes | None |

### Blocking-count convention

There are **7 implementation blockers** in the two MUST buckets:

- 4 before the first full packaged acceptance test: DEP-1 through DEP-4;
- 3 additional before beta: FAN-1, CFG-1, UI-1.

`OPS-1` is a separate acceptance-environment prerequisite, not a production implementation blocker.

START-1 and DIAG-1 are deliberately **not** counted as blockers while their controls/windows remain explicitly preview-only. If they are promoted to production UX before beta, their implementation becomes part of the beta gate.

---

# 3. MUST before first packaged acceptance test

## DEP-1 — UPower resilience and capability-local Battery failure

### Current defect

Latest hardening still builds the discovered production Session1 path by:

1. opening the system bus;
2. calling `discover_battery()`;
3. only after successful discovery building Session1.

UPower service failure, no matching battery, or unsupported battery topology therefore aborts the whole session daemon.

### Required end state

- Session1 is constructed even if UPower is absent or Battery is unsupported;
- Battery discovery is lazy/retryable or otherwise capability-local;
- later UPower availability/restart can recover Battery without requiring unrelated Session1 capabilities to disappear;
- no-battery/multiple-unresolved-battery remains `Unsupported` at Battery level rather than fake availability;
- transient UPower/service errors remain temporary/backend-unavailable semantics rather than permanent hardware unsupported;
- NixOS may set `services.upower.enable = lib.mkDefault true`, but sessiond must not use `Requires=upower.service` as a substitute for resilience.

### Minimum tests

- Session1 name/service starts with UPower unavailable;
- Performance/GPU primitive getters remain usable while Battery is unavailable;
- no matching battery affects only Battery;
- late/restarted UPower is retried/recovered;
- no hardware writes.

## DEP-2 — PolicyKit authority lifecycle

### Current defect

The full module installs the Orbis PolicyKit action XML but does not enable the native NixOS PolicyKit authority.

### Required end state

Under the full production module:

- `security.polkit.enable = true`;
- no dependency on KDE/GNOME-specific authentication agent;
- no custom PolicyKit service;
- no GUI/root privilege change;
- temporary authority failure remains a per-call fail-closed authorization error rather than a hardwared startup dependency.

### Minimum tests

- NixOS module evaluation proves PolicyKit enabled;
- VM proves `polkit.service`/`org.freedesktop.PolicyKit1` presence;
- no Hardware1 setter is required for this implementation test.

## DEP-3 — exact keyboard LED writable exception

### Current defect

Latest hardening still keeps:

- `ReadOnlyPaths = [ "/sys" ]`;
- `ReadWritePaths = [ "-/sys/firmware/acpi/platform_profile" ]`.

The existing keyboard backend writes `/sys/class/leds/asus::kbd_backlight/brightness`, so the packaged service namespace blocks the backend it exposes.

### Required end state

Keep all existing sandbox restrictions and add only:

`-/sys/class/leds/asus::kbd_backlight/brightness`

as another narrow writable exception.

Do not make `/sys`, `/sys/class/leds`, or the whole LED directory writable.

### Minimum tests

- generated service properties contain exactly the intended exceptions;
- namespace/symlink behavior is exercised without writing hardware;
- `max_brightness` remains read-only.

## DEP-4 — zero-write keyboard capability probe

### Current defect

Latest `SysfsKeyboardBacklightMutationBackend::mutation_status()` still returns `Supported` unconditionally.

### Required end state

Fresh read-only structural evidence must distinguish at least:

- absent LED ABI → Unsupported;
- permission failure → PermissionDenied;
- transient I/O → TemporarilyUnavailable;
- malformed/contradictory values → Unknown/internal diagnostic;
- valid readable `brightness` + `max_brightness`, with `brightness <= max` → structurally Supported.

`Supported` must not be documented as a guarantee that the next write will succeed.

### Minimum tests

Every status branch must assert **zero writes**.

## OPS-1 — packaged-test environment hygiene

Before running the real packaged acceptance test:

- stop/disable/remove any standalone development `orbis-hardwared.service` installed by `deploy-dev-hardwared.sh`;
- remove reliance on `/usr/local/bin/orbis-hardwared` for the test;
- ensure only the NixOS module owns the production Hardware1 service lifecycle and D-Bus name;
- use the full package/module path being evaluated for beta.

This is necessary to make test results attributable to the package under test.

---

# 4. MUST before beta

## FAN-1 — profile-specific fan read

### Current defect

The latest production composition still uses:

`SysfsFanCurveProvider(SysfsFanCurveSource)`

as the read side of `SessionHardwareFanCurveProvider`, while the UI calls:

`fan_curve_for_profile(AsusdFanProfile, FanId)`.

The sysfs provider is correctly active-curve-only and returns `Unsupported` for profile-specific reads.

### Required end state

Profile-specific stored curves:

`UI → worker → application → Session1 → sessiond → ZbusAsusdFanCurveSource → FanCurveData(profile)`.

Active-curve/capability evidence:

`Session1 → sessiond → SysfsFanCurveSource.active_curve(fan)`.

Keep unchanged:

- Hardware1 fan mutation;
- PolicyKit;
- typed asusd mutation;
- authoritative fresh post-write `FanCurveData(profile)` read-back;
- Factory Defaults refresh only after `Applied`.

### Beta-critical tests

Use distinct sentinel curves:

- active sysfs = A;
- asusd Balanced = B;
- asusd Quiet = C.

Prove capability probing sees A while profile reads return B/C and never substitute A. Also prove all four lossless asusd profiles, CPU/GPU selection, raw PWM preservation, error propagation, and no synthetic/default curve.

## CFG-1 — production preference/config store

### Why it is a beta blocker

Theme is already a real user-facing behavior. Persisting it through the existing early-stage monolithic `AppConfig` without the PR #10 separation would lock beta into a storage boundary that conflates appearance with automation policy, desired hardware state, and experimental gates.

### Required end state

At minimum:

- safe UI/application preferences have their own versioned schema, e.g. `preferences.toml`;
- automation policy is not in the same semantic blob;
- desired hardware state is not silently imported into Preferences;
- runtime window geometry belongs under XDG State, not portable Preferences;
- XDG path handling rejects/ignores invalid relative values and never silently falls back to `.` when no safe home exists;
- future schema versions are preserved and not overwritten;
- corrupt preferences recover to safe runtime defaults with warning and preserve the bad file for explicit recovery;
- writes use a unique same-directory temporary file and an explicit single-writer contract;
- legacy `config.toml` is imported conservatively and preserved.

### Minimum tests

Path resolution, schema version, future-version preservation, corruption recovery, atomic replacement behavior, legacy safe-field import, and proof that automation/battery/experimental fields are not imported into safe preferences.

## UI-1 — theme persistence

### Current behavior

Dark/Light switching is real and already synchronized by Rust across existing native windows. Newly opened windows inherit the current process theme. The process source of truth is still a session-local boolean; restart returns to Dark.

### Required end state

- load Preferences before constructing/showing the first visible top-level window;
- initialize the process theme from persisted value;
- retain the existing single process theme state + cross-window fan-out;
- persist a theme change only after validation/storage succeeds;
- newly created windows receive the current theme before `show()`;
- a persisted Light launch must not visibly render Dark first where the renderer permits this to be tested;
- storage failure must be visible without crashing or lying that persistence succeeded.

### Minimum tests

Default Dark, persisted Light first frame, all-open-window synchronization, later-created windows, persistence round-trip, corruption fallback, and write-failure reporting.

---

# 5. SHOULD before beta

## START-1 — Run on Startup

PR #5 classifies current Run on Startup controls as preview-only. PR #10 designs the correct production mechanism.

### Recommended implementation

Use actual user XDG Autostart state as source of truth:

`$XDG_CONFIG_HOME/autostart/<Orbis desktop id>.desktop`.

Do **not** add `run_on_startup = true/false` as a competing canonical preference.

The implementation should:

- probe no-entry / managed-entry / external-entry / Hidden override / conflict states;
- avoid deleting or overwriting unfamiliar user-authored entries;
- synchronize Main and Preferences from one runtime state;
- use user-level filesystem changes only;
- require no root, PolicyKit, Hardware1, or privileged Session1 operation;
- use packaged command/desktop-entry semantics that survive Nix store path changes.

### Beta policy if deferred

This is not a blocker if both visible startup controls remain explicitly preview-only/disabled and do not claim the app will actually launch at login.

If the beta UI presents the toggle as a real setting, START-1 becomes mandatory.

## DESKTOP-1 — desktop integration

The audited package has no application `.desktop`, icon/AppStream metadata, or autostart entry.

This is not a core safety blocker, but packaged beta quality benefits from:

- stable desktop ID;
- normal launcher entry;
- icon metadata;
- a stable command identity usable by START-1.

Keep this separate from NixOS system-service enabling: launching the GUI at user login is not the same as enabling `orbis-sessiond`/`orbis-hardwared`.

## DIAG-1 — DiagnosticsSnapshot

PR #8 found 47 diagnostics semantic items:

- 16 direct existing production sources;
- 26 needing aggregation/small read-only wiring;
- 5 with no honest source today.

Recommended architecture remains one immutable typed `DiagnosticsSnapshot` assembled at the application/runtime boundary. Do not make Slint independently poll providers and do not add a generic Session1 diagnostics umbrella API.

Keep these facts separate:

- service presence/health;
- capability support;
- operation read/write status;
- observed values;
- telemetry freshness;
- GPU MUX/access/runtime power/product-mode/staged backend state.

### Beta policy if deferred

Diagnostics does not block beta while the window is explicitly preview-only and static values are visibly identified as previews.

If the window is marketed as real Diagnostics, DIAG-1 becomes mandatory and all unsupported facts must remain Unknown/Unavailable rather than inferred.

## DIAG-2 — safe export/copy

If Diagnostics becomes real before beta:

- export/copy must be generated from a strict allowlisted typed snapshot;
- never dump serial, machine-id, usernames/home paths, arbitrary environment, MAC/IP data, secrets, arbitrary sysfs paths, journal text, or config contents;
- Copy Summary and Export Report must report failure rather than silently produce partial/unsafe output;
- service health should use D-Bus presence/activatability facts, not static provider `Healthy` values as a substitute for liveness.

---

# 6. Explicit non-blockers while preview-only

The following findings belong in the backlog but are **not beta blockers** if the product keeps them disabled/unavailable/preview-only and does not imply successful production behavior.

## Product GPU mode

Do not derive Eco/Standard/Ultimate/Optimized from:

- physical MUX;
- dGPU access policy;
- runtime dGPU power.

Those are independent facts. Current production product GPU mutation remains deliberately unavailable. This is safer than guessing and is acceptable for beta if the cards remain disabled/unavailable.

## Display refresh controls

Wayland output observation does not prove a 60/120 Hz mutation backend. Auto/60/120 UI may remain preview-only; do not turn the read provider into an inferred writer.

## Power limits

FA707NV exposes power-related attributes but current safe metadata/range/default semantics are not proven. Keep unsupported/experimental; do not block beta on missing writes.

## Automation execution

Persisting policy is not an executor. No beta requirement should force Orbis to invent background privileged authorization through sessiond.

If Automation UI remains preview-only, executor work is post-beta.

## Updates

The Updates window currently has no real updater backend. It may remain preview-only without blocking beta, provided Check/Install are not represented as real package operations.

## Backend-ready display/lighting controls

Panel Overdrive, keyboard backlight, Aura and some read-only display providers are useful future slices. They do not block beta simply because the visual controls exist. Only capabilities actually promoted to production must satisfy truthful probe/mutation/read-back semantics.

---

# 7. Optimal implementation order

The order below is designed to minimize same-file and same-composition conflicts, especially around `packaging/nix/module.nix`, `orbis-sessiond` composition/bootstrap, and `orbis-ui/src/main.rs`.

## 1. DEP-2 — PolicyKit enablement

**Files:** `packaging/nix/module.nix` + module/VM tests.  
**Reason first:** tiny, deterministic, independent semantic change. Establishes the production authorization lifecycle before later packaged testing.

## 2. DEP-3 — keyboard LED sandbox

**Files:** same `packaging/nix/module.nix`.  
**Conflict rule:** do this immediately after DEP-2 on the same deployment branch/stack rather than creating two long-lived parallel branches that both edit `module.nix`.

## 3. DEP-4 — keyboard capability honesty

**Files:** `crates/orbis-hardwared/src/keyboard_backlight.rs`, possibly Hardware1 status plumbing/tests.  
**Parallel safety:** can be developed in parallel with steps 1–2 because it does not need `module.nix`, but merge it after the deployment stack so the combined behavior is easy to verify.

## 4. DEP-1A — resilient/lazy Battery provider

**Files:** `orbis-sessiond` discovery/upower/composition/bootstrap.  
**Important:** first implement the application resilience without touching `module.nix` if practical. This isolates the larger Rust change from the small deployment edits above.

## 5. DEP-1B — NixOS UPower default integration

**Files:** `packaging/nix/module.nix`.  
**Conflict rule:** land only after steps 1–2 have established the final module context. Add `services.upower.enable = lib.mkDefault true`; do not add hard service lifecycle coupling.

At this point DEP-1 through DEP-4 are structurally complete.

## 6. Run first packaged acceptance test

Do not begin until the exact gate in section 8 is satisfied. Remove standalone dev hardwared first.

## 7. FAN-1 — profile-specific fan reads

**Files:** sessiond fan provider; Session1 protocol/service/client; UI `composition.rs`.  
**Reason after UPower:** both DEP-1 and FAN-1 touch sessiond composition/server construction. Serializing them avoids a high-conflict rebase and lets fan Session1 work build on the settled production session graph.

## 8. CFG-1 — production preference store

**Files:** `orbis-config` only initially.  
**Parallel safety:** this can safely start while steps 1–7 are underway because it does not need sessiond/hardwared/module files. Keep the first commit storage-only to preserve that isolation.

## 9. UI-1 — theme persistence

**Files:** `orbis-ui/src/main.rs`, small preferences lifecycle helper, Preferences Slint only as needed.  
**Reason after CFG-1:** UI should consume the final typed preference boundary rather than creating another temporary config path.

## 10. START-1 + DESKTOP-1

**Files:** unprivileged autostart helper, Main/Preferences UI state, package desktop data.  
**Reason after theme:** both startup and theme touch `main.rs` and Preferences. Serializing them avoids repeated conflict in UI lifecycle code.

If startup is deferred, keep it preview-only and skip this step without blocking beta.

## 11. DIAG-1

**Files:** core/application/runtime/Diagnostics UI.  
**Reason late:** Diagnostics should aggregate the final capability/session/preferences/deployment semantics rather than be repeatedly rewritten as those sources change.

## 12. DIAG-2 and other SHOULD work

Safe export/copy, desktop polish, and any promoted display/lighting controls can then land with isolated product decisions.

## Parallel-work lanes

To reduce conflict while multiple agents work:

- **Lane A — deployment:** DEP-2 → DEP-3 → DEP-1B, serial on `module.nix`.
- **Lane B — hardwared honesty:** DEP-4, mostly independent.
- **Lane C — session architecture:** DEP-1A → FAN-1, serial because composition overlaps.
- **Lane D — preferences:** CFG-1, independent until UI wiring.
- **Lane E — UI lifecycle:** UI-1 → START-1, serial on `main.rs`/Preferences.
- **Lane F — diagnostics:** start implementation after the core lanes settle.

Avoid parallel long-lived branches for two changes that both edit `packaging/nix/module.nix`, `orbis-sessiond/src/composition.rs`, or `orbis-ui/src/main.rs`.

---

# 8. FIRST PACKAGED TEST GATE

The branch/integration state is ready to begin the **first real full packaged NixOS acceptance test** only when every condition below is true.

## Code/deployment prerequisites

1. **DEP-1 complete:** Session1 starts without requiring successful UPower Battery discovery; Battery failure is capability-local and retryable.
2. Normal NixOS integration enables UPower by default (`lib.mkDefault`) without a hard `Requires=upower.service` dependency for sessiond.
3. **DEP-2 complete:** full Orbis module enables the native PolicyKit authority.
4. **DEP-3 complete:** hardwared sandbox reopens only the exact keyboard `brightness` path in addition to existing `platform_profile`; `/sys` remains otherwise read-only.
5. **DEP-4 complete:** keyboard mutation status is based on a zero-write structural probe rather than unconditional `Supported`.
6. All implementation tests for those four slices pass, including zero-write probe assertions and NixOS module/VM checks.
7. Package evaluation/build and existing workspace checks pass on the integrated commit.

## Environment prerequisites

8. Standalone development hardwared deployment is disabled/removed for the acceptance machine.
9. There is exactly one intended owner/lifecycle for `io.github.orbiscontrol.Hardware`.
10. The test uses the full NixOS module/package that will form the beta path, not a mixed dev/policies-only installation.
11. The acceptance checklist records the exact integration SHA, NixOS configuration, kernel, asusd version/service state, and target hardware identity at the privacy-safe model/board level.

## Safety prerequisite

12. No automated test reaches a real Hardware1 setter merely to prove readiness for the manual packaged test. The first deliberate real mutation happens only as an explicit acceptance-test step with before/after observation.

When conditions 1–12 are satisfied, Orbis is ready to start the first real packaged test. FAN-1/theme/Diagnostics/startup are **not** prerequisites to begin that deployment test because the test itself is needed to validate the corrected package lifecycle first.

---

# 9. Packaged acceptance test goals

The first packaged test should answer deployment questions before product-expansion questions.

At minimum it should prove:

1. fresh NixOS module evaluation/build/install succeeds;
2. hardwared starts and owns Hardware1;
3. sessiond starts and owns Session1;
4. PolicyKit authority is present;
5. UPower absence/failure does not kill unrelated Session1 reads;
6. on normal FA707NV runtime, Battery/Performance/GPU primitive reads are classified honestly;
7. keyboard mutation status reflects real read-only structural evidence;
8. hardwared effective sandbox contains only intended writable sysfs exceptions;
9. no duplicate dev service owns/competes for Hardware1;
10. logout/session lifecycle stops sessiond appropriately while hardwared remains system-scoped;
11. restart/failure paths do not create unsafe startup writes;
12. any deliberate mutation test is explicit, capability-gated, PolicyKit-authorized, and validated by the existing authoritative read-back semantics.

The acceptance report should distinguish:

- package/service failure;
- unsupported hardware capability;
- optional backend unavailable;
- permission/authorization failure;
- mutation attempted but not authoritatively confirmed.

Do not convert any of those into one generic “beta failed” state without preserving the evidence.

---

# 10. BETA GATE

Orbis may be called **beta-ready** only when all mandatory conditions below are satisfied on one integrated candidate SHA.

## A. First packaged-test gate passed

1. Every condition in section 8 is complete.
2. At least one full packaged NixOS acceptance run has been completed on the target FA707NV-class hardware using the full module/package and without a competing dev deployment.
3. No Critical/High deployment defect discovered by that run remains open for a capability the beta claims to support.

## B. Core production read/write truthfulness

4. Performance and Battery production controls use authoritative reads and preserve existing Hardware1/PolicyKit/read-back semantics.
5. **FAN-1 is fixed:** selected profile/fan reads return the correct asusd `FanCurveData(profile)` curve through the read-only Session1 path; active sysfs curve remains only the active/capability source.
6. Fan read failures never create optimistic/default curves.
7. Factory Defaults refreshes the selected profile only after confirmed `Applied` and receives the corrected profile-specific read.
8. Product GPU mode remains unavailable/disabled unless a separate authoritative semantic design has landed; MUX/access/runtime power are never mislabeled as Eco/Standard/Ultimate/Optimized.
9. Any other control not production-wired remains explicitly preview-only/disabled/unavailable rather than pretending success.

## C. Preferences integrity

10. **CFG-1 is complete:** production safe preferences have a separate versioned schema with corruption/future-version preservation and safe XDG path handling.
11. Loading preferences performs zero hardware writes and does not execute automation policy or desired hardware state.
12. **UI-1 is complete:** Dark/Light persists across restart and is restored before first visible window render where testable.
13. Multi-window theme synchronization still works for already-open and newly-created windows.
14. A preference write failure is reported and never falsely displayed as persisted.

## D. Startup and Diagnostics honesty

15. For **Run on Startup**, one of two states must be true:
    - START-1 is implemented using real user XDG Autostart state; **or**
    - all startup controls remain explicitly preview-only/disabled and the beta documentation does not claim login autostart support.
16. For **Diagnostics**, one of two states must be true:
    - DIAG-1 is implemented using typed authoritative aggregation; **or**
    - Diagnostics remains explicitly preview-only and does not display fake Connected/Healthy/Supported claims as production facts.
17. If Copy Summary/Export Report is production-enabled, DIAG-2 safe allowlisting is complete; otherwise those actions remain preview-only/disabled.

## E. Automation boundary

18. Automation persistence, if present, stores policy only.
19. No automation rule executes as a side effect of load, migration, opening the window, Save Rules, or application startup.
20. Session1 has not been expanded into a generic privileged automation deputy.
21. If no separate reviewed background-authorization/reconciliation design exists, automation execution remains disabled/preview-only.

## F. Package/quality evidence

22. Candidate package builds reproducibly from the recorded lock/integration SHA.
23. Workspace/unit/integration/NixOS checks used by the production branch pass on that candidate.
24. The full installed service/policy paths match the package under test; no stale `/usr/local` development artifacts are required.
25. Every beta-advertised writable capability has at least one documented real-hardware acceptance result on supported hardware, with PolicyKit authorization and authoritative post-write observation appropriate to that backend.
26. Capabilities not proven writable are read-only, unsupported, temporarily unavailable, or preview-only in the product contract.
27. No unresolved Critical or High item in the MUST buckets remains.

Passing conditions 1–27 is the beta gate. SHOULD items may remain open only when their corresponding UX remains honest about being preview/unavailable.

---

# 11. Recommended beta scope

To reach a trustworthy beta sooner, scope the first beta around functionality whose semantics are already strong:

- Performance profile;
- Battery charge threshold;
- Fan editor after FAN-1;
- read-only GPU primitives as separate facts;
- telemetry as real read-only data;
- Dark/Light with persistence;
- strong capability/error presentation;
- NixOS packaged lifecycle with PolicyKit/UPower/sandbox correctness.

Do not delay beta merely to implement every visual prototype.

Explicitly acceptable beta deferrals include:

- product GPU mode cards remaining unavailable;
- display refresh writes remaining preview-only;
- power limits remaining unavailable/experimental;
- Automation execution remaining preview-only;
- Updates remaining preview-only;
- Diagnostics remaining preview-only if clearly labeled;
- Run on Startup remaining preview-only if clearly labeled;
- non-core lighting/display controls remaining unpromoted.

This is preferable to expanding the beta surface with semantically guessed or weakly authorized behavior.

---

# 12. Summary

The shortest credible path to beta is not “wire every screen.” It is:

1. make the package self-contained enough to test;
2. remove false capability claims;
3. run the first real packaged acceptance test;
4. repair the fan editor's required profile read;
5. put preferences on a production-safe storage boundary and persist the already-real theme;
6. keep unimplemented UI explicitly preview-only;
7. promote startup/Diagnostics only when their real mechanisms are ready;
8. require one integrated candidate to pass the explicit beta gate above.

The resulting beta can be intentionally narrow while still being truthful, testable, and safe.