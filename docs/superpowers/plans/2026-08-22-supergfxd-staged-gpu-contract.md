# Staged supergfxd GPU Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose and verify the existing typed `supergfxd` staged lifecycle as a read-only snapshot/request contract for `Hybrid <-> Integrated`, without enabling production GPU mutation or product-mode mapping.

**Architecture:** `orbis-providers::supergfxd` remains the pure backend model and classifier. `orbis-hardwared::SupergfxdMutationBackend` remains an internal typed adapter with no Hardware1 exposure; this plan adds only a read-only snapshot entry point and exact P2P coverage around the existing one-request/read-back flow. `supergfxd` remains the owner of display-manager, driver, PCI and runtime-PM lifecycle.

**Tech Stack:** Rust 2024, MSRV 1.87, Tokio, zbus 5, existing `orbis-providers` supergfxd types, existing `orbis-hardwared` typed client, private Unix-stream P2P fake service.

**Spec:** `docs/superpowers/specs/2026-08-22-supergfxd-staged-gpu-contract-design.md`

## Global Constraints

- Do not add product `Eco/Standard/Ultimate/Optimized` mapping.
- Do not expose or enable `Hardware1` GPU writes, polkit changes, UI mutation controls, logout/reboot automation, or Nix sandbox changes.
- `SetMode` is sent at most once per request; no retry or automatic rollback.
- Unknown wire values remain explicit `Unknown(value)` and classify fail-closed.
- A returned user action is advisory and never means `Applied` without read-back.
- Tests use the existing private P2P fake service only; no real system bus, real `supergfxd`, or valid hardware mutation.
- No new dependencies.

---

### Task 1: Add the read-only snapshot entry point

**Files:**
- Modify: `crates/orbis-hardwared/src/supergfxd.rs`
- Test: `crates/orbis-hardwared/tests/supergfxd_mutation_p2p.rs`

**Interfaces:**
- Consumes: existing `SupergfxdMutationBackend<C>`, `SupergfxdMutationClient`, `SupergfxdSnapshot`, and private P2P fake service.
- Produces: `pub async fn read_snapshot(&self) -> Result<SupergfxdSnapshot, ProviderError>` on `SupergfxdMutationBackend<C>`.

- [ ] **Step 1: Extend the fake call journal and write the failing test**

Add a `FakeCall` enum to the P2P test fixture:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeCall {
    Supported,
    SetMode(u32),
    Mode,
    PendingMode,
    PendingUserAction,
    Power,
}
```

Change `FakeState.calls` to `Arc<Mutex<Vec<FakeCall>>>` and append the corresponding entry in every fake method. Add:

```rust
#[tokio::test]
async fn read_snapshot_reads_all_fields_once_in_order() {
    let state = Arc::new(Mutex::new(state(snapshot(0, 6, 4), 4)));
    let (_server, client) = connect_fake(state.clone()).await.expect("private P2P");
    let backend = SupergfxdMutationBackend::new(ZbusSupergfxdMutationClient::new(client));

    let result = backend.read_snapshot().await.expect("snapshot");
    assert_eq!(result.current_mode, SupergfxdMode::Hybrid);
    assert_eq!(result.pending_mode, SupergfxdMode::None);
    assert_eq!(result.pending_user_action, SupergfxdUserAction::Nothing);
    assert_eq!(result.power, GpuPowerState::Suspended);
    assert_eq!(calls(&state), vec![
        FakeCall::Mode,
        FakeCall::PendingMode,
        FakeCall::PendingUserAction,
        FakeCall::Power,
        FakeCall::Supported,
    ]);
}
```

Import `GpuPowerState` in the test and adapt the existing `calls` helper to return `Vec<FakeCall>`.

- [ ] **Step 2: Run the focused test and verify the expected failure**

Run:

```bash
cargo test -p orbis-hardwared --test supergfxd_mutation_p2p read_snapshot_reads_all_fields_once_in_order --locked
```

Expected: compilation fails because `SupergfxdMutationBackend::read_snapshot` does not exist yet. No production code is changed before this failing test.

- [ ] **Step 3: Implement the minimum read-only helper**

Move the existing private `fresh_snapshot` body behind this public method:

```rust
pub async fn read_snapshot(&self) -> Result<SupergfxdSnapshot, ProviderError> {
    self.fresh_snapshot().await
}
```

Keep the existing private helper if it is still needed by `request_mode`, or make `request_mode` call the new method. Do not change request ordering or add retries.

- [ ] **Step 4: Run the focused green checks**

Run:

```bash
cargo fmt --all
cargo test -p orbis-hardwared --test supergfxd_mutation_p2p read_snapshot_reads_all_fields_once_in_order --locked
cargo clippy -p orbis-hardwared --all-targets --locked -- -D warnings
```

Expected: the new test and clippy pass.

- [ ] **Step 5: Commit**

```bash
git add crates/orbis-hardwared/src/supergfxd.rs crates/orbis-hardwared/tests/supergfxd_mutation_p2p.rs
git commit -m "hardwared: expose staged supergfxd snapshot read"
```

### Task 2: Lock down staged request and failure semantics

**Files:**
- Modify: `crates/orbis-hardwared/tests/supergfxd_mutation_p2p.rs`
- Modify: `crates/orbis-hardwared/src/supergfxd.rs` only when a test proves an implementation defect

**Interfaces:**
- Consumes: `SupergfxdMutationBackend::read_snapshot` from Task 1 and existing `request_mode`/`MutationObservation`.
- Produces: exact private P2P evidence for one request, fresh read-back, action consistency and fail-closed errors.

- [ ] **Step 1: Update existing call-count assertions to the explicit journal**

For every existing request test, assert that `FakeCall::SetMode(requested_wire)` appears exactly once. Preserve the existing scenario assertions for `Applied`, `Pending`, `RequiresUserAction`, and `Inconsistent`.

- [ ] **Step 2: Add the missing failing scenarios**

Add these focused tests:

```rust
#[tokio::test]
async fn unsupported_request_is_rejected_before_set_mode() {
    let (state, result) = operation(state(snapshot(0, 6, 4), 4), SupergfxdMode::AsusEgpu).await;
    assert!(matches!(result, Err(ProviderError::Unsupported(_))));
    assert!(!calls(&state).iter().any(|call| matches!(call, FakeCall::SetMode(_))));
}

#[tokio::test]
async fn returned_action_mismatch_is_inconsistent() {
    let (state, result) = operation(state(snapshot(0, 1, 0), 4), SupergfxdMode::Integrated).await;
    assert_eq!(result.expect("read-back").state, SupergfxdStagedState::Inconsistent);
    assert_eq!(calls(&state).iter().filter(|call| matches!(call, FakeCall::SetMode(_))).count(), 1);
}

#[tokio::test]
async fn readback_failure_is_not_retried_or_reported_as_applied() {
    let mut fake = state(snapshot(0, 1, 0), 0);
    fake.read_error = Some("read-back failed".into());
    let (state, result) = operation(fake, SupergfxdMode::Integrated).await;
    assert!(result.is_err());
    assert_eq!(calls(&state).iter().filter(|call| matches!(call, FakeCall::SetMode(_))).count(), 1);
}
```

Also retain coverage for future wire values, set-mode D-Bus failure, `Pending`, `Logout`, `Reboot`, and contradictory snapshots.

- [ ] **Step 3: Run the focused tests and verify failures**

Run:

```bash
cargo test -p orbis-hardwared --test supergfxd_mutation_p2p --locked
```

Expected: any failure must identify a missing call-journal assertion or a concrete classifier/client defect; do not weaken the tests.

- [ ] **Step 4: Implement only proven defects**

Preserve the current `request_mode` order: supported validation, exactly one `SetMode`, fresh snapshot, classifier, returned-action consistency check. If all tests pass without production changes, keep the task test-only.

- [ ] **Step 5: Run the complete focused suite**

Run:

```bash
cargo test -p orbis-providers supergfxd --locked
cargo test -p orbis-hardwared --test supergfxd_mutation_p2p --locked
cargo clippy -p orbis-providers -p orbis-hardwared --all-targets --locked -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/orbis-providers/src/supergfxd.rs crates/orbis-hardwared/src/supergfxd.rs crates/orbis-hardwared/tests/supergfxd_mutation_p2p.rs
git commit -m "tests: lock down staged supergfxd outcomes"
```

### Task 3: Integration verification and non-promotion record

**Files:**
- Modify: `docs/current-state.md` only if the source status wording needs updating
- Test: repository verification commands

**Interfaces:**
- Consumes: Tasks 1-2 typed contract and private P2P evidence.
- Produces: exact-build `IMPLEMENTED/TESTED` evidence for the staged contract only.

- [ ] **Step 1: Run workspace checks**

Run:

```bash
python3 scripts/verify-static
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

- [ ] **Step 2: Run the package build once at the end**

Because this slice changes Rust service code but not packaging, run Nix only at the end:

```bash
nix build .#orbis-control --no-link --print-build-logs
```

This proves package compilation/tests, not live `supergfxd` behavior.

- [ ] **Step 3: Inspect promotion boundaries**

Confirm the final diff does not change production `Hardware1` GPU composition, polkit defaults, UI write gates, NixOS writable paths, product-mode mapping, automatic logout/reboot, or display-manager lifecycle.

- [ ] **Step 4: Update status documentation if needed**

Record only the staged typed contract as `IMPLEMENTED/TESTED`. Keep production GPU/product mutation `BLOCKED` and live hardware validation unclaimed.

- [ ] **Step 5: Commit documentation if changed**

```bash
git add docs/current-state.md
git commit -m "docs: record staged GPU contract evidence"
```
