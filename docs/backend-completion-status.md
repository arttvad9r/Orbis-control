# Backend Completion Status

> Status date: 2026-08-20.
>
> This file is the concise current-state companion to `ui-backend-contract.md`.
> The older contract remains useful for invariant details; where its historical
> "not connected" wording conflicts with this file, this status file describes
> the current branch implementation.

## Production-connected

- Performance: sequential worker → application owner → provider mutation →
  authoritative read-back.
- Battery charge limit: sequential worker → Hardware1/application path → fresh
  read-back; UI commits the slider only at the request boundary.
- Theme persistence.
- XDG user autostart read/write/read-back.
- Start Minimized persistence and next-launch lifecycle.
- Diagnostics Refresh and privacy-bounded JSON Export.
- Diagnostics Copy Summary through Slint's clipboard-capable `TextEdit` path.
- Fan selection/profile/curve reads. Fan writes remain safety-blocked.
- Display Quick Control authoritative read-only compositor observation.
- Keyboard Quick Control authoritative state plus Hardware1 mutation-status
  gating. The current production daemon reports the product write Unsupported,
  so the control remains disabled without a UI code change.
- Extra Keyboard brightness and Panel Overdrive have independent request-only
  Hardware1 status/set/read-back wiring. The current daemon reports both product
  writes Unsupported.
- Extra Aura mode/speed observation remains read-only because the existing Aura
  writer is Static RGB, not the same UI contract.
- Extra Boot sound now has a strict read-only `asus-armoury` firmware-attribute
  provider; mutation is not exposed.
- Remember Window Position is connected on X11-style positioning backends and
  fail-closed on Wayland.
- StatusNotifierItem tray backend is connected. HideToTray is available only
  while a real host is registered; Quit remains independently writable and
  terminates the Slint event loop.
- Typed confirmation-dialog context exists. Legacy numeric dialog kinds never
  authorize a side effect; only an explicit closed typed action may do so.

## Automation

Production `orbis_ui::worker` is `worker_runtime.rs`. It owns:

- hardened persisted policy synchronization and policy revision;
- raw worker telemetry observations;
- logind `PrepareForSleep(bool)` lifecycle observations;
- debounce/freshness/resume coalescing;
- lifecycle revision and supersession;
- capability-generation preflight/revalidation;
- replay-resistant single-slot serialization;
- Performance unknown-outcome recovery;
- compiled Performance-only executor semantics with mandatory authoritative
  read-back.

Unattended mutation is still deliberately unreachable because
`AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED` is `false`. This exact branch has not
passed executable Cargo/Clippy/Slint validation in the available environment.
GPU/Fan/Battery/Display/Lighting automation executors are not enabled.

## Release/product-gated typed writers

Typed Hardware1 implementations already exist for Panel Overdrive, Keyboard
Backlight and Aura Static RGB, but production hardwared deliberately composes
Unsupported backends for them. The UI reads the effective mutation-status rather
than inferring support from implementation existence.

`product_mutation_promotion.rs` models promotion evidence independently:

- closed typed backend;
- capability-specific authorization;
- non-mutating readiness preflight;
- authoritative hardware read-back, or config read-back only for an explicitly
  interactive Accepted operation;
- executable validation of the exact build;
- explicit product-policy approval.

Aura has no authoritative hardware RGB read-back, so config confirmation cannot
qualify it for unattended execution.

## Completed fail-closed boundaries

### Application updates

The Updates backend detects the local installation owner but does not invent an
update source. There is no repository-defined canonical signed Orbis release
feed and no proven universal installer owner. Typed blockers distinguish Nix,
AppImage, system-prefix, development and unknown installations. Network check,
channel mutation and install remain disabled; no shell/package-manager/self-
replacement path exists.

### Display Refresh

The typed domain/provider/application contract is implemented: stable target
identity, exact refresh presets, fresh preflight, owner-side revalidation,
mutation result and mandatory exact applied-state read-back. The missing piece is
one concrete compositor configuration owner. Current dependencies contain
`wayland-client` but not a WLR output-management protocol binding dependency, so
no shell fallback or unverifiable lockfile change is introduced on this branch.

### Remaining ambiguous Extra fields

Status LEDs, auto-clamshell, ASPM, Modern Standby networking, iGPU memory, CPU
core controls and hotkey mappings stay disabled until an exact upstream/domain
owner is proven. Similar-looking ASUS controls are not treated as equivalent by
name alone.

## Safety gates that remain intentional

- Fan mutation stays blocked until the existing enabled-state/factory-restore
  safety issues are resolved.
- GPU product-mode mutation stays policy-blocked.
- Panel/Keyboard/Aura production writes stay product/release-gated.
- Automation execution promotion stays false until executable validation.
- Updates has no network/install surface without an explicit signed source and
  installer owner.
- Display has no modeset write without a typed compositor owner.

## Validation state

Repository stdlib static contracts cover UI request-only semantics, Automation
shadow/executor invariants, Display Refresh contracts and cross-surface backend
completion invariants. They do not replace compilation.

The current execution environment does not provide Rust/Cargo/Slint binaries or
a vendored Cargo registry, and outbound package/toolchain download is blocked.
Therefore `cargo check --locked`, tests, clippy and the Slint compiler are not
claimed as passing for this branch.
