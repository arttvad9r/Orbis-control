# Orbis Control — canonical finish plan

This file is the **single execution plan for finishing Orbis Control into a usable Arch Linux daily-control application**.

When the user asks to `continue`, `finish`, `make it work`, or otherwise continue project development, agents must work from this file in order. Do not create a parallel roadmap or stop after producing a status report.

Production source and executable behavior override stale issues or historical documents. If an item below is already complete in current `main`, verify it, mark it complete, and immediately continue to the next item.

## Finish target

The first release is done when, on the target Arch Linux ASUS laptop:

- Orbis installs cleanly;
- `orbis-hardwared` and `orbis-sessiond` run from the installed assets;
- the unprivileged GUI starts normally and shows real machine state;
- the core daily controls that are supported by the machine work end to end and report authoritative state/errors;
- unfinished experimental surfaces are removed or hidden rather than presented as product features;
- a real Arch package can install the same product layout;
- `scripts/verify full` passes on the release candidate;
- an installed smoke test passes.

Do **not** delay this target to finish speculative architecture, broad automation infrastructure, a self-updater, or unsupported hardware features.

---

## Phase 0 — establish the real current baseline

Goal: make the current `main` buildable and remove stale backlog assumptions before new feature work.

- [ ] Run `scripts/verify task` on current `main` and fix all actual build/test/clippy failures.
- [ ] Run `cargo build --workspace --release --locked` and fix release-only failures.
- [ ] Confirm production `orbis-control` starts from `UiState::production_initial()` and does not depend on fixture/mock state.
- [ ] Confirm interactive GUI launch as effective UID 0 is rejected before preferences/runtime/bus setup.
- [ ] Close or rewrite stale GitHub issues only when they materially misdirect implementation. Do not spend a session on issue bookkeeping.

Exit condition: current source builds, tests are meaningful, and no stale checklist item is being treated as a blocker when the production code already solved it.

---

## Phase 1 — make the Arch install path internally correct

Goal: one local installation command produces a coherent runnable system on Arch.

### 1.1 Fix binary/service path ownership

There is currently a concrete mismatch to resolve: the local Arch installer places `orbis-sessiond` under `/usr/local/bin`, while the shipped user unit references `/usr/bin/orbis-sessiond`.

- [ ] Make `packaging/install-arch.sh` and the installed `orbis-sessiond.service` agree on the executable path.
- [ ] Make the same ownership decision explicit for `orbis-hardwared`.
- [ ] Keep local/dev installation under a coherent local prefix; do not accidentally overwrite files that should later be pacman-owned.
- [ ] Keep release/package units suitable for a future `/usr/bin` package layout. If local and package paths differ, generate/substitute them deliberately rather than keeping contradictory files.

### 1.2 Verify installed assets

- [ ] D-Bus policy installs to the intended system location.
- [ ] polkit actions install correctly.
- [ ] system `orbis-hardwared.service` is loadable.
- [ ] user `orbis-sessiond.service` is loadable for the real desktop user.
- [ ] desktop/AppStream assets point to the installed binary/identity.
- [ ] uninstall/overwrite behavior is explicit enough that a later pacman package will not silently conflict with local development files.

### 1.3 Local install smoke

When running on the user's Arch machine and privileged installation is authorized:

```bash
bash packaging/install-arch.sh
systemctl is-active orbis-hardwared.service
systemctl --user is-active orbis-sessiond.service
```

Then verify both expected D-Bus names are actually owned and launch `orbis-control` **without sudo**.

Exit condition: a clean local Arch installation starts both daemons and the GUI without manual path surgery.

---

## Phase 2 — make the installed application a truthful read-only dashboard first

Goal: before validating mutations, every main screen must show real current machine state or a useful unsupported/unavailable state.

Work through visible UI in this order:

- [ ] Dashboard: real CPU/GPU temperatures, fans, battery, AC/power and relevant GPU state; no fixture values.
- [ ] Performance: current profile and available profiles are read from the real backend.
- [ ] Power/Battery: charge state, health/status where available, and current charge limit are real.
- [ ] Cooling: CPU/GPU fan observations and profile-specific curve state are real; malformed/unsupported curves fail honestly.
- [ ] Graphics: ASUS product GPU state, queued state and reboot-required semantics are real.
- [ ] Backlight/ASUS extras: current supported keyboard/panel/Aura state is real where an authoritative reader exists.
- [ ] System/Diagnostics: device identity and diagnostic collection work in the installed application.
- [ ] Preferences/autostart/tray/close/start-minimized behavior works from installed paths.

For every surface: UI -> runtime/application -> provider/service -> authoritative observation/error. Do not repair a broken read path by filling in plausible defaults.

Exit condition: the application is already useful as a read-only monitor and lifecycle/settings application even if every mutation is disabled.

---

## Phase 3 — validate and finish the core daily mutations

Goal: supported controls work end to end on the target machine with confirmed state, pending state, or a useful error.

**Real hardware writes require explicit user authorization for that validation session.** Lack of authorization blocks live mutation testing only; continue implementing and testing with fakes/P2P environments until the live-validation checkpoint.

### 3.1 Performance

- [ ] UI applies a selected supported profile through `Hardware1`.
- [ ] Read-back confirms the resulting profile.
- [ ] Failure/permission/unavailable states resynchronize UI instead of leaving optimistic selection.

### 3.2 Battery charge limit

- [ ] Current limit reads correctly.
- [ ] Mutation is enabled only when the real asusd/effective-threshold path is currently usable.
- [ ] Requested limit is confirmed by authoritative read-back.
- [ ] asusd disappearance/restart does not fabricate continued support.

### 3.3 ASUS product GPU mode

- [ ] Use the product-level ASUS GPU flow as the user-facing mutation path.
- [ ] Preserve current vs queued vs reboot-required state distinctly.
- [ ] Do not expose raw/legacy GPU mutation as a fake alternative when its backend is absent or product semantics are not appropriate.

### 3.4 Fans

- [ ] Keep per-fan CPU/GPU read support independent; one readable fan must not imply the other is supported.
- [ ] Preserve authoritative `FanCurveData.enabled` across custom curve writes and verify it after the setter.
- [ ] Custom curve Apply must perform authoritative read-back and surface mismatch/error honestly.
- [ ] **Do not ship Fan Factory Defaults in v0.1 unless its upstream reset path is proven failure-safe.** If that cannot be proven quickly, remove/disable that control and keep custom curve editing only.

### 3.5 Keyboard / Panel / Aura

These paths have historical FA707NV live evidence, but the current Arch-installed revision must still be checked.

- [ ] Keyboard brightness: bounded read-back, no blind mutation retry.
- [ ] Panel Overdrive: authoritative read-back.
- [ ] Aura Static RGB: preserve the rest of the mode record; claim only the level of confirmation actually observable.
- [ ] Unsupported hardware hides/disables the control honestly rather than showing a working-looking dead control.

Exit condition: the supported controls needed for normal daily use work from the installed GUI on the target Arch machine.

---

## Phase 4 — remove unfinished product surfaces instead of expanding them

Goal: v0.1 contains fewer complete features, not many half-features.

The default decision for the first release is already made here so agents do not stop to redesign scope.

### Automation

**Decision for v0.1: remove/defer it.**

- [ ] Remove Automation from the normal interactive product surface.
- [ ] Remove production-only scaffolding that has no remaining consumer and only exists for the unfinished Automation feature.
- [ ] Keep generic domain helpers only if another real product flow uses them.
- [ ] Do not spend time finishing unattended Automation before v0.1.

### Display refresh mutation

**Decision for v0.1: read-only.**

- [ ] Keep useful display/output information.
- [ ] Remove or hide refresh-rate/modeset mutation UI that has no concrete working compositor owner on the target environment.
- [ ] Do not invent a privileged display proxy to make the button work.

### Self-update

**Decision for v0.1: package manager only.**

- [ ] Remove/hide application self-update UI and dormant updater code if it has no other consumer.
- [ ] Version/About may report installed version; updating belongs to pacman/AUR/manual package flow.

### Other unused foundations

- [ ] Delete dead modules, duplicate state models, empty feature shells and obsolete compatibility paths encountered during the above work.
- [ ] Do not refactor large files merely for line count; split only where it helps maintain a live feature.

Exit condition: every visible v0.1 control has a real owner and purpose. There are no knowingly dead buttons or large dormant product subsystems pretending to be near completion.

---

## Phase 5 — final Arch packaging

Goal: produce a normal Arch-installable release candidate after the application itself works.

- [ ] Create a real `packaging/arch/PKGBUILD` suitable for the chosen release/source strategy.
- [ ] Package binaries under the normal package prefix (`/usr/bin`).
- [ ] Install system and user systemd units to normal package-owned locations.
- [ ] Install D-Bus policy, polkit actions, desktop file, AppStream metadata and icon to normal package-owned locations.
- [ ] Add a real application icon; remove empty icon placeholders from release expectations.
- [ ] Do not run/enable systemd services from `package()`.
- [ ] Ensure the package layout and `packaging/install-arch.sh` exercise the same binaries/assets even when their prefixes differ.
- [ ] Run appropriate Arch package sanity checks (including `namcap` when available) and fix substantive findings.

Exit condition: a package can be built and installed on Arch without relying on repository-relative files after installation.

---

## Phase 6 — release candidate verification

Run once the product work above is complete, not after every small edit.

- [ ] `scripts/verify full` passes.
- [ ] Fresh release build passes with default production features only.
- [ ] Normal release dependency graph does not require `orbis-test-support`/mock providers.
- [ ] Install the release candidate on the target Arch machine.
- [ ] Verify `orbis-hardwared` and `orbis-sessiond` lifecycle.
- [ ] Verify GUI launch as normal user.
- [ ] Verify tray/close/startup/preferences/diagnostics.
- [ ] Verify read-only dashboard state.
- [ ] With explicit authorization, perform bounded round-trip validation of each enabled hardware mutation and restore test values where applicable.
- [ ] Confirm no known unsafe write or unvalidated Factory Defaults path is enabled by default.
- [ ] Bump version only after the installed candidate passes.
- [ ] Create the first release.

---

## Agent execution rules

1. Start at the earliest unchecked phase whose work can be done in the current environment.
2. Inspect enough source to act, then implement; do not spend a session writing another audit.
3. Complete coherent vertical slices across crates/UI/services as needed.
4. After a substantial slice, run targeted verification and fix failures immediately.
5. Mark checkboxes only for work actually verified at the level the checkbox states.
6. If a checkbox is already satisfied by current source, verify it, mark it, and keep going in the same session.
7. Do not stop because one test/build/check passes.
8. Do not add new features until Phases 0–4 are complete unless the user explicitly changes scope.
9. Real hardware mutation validation is the one expected point that may require explicit user authorization/device access; software implementation must continue without it.
10. Finish with a short report of what now works, what was verified, and the next genuine blocker. Do not replace implementation with a progress report.
