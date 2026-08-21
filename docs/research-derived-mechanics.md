# Research-derived mechanics backlog

> Role: implementation backlog derived from comparative research of G-Helper,
> rog-helper / g-helper-linux, LACT, asusctl / ROG Control Center, CoolerControl,
> NBFC-Linux, TUXEDO Control Center, Lenovo Legion Toolkit, HHD, UXTU and
> OpenRGB.
>
> This file does not override `docs/current-state.md` or `docs/roadmap.md`.
> Safety blockers and release gates remain authoritative there.
>
> Updated: 2026-08-20.

## Design rule

Orbis should not copy any one competing architecture wholesale. Preserve the
existing Orbis invariants:

- unprivileged GUI;
- session/read boundary separated from privileged mutation;
- narrow typed Hardware1 API, never a generic filesystem/sysfs/shell proxy;
- capability support established by runtime evidence, not DMI/model names;
- read and write evidence remain independent;
- Desired, Observed and Pending are distinct;
- `Accepted != Applied`;
- unsupported or uncertain operations stay fail-closed;
- mutation retries are never automatic when timeout leaves the outcome unknown.

The target is to combine the strongest mechanics from mature projects while
keeping Orbis's stricter security and evidence model.

## Priority legend

- **P0** — runtime correctness / safety foundation. Implement before new control
  surfaces.
- **P1** — high-value production functionality after P0 is proven.
- **P2** — advanced UX / automation / telemetry.
- **P3** — ecosystem and specialist functionality.
- **R&D** — experimental work requiring separate evidence and product decision.

## P0 — runtime and mutation safety

### P0.1 Canonical bounded provider execution

**Sources:** rog-helper readiness probes; existing Orbis `Provider::timeout()`
contract.

Implement one reusable execution primitive at the provider/application boundary:

- every provider read/status/probe is bounded by that provider's declared
  timeout;
- elapsed operations map to `ProviderError::Timeout`;
- timeout stays local to the capability/operation that timed out;
- a hung read cannot stall unrelated sequential worker work indefinitely;
- capability probes classify timeout as `TemporarilyUnavailable`, never
  `Unsupported`;
- no automatic mutation retry after timeout;
- mutation timeout must preserve "outcome unknown" semantics and trigger
  authoritative follow-up observation where supported.

**Acceptance:** deterministic never-completing-provider tests with Tokio paused
clock; later unrelated work proceeds after timeout.

**Related:** #123.

### P0.2 Bounded parallel readiness/setup report

**Source:** rog-helper `setup.rs`.

Add structured readiness information rather than a single available/unavailable
flag:

- dependency kind;
- state (`Ready`, `Inactive`, `NotAvailable`, `Unreachable`, `NotRelevant` or
  equivalent Orbis capability states);
- `required_for` feature list;
- evidence;
- permission state separated from dependency state;
- probes execute independently and in parallel where safe;
- one slow backend cannot block publication of unrelated evidence.

Prefer integrating this with the existing capability registry and diagnostics
DTOs rather than adding a second source of truth.

### P0.3 Canonical capability refresh path

Periodic and explicit refresh must execute the same sequence:

1. re-query mutation owner/status evidence;
2. run bounded capability probes;
3. build a complete next-generation snapshot;
4. publish whole-swap only when the snapshot contract is valid.

No partially built registry may become authoritative.

**Related:** #112.

### P0.4 Transactional mutation coordinator

**Source:** LACT pending configuration confirmation/rollback.

For risky mutations, model the operation as a transaction:

1. read authoritative previous Observed state;
2. validate request and policy;
3. record Requested/Pending state;
4. execute exactly one mutation attempt;
5. perform authoritative read-back;
6. if the change is proven, allow commit/confirmation;
7. if confirmation expires or explicit revert is requested, restore previous
   state when restoration semantics are proven;
8. if mutation outcome is unknown, do not blindly retry.

The coordinator should expose typed terminal states such as `Applied`,
`RolledBack`, `Failed`, `UnknownOutcome` and `PendingRequirement` rather than
booleans.

Initial targets after current safety blockers: GPU/power/display class changes.
Fan writes remain fail-closed until the existing fan blockers are resolved.

### P0.5 Explicit rollback guard for temporary profile switches

**Source:** asusctl fan-default/profile semantics.

Any operation that temporarily changes platform profile to read/apply profile-
specific defaults must restore the previous profile on every failure path.
Use a scope/transaction guard or explicit finally-style restoration. Never rely
on a sequence of fallible `?` calls where an intermediate error skips restore.

**Related:** #105.

### P0.6 Conflict/owner detection

**Sources:** G-Helper, LACT, HHD operational behavior.

Detect competing owners/services for controls Orbis intends to manage:

- performance profile owner;
- GPU policy/MUX owner;
- fan-control owner;
- power-profile daemon conflicts where semantics are known.

Conflicts should become explicit `Conflicted`/policy evidence. Do not silently
stop, disable or reconfigure another service.

## P1 — production control model

### P1.1 Global preset / policy model

**Sources:** G-Helper, TUXEDO Control Center, UXTU.

A preset is a desired policy bundle, not a bag of direct writes. Candidate
fields:

- performance profile;
- fan profile/curve reference;
- GPU product policy;
- display refresh policy;
- battery policy;
- lighting policy.

Applying a preset changes Desired state. The normal reconciliation engine owns
capability/policy checks and execution.

### P1.2 Reconciliation engine

Implement the existing Desired/Observed/Pending foundations as:

`read authoritative Observed -> compare -> capability/policy gate -> decide ->
one mutation -> read-back -> publish Applied/Pending/Failed`.

Rules:

- loading persisted/default config never itself writes hardware;
- missing/corrupt config never synthesizes hardware actions;
- no write is issued when Observed already matches Desired;
- lifecycle re-entry rechecks Observed before proposing a mutation.

### P1.3 Suspend/resume reconciliation

**Sources:** LACT, CoolerControl.

After resume, re-read only the relevant authoritative state because firmware or
drivers can reset controls. Reconcile from Desired state rather than blindly
replaying all previous writes.

### P1.4 Pending reboot/logout state

**Sources:** supergfx/asusctl-style GPU transitions.

Represent action requirements explicitly in state and UI:

- requested policy;
- currently observed hardware state;
- requirement (`Reboot`, `Logout`, etc.);
- pending transition reason;
- cancel/revert capability when semantics are proven.

Do not present the requested product mode as already applied.

### P1.5 Quick-control surface

**Source:** G-Helper.

Keep the main window focused on frequent actions and current state:

- Performance;
- GPU policy/state;
- Display;
- Battery;
- Fans;
- Lighting.

Advanced editors remain separate. Quick controls must still expose Pending,
blocked and unsupported states honestly.

### P1.6 Advanced control pages

**Sources:** ROG Control Center and LACT.

Use dedicated technical pages for detailed editors and diagnostics. Edits should
be staged:

`edit -> dirty -> Apply -> Pending -> authoritative read-back`.

Changing a toggle or dragging a graph must not implicitly mean a hardware write
unless the control is explicitly designed as immediate and safe.

### P1.7 Telemetry history

**Source:** LACT.

Add bounded in-memory history with configurable retention for useful metrics:

- temperatures;
- clocks;
- power;
- fan RPM;
- battery/AC values where useful.

Track freshness/completeness separately from successful transport. Empty or
partial successful calls must not be presented as full fresh telemetry.

**Related:** #117.

### P1.8 Telemetry export

**Source:** LACT.

CSV export should use the same allowlisted/redacted data policy as diagnostics.
Export metadata should include timestamp, metric identity and freshness where
relevant.

### P1.9 System telemetry provider extensions

**Source:** rog-helper provider decomposition.

Candidate read-only data:

- RAM/swap;
- PSI;
- zram/zswap;
- NVIDIA telemetry fallback;
- additional hwmon sensors.

Each source remains independently optional and evidence-gated.

### P1.10 Hardware support validation mode

**Source:** NBFC-Linux read-only onboarding workflow.

Provide an explicit validation workflow for unknown/new hardware:

1. read-only discovery;
2. verify sensor identity and independent behavior;
3. record capability evidence;
4. only then offer controlled mutation validation when the project safety gate
   allows it.

DMI/model database entries may narrow discovery but are never authoritative
support proof.

## P2 — automation, fan quality and UX

### P2.1 Trigger -> condition -> Desired State automation

**Sources:** LACT, Lenovo Legion Toolkit, G-Helper.

Candidate triggers:

- AC/battery transition;
- process/game running;
- GameMode state;
- session lifecycle;
- user hotkey;
- time/profile event if later desired.

Automation must update Desired policy, not bypass reconciliation with arbitrary
hardware actions.

### P2.2 Per-application profiles

Map processes/games to presets. Rules need deterministic precedence and a clear
restore policy when the triggering process exits.

### P2.3 AC/battery policy

**Source:** G-Helper.

Examples:

- performance profile by power source;
- display refresh by power source;
- optional GPU/battery policy when semantics are proven.

### P2.4 Fan hysteresis and response shaping

**Source:** CoolerControl.

For any future software-controlled fan mode, support:

- hysteresis;
- change threshold;
- bounded speed-up/down rate;
- optional smoothing.

Do not apply these algorithms to firmware-managed ASUS fan profiles silently.
Firmware mode and software control mode must be explicit separate concepts.

### P2.5 Smoothed/virtual sensors

**Source:** CoolerControl.

Candidate functions:

- EMA/time average;
- maximum/minimum;
- delta;
- weighted average;
- offset.

Virtual sensors are derived telemetry and never evidence that an underlying
hardware control exists.

### P2.6 Control-flow visualization

**Source:** CoolerControl.

Advanced diagnostics may visualize:

`Trigger -> Preset/Desired State -> Capability Gate -> Backend -> Observed`.

This is especially useful once automation and reconciliation are active.

### P2.7 Alerts

**Source:** CoolerControl.

Candidates:

- thermal threshold;
- fan RPM unexpectedly zero;
- telemetry stale;
- Desired != Observed for too long;
- repeated reconciliation failure.

Alerts report; they do not silently perform dangerous corrective mutations.

### P2.8 Last-applied/audit surface

**Sources:** UXTU and Orbis's existing outcome model.

Show at least:

- requested state;
- authoritative observed state;
- last successful change time;
- pending requirement;
- last failure/rollback classification where useful.

### P2.9 Preset import/export

**Source:** UXTU.

Use versioned, validated schemas. Imported presets are untrusted configuration
and must not trigger writes merely by being loaded.

### P2.10 Hotkeys and tray quick actions

**Sources:** G-Helper, UXTU.

Hotkeys/tray actions should select presets or Desired state through the same
application boundary as the main UI.

## P3 — ecosystem / alternate interaction models

### P3.1 Compact overlay mode

**Sources:** HHD, Lenovo Legion Toolkit.

Useful for gaming/handheld scenarios: compact status + safe preset switching.
Keep it a client of the same typed state model, never a separate control path.

### P3.2 Lighting capability model

**Sources:** asusctl and OpenRGB concepts.

Long-term model should separate:

- device;
- zone;
- supported effects;
- current effect/state;
- mutation evidence.

Prefer asusctl/Linux ASUS semantics for device-specific implementation. Avoid
copying GPL-2.0-only OpenRGB implementation code into the GPL-3.0-or-later Orbis
codebase; external integration is cleaner if broad OpenRGB ecosystem access is
needed.

### P3.3 Plugin architecture

**Sources:** Lenovo Legion Toolkit, OpenRGB, HHD.

Do not add plugins before core state/protocol contracts stabilize. Future plugins
must not be able to widen Hardware1 into an arbitrary privileged execution
surface.

## R&D

### R&D.1 Adaptive power control

**Source:** UXTU.

Dynamic TDP/power control requires a separate safety design, bounds, rate limits,
observability and rollback. It is not an extension of static profile switching.

### R&D.2 Predictive fan ramping

**Source:** Lenovo Legion Toolkit plugin concepts.

Potential algorithm uses smoothed `dT/dt` to ramp before thermal saturation.
Only relevant to an explicit future software fan-control mode and requires
hardware validation.

## External-code reuse policy

Research sources are useful at different levels:

- **rog-helper / g-helper-linux (Apache-2.0):** good candidate for selective
  adaptation of readiness/probe patterns and Linux provider ideas.
- **LACT (MIT):** good candidate for selective adaptation of transactional
  confirmation/rollback and telemetry/profile patterns.
- **asusctl (MPL-2.0):** primary ASUS Linux semantics reference; prefer clean
  Orbis implementations unless direct file-level reuse is justified and MPL
  obligations are tracked.
- **G-Helper:** use primarily for product/domain knowledge and UX; Windows ACPI
  implementation is not a Linux backend.
- **CoolerControl / TUXEDO / NBFC and other GPL projects:** use for design and
  behavior research unless a license-reviewed direct reuse decision is made.
- **OpenRGB GPL-2.0-only code:** do not copy into Orbis GPL-3.0-or-later.

Any direct code import must record source revision, license and modifications.

## Implementation order

Recommended order, subordinate to the canonical roadmap:

1. P0.1 bounded provider execution (#123);
2. P0.3 canonical refresh path (#112);
3. P0.2 readiness/evidence presentation;
4. complete current capability truth blockers (#117, #107, #120 and related
   fan blockers);
5. P0.4 transactional mutation coordinator foundation without enabling new
   writes;
6. P1.2 reconciliation engine;
7. suspend/resume and pending-requirement handling;
8. presets + automation through Desired State;
9. telemetry history/export and system telemetry extensions;
10. advanced fan algorithms only after fan mutation safety is re-proven;
11. lighting/overlay/plugins after core protocol stability.

## Current implementation status

- [x] Desired / Observed / Pending foundations exist.
- [x] Typed narrow Hardware1 privileged boundary exists.
- [x] Capability registry and read/write evidence separation exist.
- [x] Provider timeout is declared by the provider contract.
- [ ] Provider timeout is generically enforced in the production execution path.
- [ ] Explicit and periodic capability refresh share one canonical status-requery path.
- [ ] Structured setup/readiness report is surfaced.
- [ ] Transactional confirm/rollback coordinator exists.
- [ ] Reconciliation executes desired policy.
- [ ] Resume reconciliation exists.
- [ ] Global presets and automation execute through Desired State.
- [ ] Historical telemetry/export exists.
- [ ] Advanced fan smoothing/virtual sensors exist.
- [ ] Overlay/plugin architecture exists.
