# CI Validation Matrix

> Роль: **CURRENT VERIFICATION MAP**. Команды/уровни evidence определены также
> в [`verification.md`](verification.md) и `AGENTS.md`.
>
> Текущий GitHub Actions execution **BLOCKED** по #106: jobs не доходят до
> Checkout/steps, поэтому наличие workflow definition не является PASS evidence.

## Canonical flake checks

Current `flake.nix` exposes:

- `checks.default` — Cargo fmt/clippy/workspace tests using package vendoring;
- `checks.hardwared-lifecycle` — Hardware1 service/bus/sandbox/invalid-wire VM;
- `checks.performance-mutation-vm` — controlled fake-file Performance mutation VM; UPower intentionally disabled to prove capability-local Session1 startup;
- `checks.battery-mutation-vm` — controlled fake-asusd/fake-power-supply Battery mutation VM.

Full release validation:

```bash
nix flake check --max-jobs 1 --cores 4
```

No check in this matrix performs real ASUS hardware mutation.

## GitHub Actions pipeline

`.github/workflows/ci.yml` is intended to run on pull requests and pushes to
`main` with these stages:

1. Checkout;
2. install Nix;
3. flake evaluation;
4. package/Cargo check;
5. Hardware1 lifecycle VM;
6. Performance mutation VM;
7. Battery mutation VM;
8. final full `nix flake check`.

Current state under #106:

- earlier jobs fail before the first step with `steps=[]` and no log blob;
- a manual re-run reproduced the same failure;
- recent `main` pushes may produce no workflow run at all.

Therefore none of those GitHub-hosted stages can currently be claimed green on
latest `main`.

## Development tiers

### FAST

For a normal single-crate change:

```bash
cargo fmt --all
cargo check -p <crate> --all-targets
cargo test -p <crate>
cargo clippy -p <crate> --all-targets -- -D warnings
git diff --check
```

### INTEGRATION

For multi-crate/protocol/service-boundary changes:

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

### FULL

For package/module/polkit/D-Bus/release changes:

- INTEGRATION tier;
- targeted Nix evaluation/build/checks;
- full `nix flake check` before release acceptance.

## Hardware safety

Automated CI must not:

- access real ASUS hardware;
- call real hardware mutation methods;
- depend on host UPower/asusd/supergfxd state;
- turn VM/fake-system success into LIVE-VALIDATED evidence.

Controlled hardware validation remains separate, dated and revision-scoped.

## Evidence rule

A workflow file, queued run or run object is not validation evidence. PASS
requires actual executed steps and successful outputs for the exact intended
revision. Until #106 is resolved, current-main release validation is BLOCKED.