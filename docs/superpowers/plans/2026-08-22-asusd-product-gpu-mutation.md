# ASUS Product GPU Mutation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a separate typed ASUS product-GPU queue operation for `Hybrid`, `Integrated`, and `Ultimate`, with paired attribute read-back and reboot-required semantics.

**Architecture:** Keep raw `Hardware1.SetGpuMode` and its supergfxd adapter unchanged. Add a separate ASUS-specific backend/client and `Hardware1.SetProductGpuMode` method that validates the current pair, authorizes the original caller, queues both attributes through asusd, and verifies current/queued state without rebooting or retrying.

**Tech Stack:** Rust 2024, Tokio, zbus, existing Hardware1/polkit infrastructure, private P2P fake D-Bus services, Slint UI.

**Spec:** `docs/superpowers/specs/2026-08-22-asusd-product-gpu-mutation-design.md`

## Global Constraints

- Do not modify raw `Hardware1.SetGpuMode` or supergfxd semantics.
- Do not accept paths, attribute names, arbitrary values, shell commands, or generic D-Bus forwarding.
- Supported product modes are exactly `Hybrid`, `Integrated`, `Ultimate`; `Optimized` remains unavailable.
- Pair mapping is exact: Hybrid `(0,1)`, Integrated `(1,1)`, Ultimate `(0,0)`.
- Set both attributes in deterministic order and read current/queued values for both afterward.
- Timeout after possible dispatch is unknown; never retry or auto-rollback.
- Never reboot, logout, shutdown, stop display manager, unload drivers, or change PCI/runtime-PM state automatically.
- Private tests use fake P2P D-Bus only and no valid real GPU mutation.
- Before any real host write, stop and require a fresh read-before-write confirmation from the user.

---

### Task 1: Add pure ASUS product-GPU request/read-back model

**Files:**
- Modify: `crates/orbis-providers/src/asus_gpu_mode.rs`
- Test: inline tests in the same file

**Interfaces:**
- Consumes: existing `AsusGpuMode`, `AsusGpuModeSnapshot`, `decode_asus_gpu_mode`.
- Produces: typed target mapping and read-back classification used by hardwared.

- [ ] **Step 1: Write failing tests**

Add exact tests:

```rust
#[test]
fn product_modes_have_exact_attribute_targets() {
    assert_eq!(target_values(AsusGpuMode::Hybrid), Some((0, 1)));
    assert_eq!(target_values(AsusGpuMode::Integrated), Some((1, 1)));
    assert_eq!(target_values(AsusGpuMode::Ultimate), Some((0, 0)));
    assert_eq!(target_values(AsusGpuMode::Incomplete), None);
}

#[test]
fn queued_target_requires_both_attributes_to_match() {
    let target = AsusGpuMode::Integrated;
    assert_eq!(classify_product_gpu_readback(target, snapshot(1, 1, Some(1), Some(1))), ProductGpuOutcome::RebootRequired);
    assert_eq!(classify_product_gpu_readback(target, snapshot(1, 1, Some(1), None)), ProductGpuOutcome::Unknown);
    assert_eq!(classify_product_gpu_readback(target, snapshot(1, 1, Some(0), Some(1))), ProductGpuOutcome::Inconsistent);
}
```

- [ ] **Step 2: Run focused tests and verify failure**

```bash
cargo test -p orbis-providers asus_gpu_mode --locked
```

Expected: failure because target mapping/classification API is absent.

- [ ] **Step 3: Implement pure mapping and classification**

Add:

```rust
pub fn target_values(mode: AsusGpuMode) -> Option<(u32, u32)>;
pub enum ProductGpuOutcome { AlreadyActive, RebootRequired, Unknown, Inconsistent }
pub fn classify_product_gpu_readback(mode: AsusGpuMode, snapshot: AsusGpuModeSnapshot) -> ProductGpuOutcome;
```

Treat partial queue, unknown current state, and missing values as `Unknown`; conflicting known values as `Inconsistent`. Do not add I/O here.

- [ ] **Step 4: Run focused green checks**

```bash
cargo fmt --all -- --check
cargo test -p orbis-providers asus_gpu_mode --locked
cargo clippy -p orbis-providers --all-targets --locked -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/orbis-providers/src/asus_gpu_mode.rs
git commit -m "providers: classify ASUS GPU queue outcomes"
```

### Task 2: Add typed asusd paired mutation backend and private P2P tests

**Files:**
- Create: `crates/orbis-hardwared/src/asus_gpu_mode.rs`
- Modify: `crates/orbis-hardwared/src/lib.rs`
- Create: `crates/orbis-hardwared/tests/asus_gpu_mode_p2p.rs`

**Interfaces:**
- Consumes: Task 1 target/classification API and existing `Authorizer`/`provider_error_to_dbus` patterns.
- Produces: internal `AsusGpuMutationOperation` and `AsusdGpuMutationClient`, with no Hardware1 exposure until Task 3.

- [ ] **Step 1: Write private P2P failing tests**

Fake attributes must expose only typed `CurrentValue`, `QueuedGpuValue`, and `SetCurrentValue(i32)` methods/properties. Add tests for:

- current `Hybrid`, target `Integrated` → both setters once, queued pair confirmed;
- current already target → no setters and `AlreadyActive`;
- first setter failure → no second setter, error preserved;
- second setter failure → one setter may have happened, return unknown/inconsistent, no retry;
- partial queued read-back → `Unknown`;
- contradictory queued pair → `Inconsistent`;
- unknown target/unsupported mode rejected before setter.

Every test asserts exact setter order and count.

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test -p orbis-hardwared --test asus_gpu_mode_p2p --locked
```

- [ ] **Step 3: Implement typed client/backend**

Use fixed constants for the two asusd object paths. Read current state before authorization/backend writes at the orchestration boundary, set `dgpu_disable` first and `gpu_mux_mode` second, then read both current and queued values. Never call `ApplyQueuedGpuValue` in this operation.

- [ ] **Step 4: Run focused green checks**

```bash
cargo fmt --all -- --check
cargo test -p orbis-hardwared --test asus_gpu_mode_p2p --locked
cargo clippy -p orbis-hardwared --all-targets --locked -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/orbis-hardwared/src/asus_gpu_mode.rs crates/orbis-hardwared/src/lib.rs crates/orbis-hardwared/tests/asus_gpu_mode_p2p.rs
git commit -m "hardwared: add paired ASUS GPU queue backend"
```

### Task 3: Expose separate Hardware1 method and polkit action

**Files:**
- Modify: `crates/orbis-hardwared/src/lib.rs`
- Modify: `crates/orbis-hardwared/src/main.rs`
- Modify: `crates/orbis-session-client/src/lib.rs`
- Modify: `packaging/nix/polkit/io.github.orbiscontrol.hardware.policy`
- Modify: `packaging/nix/tests/hardwared-lifecycle.nix`
- Test: `crates/orbis-hardwared/tests/asus_gpu_mode_p2p.rs`

**Interfaces:**
- Consumes: Task 2 internal backend.
- Produces: typed `Hardware1.SetProductGpuMode(u32) -> ProductGpuMutationResult` and a separate action `io.github.orbiscontrol.hardware.set-product-gpu-mode`.

- [ ] **Step 1: Add failing boundary tests**

Assert strict wire decoding (`0=Hybrid`, `1=Integrated`, `2=Ultimate`, other invalid), authorization before backend call, separate action identity, and result fields for current/queued pair plus `reboot_required`.

- [ ] **Step 2: Run focused boundary tests and verify failure**

```bash
cargo test -p orbis-hardwared --test asus_gpu_mode_p2p --locked
```

- [ ] **Step 3: Implement the typed method**

Add the method alongside existing Hardware1 methods, but keep default production composition disabled until the real read-before-write gate. Preserve raw `SetGpuMode` unchanged. Add polkit action with `allow_active=no` initially; the method must be denied in production until controlled validation and explicit promotion.

- [ ] **Step 4: Verify packaging metadata**

Run:

```bash
xmllint --noout packaging/nix/polkit/io.github.orbiscontrol.hardware.policy
```

Update lifecycle VM assertions for the new action and method while preserving default-deny.

- [ ] **Step 5: Commit**

```bash
git add crates/orbis-hardwared/src/lib.rs crates/orbis-hardwared/src/main.rs crates/orbis-session-client/src/lib.rs packaging/nix/polkit/io.github.orbiscontrol.hardware.policy packaging/nix/tests/hardwared-lifecycle.nix crates/orbis-hardwared/tests/asus_gpu_mode_p2p.rs
git commit -m "hardwared: expose typed ASUS product GPU method"
```

### Task 4: Connect UI request/pending state, still fail-closed

**Files:**
- Modify: `crates/orbis-ui/src/controller.rs`
- Modify: `crates/orbis-ui/src/main.rs`
- Modify: `crates/orbis-ui/src/worker_runtime.rs`
- Modify: `ui/audited/model.slint`
- Modify: `ui/audited/main-window.slint`
- Test: `crates/orbis-ui/src/main_tests.rs`

**Interfaces:**
- Consumes: Task 3 typed client/result; existing UI state and worker FIFO.
- Produces: Hybrid/Integrated/Ultimate controls showing queued target and reboot-required state; no automatic reboot and no enabled production mutation until capability/polkit promotion.

- [ ] **Step 1: Write failing UI tests**

Test that current mode maps to the selected card, queued mode is shown as pending, reboot-required text is present, `Optimized` is not generated, and controls remain disabled when write evidence is not `Supported`.

- [ ] **Step 2: Implement state/event mapping**

Keep request serialized through the worker FIFO. Apply only authoritative current/queued read-back to UI state. A successful queued result must not set current applied mode optimistically.

- [ ] **Step 3: Run targeted UI checks**

```bash
cargo fmt --all -- --check
cargo check -p orbis-ui --all-targets --locked
cargo test -p orbis-ui --locked
cargo clippy -p orbis-ui --all-targets --locked -- -D warnings
python3 scripts/verify-static
```

- [ ] **Step 4: Commit**

```bash
git add crates/orbis-ui/src/controller.rs crates/orbis-ui/src/main.rs crates/orbis-ui/src/worker_runtime.rs ui/audited/model.slint ui/audited/main-window.slint crates/orbis-ui/src/main_tests.rs
git commit -m "ui: show ASUS GPU queued mode and reboot state"
```

### Task 5: Stop at real read-before-write gate

**Files:**
- Modify: `docs/current-state.md` only for exact evidence status
- Test: workspace verification commands

- [ ] **Step 1: Run final checks**

```bash
python3 scripts/verify-static
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
nix build .#orbis-control --no-link --print-build-logs
```

- [ ] **Step 2: Perform only read-only host inspection**

```bash
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/asus_armoury/dgpu_disable xyz.ljones.AsusArmoury CurrentValue
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/asus_armoury/dgpu_disable xyz.ljones.AsusArmoury QueuedGpuValue
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/asus_armoury/gpu_mux_mode xyz.ljones.AsusArmoury CurrentValue
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/asus_armoury/gpu_mux_mode xyz.ljones.AsusArmoury QueuedGpuValue
```

- [ ] **Step 3: Stop and ask for explicit real-write confirmation**

Before calling `SetProductGpuMode` on the host, report current/queued values, target mode, expected queued pair, and the fact that the next shutdown/reboot applies the firmware state. Do not call the method in this plan automatically.
