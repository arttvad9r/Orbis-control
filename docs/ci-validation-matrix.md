# CI Validation Matrix

> Роль: **CURRENT VERIFICATION MAP**.
> Обновлено: 2026-08-20.
>
> Evidence rules также определены в [`verification.md`](verification.md), [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md) и `AGENTS.md`.
>
> GitHub Actions execution currently **BLOCKED** by #106: a workflow definition is not PASS evidence until runner steps actually execute.

## Layer 0 — stdlib static contracts

No Rust/Nix toolchain required:

```bash
python3 scripts/verify-static
```

Current suite includes source contracts for:

- Slint/UI request-only and fake-success invariants;
- Automation shadow/executor/recovery invariants;
- Display Refresh fail-closed boundaries;
- cross-surface backend completion status;
- provider timeout / canonical capability-refresh invariants;
- canonical documentation/current-status consistency and absence of one-off `.github` validation artifacts.

This layer is a **fail-fast source check**, not compilation or runtime evidence.

## Layer 1 — Rust workspace

Intended exact-revision checks:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Use `--locked` for repository/release claims.

## Layer 2 — Nix / VM checks

Current flake checks include:

- `checks.default` — Rust workspace/package validation;
- `checks.hardwared-lifecycle` — Hardware1 service/bus/sandbox/invalid-wire VM;
- `checks.performance-mutation-vm` — controlled fake-file Performance mutation VM;
- `checks.battery-mutation-vm` — controlled fake-asusd/fake-power-supply Battery mutation VM.

Full gate:

```bash
nix flake check --max-jobs 1 --cores 4
```

Automated checks must not perform real ASUS hardware mutation.

## GitHub Actions pipeline

`.github/workflows/ci.yml` is the only canonical workflow. Intended order:

1. Checkout;
2. `python3 scripts/verify-static`;
3. install Nix;
4. flake evaluation;
5. Rust/package check;
6. Hardware1 lifecycle VM;
7. Performance mutation VM;
8. Battery mutation VM;
9. final full `nix flake check`.

The static step is deliberately before Nix so cheap contract failures are reported early once Actions execution is restored.

### Current #106 state

Historical observations include jobs failing before their first repository step (`steps=[]`, no useful log blob) and pushes with no workflow run. That is infrastructure/account/repository execution evidence, not proof of Cargo/Nix failure.

A run object, queued state or workflow YAML is not green evidence. PASS requires executed successful steps for the exact intended revision.

## Development tiers

### FAST

For a narrow source change when toolchain exists:

```bash
python3 scripts/verify-static
cargo fmt --all
cargo check -p <crate> --all-targets --locked
cargo test -p <crate> --locked
cargo clippy -p <crate> --all-targets --locked -- -D warnings
git diff --check
```

### INTEGRATION

For cross-crate/protocol/runtime changes:

```bash
python3 scripts/verify-static
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### FULL

For D-Bus/polkit/service/package/release changes:

- INTEGRATION tier;
- targeted Nix checks/VMs;
- full `nix flake check`;
- packaged acceptance where applicable.

## Hardware safety

Automated CI must not:

- access real ASUS hardware;
- call real hardware mutation methods;
- depend on host UPower/asusd/supergfxd state;
- convert VM/fake-system success into `LIVE-VALIDATED` evidence.

Controlled hardware validation is separate, dated, model/environment-specific and revision-scoped.

## Current execution claim

The active integration branch has expanded source/static contracts, but the available environment lacks Rust/Cargo/Slint and GitHub Actions remains blocked by #106. Therefore no new green Rust/Nix/Slint claim is made by documentation or static checks alone.
