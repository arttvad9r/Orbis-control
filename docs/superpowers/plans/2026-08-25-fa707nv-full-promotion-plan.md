# FA707NV Full Capability Promotion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote only proven keyboard, fan and ASUS product-GPU capabilities on FA707NV, and add a real typed display owner before any display write.

**Architecture:** Reuse existing typed Hardware1 backends and capability-level polkit. Production composition is changed only after source/P2P/VM evidence; live validation uses the current NixOS deployment and restores the original state. Raw GPU remains disabled without `supergfxd`; display remains read-only until a compositor owner exists.

**Tech Stack:** Rust 2024/MSRV 1.87, zbus, Tokio, NixOS/systemd/polkit, Slint, real ASUS FA707NV/asusd/KWin.

**Spec:** `docs/superpowers/specs/2026-08-25-fa707nv-full-promotion-design.md`

## Global Constraints

- No writes to other models or unsupported owners.
- No generic sysfs/shell/D-Bus proxy.
- Every mutation must read before write, validate ranges, authoritative-read-back and restore live state.
- `Unsupported`, `PermissionDenied`, `TemporarilyUnavailable`, `Unknown` remain distinct.
- Hosted GitHub Actions is optional; local locked Cargo + `verify-static` + full `nix flake check` are canonical.
- Fan, GPU, keyboard and display promotion are independent gates; enabling one never implies another.

---

### Task 1: Promote keyboard backlight on FA707NV

**Files:**
- Modify: `crates/orbis-hardwared/src/main.rs` production composition.
- Modify: `packaging/nix/module.nix` or the authoritative hardwared unit module to add only `/sys/class/leds/asus::kbd_backlight` write access.
- Modify: `packaging/nix/polkit/io.github.orbiscontrol.hardware.policy` keyboard action.
- Test: existing `crates/orbis-hardwared/src/keyboard_backlight.rs` tests and `packaging/nix/tests/hardwared-lifecycle.nix`.
- Evidence: `docs/hardware-evidence/fa707nv-live-validation-2026-08-25-keyboard.md`.

**Implementation:**
- Construct `SysfsKeyboardBacklightMutationBackend` only after read-only probe confirms `brightness`, `max_brightness`, valid `0..=max` and path ownership.
- Change the production writable surface to the exact ASUS LED directory; do not grant `/sys/class/leds` broadly.
- Set only `io.github.orbiscontrol.hardware.set-keyboard-backlight` active-session authorization; preserve deny for unrelated capabilities.
- Keep UI gate based on `KeyboardBacklightMutationStatus::Supported`.
- Live test `3→0→3` (or the exact pre-read value) through Hardware1, verify kernel read-back after each write and restore the initial value.

**Checks:** focused hardwared tests, sandbox VM check, `verify-static`, full flake check, NixOS switch, live round-trip.

### Task 2: Promote fan custom/reset backend

**Files:**
- Modify: `crates/orbis-hardwared/src/main.rs` to compose `AsusdFanCurveMutationBackend<ZbusAsusdFanCurveClient>`.
- Modify: `packaging/nix/polkit/io.github.orbiscontrol.hardware.policy` fan action.
- Modify: `packaging/nix/tests/hardwared-lifecycle.nix` expected fan policy only if the promotion decision explicitly changes the gate.
- Extend: `crates/orbis-hardwared/src/bin/fan-live-validate.rs` only if the current evidence needs a missing assertion.
- Update: `docs/hardware-evidence/fa707nv-live-validation-2026-08-25-fan.md` and `docs/current-state.md`.

**Implementation:**
- Preserve the already-tested algorithm: fresh stored `enabled` read, one typed setter, fresh CPU/GPU read-back, exact point/enabled comparison.
- Keep factory reset profile-scoped and never switch the kernel platform profile in Orbis.
- Before production composition change, run the existing harness after current NixOS deployment and record pre/post fan curves and profile.
- Enable UI only from operation-level `Supported`; malformed aggregate reads remain visible as honest absence, never synthetic defaults.

**Checks:** hardwared fan unit/P2P tests, full Nix/VM checks, current live harness, restore/final-state inspection. If the aggregate FA707NV read remains malformed, keep fan UI read-only while mutation status remains independently truthful.

### Task 3: Promote ASUS product GPU mode

**Files:**
- Modify: `crates/orbis-hardwared/src/main.rs` to attach the existing `AsusGpuMutationBackend` through the typed builder.
- Modify: `packaging/nix/polkit/io.github.orbiscontrol.hardware.policy` product-GPU action.
- Test: existing product-GPU P2P/VM tests and add live-safe no-op queue/read-back coverage if absent.
- Update: `docs/hardware-evidence/fa707nv-live-validation-2026-08-25-gpu.md` and current-state.

**Implementation:**
- Use only the paired `dgpu_disable` + `gpu_mux_mode` owner; do not map raw supergfxd enums into product modes.
- Validate current and queued pairs before mutation, preserve `Pending`/reboot semantics, require both authoritative values in read-back, and never auto-reboot.
- First live mutation is a no-op Hybrid→Hybrid queue/read-back with no reboot requirement; only after that consider a user-confirmed staged mode change with explicit restore plan.
- Keep raw `SetGpuMode` disabled because `supergfxd` is absent on FA707NV.

**Checks:** focused provider/hardwared/P2P tests, full flake check, live no-op queue/read-back, no reboot performed automatically.

### Task 4: Implement a concrete display refresh owner

**Files:**
- Modify/create: `crates/orbis-providers/src/display_refresh_owner.rs` concrete KWin/KScreen owner only after its stable D-Bus API and target identity are proven.
- Modify: `crates/orbis-application`/`orbis-ui` composition to inject the typed owner.
- Add: capability-specific polkit action only if the owner requires privilege.
- Test: private-P2P owner contract tests and a fake compositor integration test.
- Update: display readiness and evidence docs.

**Implementation:**
- Prove internal `eDP-2` identity and exact supported presets before any setter.
- Use typed D-Bus/KWin API with fresh read-back; no `kscreen-doctor`, `xrandr`, shell or guessed connector paths.
- Add pending/rollback behavior for modeset failure and preserve read-only behavior when the owner is absent.
- Do not start live display mutation until the concrete owner passes source/P2P/VM checks and a dry-run target proof.

**Checks:** owner contract/P2P tests, full flake check, controlled refresh `144→60→144` only after explicit owner proof and user-visible restore confirmation.

### Task 5: Final FA707NV release acceptance

**Files:**
- Update: `docs/current-state.md`, `docs/verification.md`, `docs/hardware-evidence/*`, `docs/beta-acceptance-checklist.md`.
- Do not modify: other-model support claims or unrelated capability gates.

**Steps:**
- Run locked Cargo fmt/check/test/clippy, `python3 scripts/verify-static`, full `nix flake check --max-jobs 1 --cores 4`.
- Deploy the exact resulting NixOS closure to FA707NV.
- Re-run each promoted capability’s read/status/mutation/read-back/restore scenario.
- Record exact commit, package path, owner services, sandbox paths, polkit actions, pre/post values and any unsupported capability.
- Keep raw GPU, Aura and display mutation blocked if their independent owners/evidence are not complete.

**Commit boundaries:** one focused commit per capability promotion, followed by one evidence/docs commit; never combine unrelated promotion gates.
