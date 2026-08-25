# Beta Acceptance Checklist

> Current checklist for the active integration branch.
> Status date: 2026-08-20.
>
> This file intentionally contains **current release gates**, not old PR-stack status. Historical beta-readiness plans remain indexed by [`history.md`](history.md).

## Evidence rule

Use [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md).

- source presence/review → at most `IMPLEMENTED`;
- executable tests/checks → `TESTED` only for the exact revision/environment;
- installed artifact acceptance → `PACKAGED`;
- hardware behavior observed on a named environment → `LIVE-VALIDATED` for that revision/environment only;
- absent evidence remains `UNKNOWN` or `BLOCKED`.

## Repository / CI

- [x] One canonical general-purpose workflow remains: `.github/workflows/ci.yml`.
- [x] Obsolete one-off validation trigger artifacts are removed from `.github`.
- [x] Canonical documentation hierarchy exists and historical plans are separated.
- [x] Draft PR #129 existed as the explicit integration checkpoint; closed 2026-08-25 as superseded — `development` is now the single integration line.
- [ ] #106: GitHub Actions actually executes repository steps and produces trustworthy green results. Earlier runs failed pre-step (`steps=null`); root cause identified as Actions billing (2026-08-24).
- [ ] Exact candidate passes `cargo fmt --all -- --check`.
- [ ] Exact candidate passes `cargo check --workspace --all-targets --locked`.
- [ ] Exact candidate passes `cargo test --workspace --locked`.
- [ ] Exact candidate passes `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [ ] Exact candidate passes `nix flake check`.
- [ ] #114: after CI recovery, `main` is protected by the real required checks.

## Security / privilege boundary

- [x] Privileged mutation is behind typed `Hardware1`, not a generic root proxy.
- [x] `sessiond` remains a read/session boundary.
- [x] Raw GPU/Fan/Panel/Keyboard/Aura writes are product/policy blocked in the current production composition.
- [x] Fan writes are defense-in-depth blocked while known safety defects remain.
- [x] #125: interactive GUI rejects euid 0 before preferences/runtime/D-Bus setup; screenshot/offscreen paths remain exempt.
- [x] #123: provider/status hang handling is complete, including mutation unknown-outcome recovery without blind retry; local workspace and full Nix checks are green.
- [x] #126: packaged/VM/live inspection proves the intended minimal hardwared sandbox on the exact candidate; live module-unit evidence is recorded for FA707NV.

## Capability truth

- [x] Read and write operation evidence are separate.
- [x] Explicit and periodic capability refresh use one canonical mutation-status requery path (#112 source-complete).
- [x] Public probes use bounded read-only adapters.
- [x] #120: UI/Diagnostics consume operation-level/equivalent typed write evidence; support-matrix schema separates read/write; product-disabled unvalidated writers remain conservative `Unsupported` with reasons.
- [x] #107: Battery write owner/interface liveness is dynamically re-proven across restart/disappearance; live owner-present evidence is recorded.
- [x] #117: telemetry distinguishes useful fresh data from empty/partial/field-local failure states.

## Battery / Performance / GPU reads

- [x] Source contract preserves Battery configured/effective/enabled distinctions.
- [x] Performance read/write uses typed profile values and read-back.
- [x] GPU power / MUX / access remain independent concepts.
- [ ] Re-run executable integration tests on the exact beta candidate.
- [ ] Re-run packaged acceptance on the exact beta candidate.
- [ ] Any live hardware claims are recorded with model/environment/revision and read/write distinction.

## Fans

- [x] Profile/fan-specific Session1 reads request a concrete fan; one requested fan no longer requires the other fan in the same response.
- [x] Fan mutation remains disabled.
- [ ] #109: aggregate capability no longer infers GPU support from CPU-only evidence.
- [ ] #116: stored `FanCurveData.enabled` reaches typed Session1/client/UI evidence.
- [ ] #104: dormant custom-write path preserves authoritative enabled state on write/read-back.
- [ ] #105: Factory Defaults guarantees restoration of the previous platform profile on every failure path or avoids temporary switching.
- [ ] Controlled hardware validation proves final fan/profile state before any write promotion.

## Preferences / desktop lifecycle

- [x] Theme persistence source wiring exists.
- [x] XDG Autostart read/write/read-back exists (#110 closed as source-complete).
- [x] Start Minimized persistence/startup lifecycle exists.
- [x] Window position is supported only where absolute placement is valid; Wayland fails closed.
- [x] Tray close behavior requires a live host; Quit explicitly terminates the event loop (#121 closed).
- [x] Diagnostics Refresh/Export/Copy lifecycle exists (#111 closed); Open Logs remains intentionally unavailable.
- [ ] Packaged desktop/tray/preferences restart acceptance on the exact candidate.

## CLI / supportability

- [x] `orbisctl status` is read-only.
- [x] `orbisctl status --json` uses a versioned schema and explicit observation states.
- [x] CLI uses provider deadlines and has no Hardware1 mutation commands.
- [ ] #119: executable service-present/service-absent integration coverage on the exact candidate.
- [ ] Diagnostics export is checked in the packaged environment for permissions/path/privacy behavior.

## Production dependency graph

- [x] Providers/sessiond defaults no longer implicitly enable mock features.
- [x] #115: GUI production startup no longer constructs fixture-derived initial state.
- [x] #115: `orbis-test-support` is removed from the normal GUI release dependency graph.
- [x] UI uses compile-time package version rather than fixture hardcoding.

## Automation / future writes

- [x] Automation shadow/policy/lifecycle/recovery/serialization contracts exist in source.
- [x] Performance executor requires authoritative read-back.
- [x] `AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED` remains false without executable validation.
- [x] GPU/Fan/Battery/Display/Lighting unattended executors remain disabled.
- [ ] Any future promotion has an exact-build executable test and explicit product-policy approval.

## Display / Updates / extended controls

- [x] Display Refresh has typed target/request/read-back semantics.
- [x] No shell fallback is used as a substitute for a compositor mutation owner.
- [ ] A concrete typed compositor owner exists and is validated before Display modeset is enabled.
- [x] Updates does not invent a release feed/downloader/installer.
- [ ] A canonical signed source and installation-owner contract exists before application self-update is enabled.
- [x] #124 application identity is explicitly accepted before stable release; ADR 0013 fixes `io.github.orbiscontrol.Orbis` as permanent and hosting-independent.

## Final beta decision

A beta is acceptable only when:

1. The validated integration line (`development`; formerly Draft PR #129) is merged into `main`;
2. #106 is resolved and the exact candidate has executable green Rust/Nix checks;
3. #125 and the remaining safety-critical #123 contract are complete;
4. no known unsafe or policy-unproven write is enabled;
5. package/desktop/D-Bus/polkit/tray acceptance succeeds on the exact artifact;
6. support claims match recorded evidence;
7. unresolved items are presented as unavailable/blocked rather than hidden behind optimistic UI.

Until then, the beta gate is **BLOCKED** even if source-level static contracts pass.
