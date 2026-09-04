# Orbis Control

Orbis Control is a Rust + Slint Linux application for monitoring and controlling supported ASUS ROG/TUF/Zephyrus laptop features. It is Wayland-first, keeps unsupported hardware honest, and routes privileged mutations through a narrow typed `Hardware1` service instead of running the GUI as root.

`main` is the canonical development branch.

## What already exists

The current codebase includes production paths for the core application shell and a substantial part of the daily-control feature set, including:

- system telemetry and capability discovery;
- Performance profile read/write with read-back;
- Battery charge-limit read/write with read-back;
- fan state/curve support and ASUS fan mutation paths where capability evidence permits them;
- ASUS product GPU mode read/queued mutation flow;
- keyboard backlight and selected ASUS extra controls;
- preferences, autostart, tray/window lifecycle and diagnostics;
- read-only CLI status output;
- Nix package/module, D-Bus and polkit integration;
- fake-system/private-P2P/VM integration tests for privileged boundaries.

The project still contains unfinished or deliberately disabled product surfaces. The active completion queue is [`TODO.md`](TODO.md). A feature is considered finished only when the user-visible flow is connected end to end or the unused shell has been removed.

## Development

Rust workspace: edition 2024, MSRV 1.87.

Recommended environment:

```bash
nix develop
```

Run the GUI during development:

```bash
cargo run -p orbis-ui --bin orbis-control
```

Or build/run through the flake:

```bash
nix build .#orbis-control
nix run .
```

## Verification

Use real build/test checks rather than documentation/source-marker contracts:

```bash
scripts/verify crate orbis-ui   # targeted crate while iterating
scripts/verify quick            # fmt + workspace check
scripts/verify task             # fmt + check + tests + clippy
scripts/verify full             # task checks + Nix/package/integration checks
```

Do not run the full suite after every small edit. Implement a coherent batch, run targeted checks, fix failures, and use broader verification at the end of the vertical slice.

CI runs the flake checks on pushes to `main` and pull requests.

## Architecture

Read path:

```text
UPower / kernel / asusd / supergfxd / read-only sysfs / compositor observation
→ providers / orbis-sessiond
→ Session1 / application runtime
→ worker
→ GUI / CLI / diagnostics
```

Privileged mutation path:

```text
original application caller
→ typed Hardware1 system-bus API
→ capability-specific polkit
→ narrow backend
→ authoritative read-back / explicit pending / honest error
```

Important invariants:

- the GUI is an unprivileged user-session application;
- `orbis-sessiond` is not a privileged mutation deputy;
- no generic root/sysfs/shell proxy;
- runtime evidence determines capability support, not the laptop model name alone;
- read and write support are independent;
- requested, observed and pending state stay distinct;
- `Accepted` is not automatically `Applied`;
- an unknown mutation outcome is never blindly retried.

Stable architecture details live in [`docs/architecture.md`](docs/architecture.md) and accepted ADRs under [`docs/adr/`](docs/adr/). Hardware evidence under `docs/hardware-evidence/` is revision/device-specific reference material, not a development gate.

## Repository workflow for AI agents

[`AGENTS.md`](AGENTS.md) is intentionally product-first: agents are expected to complete coherent vertical work, continue across necessary crates, and stop only at a real blocker. Documentation maintenance and source-marker test generation are not default development work.

When asked simply to continue or finish the project, start from [`TODO.md`](TODO.md), verify the actual source/runtime state, complete the highest-priority actionable slice, and continue to the next related item instead of stopping after a micro-fix.

## License

GPL-3.0-or-later.

Orbis Control is an independent project and is not affiliated with ASUSTeK Computer Inc.