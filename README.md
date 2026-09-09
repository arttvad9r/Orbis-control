# Orbis Control

Orbis Control is a Rust + Slint Linux application for monitoring and controlling supported ASUS ROG/TUF/Zephyrus laptop features. It is Wayland-first, keeps unsupported hardware honest, and routes privileged mutations through a narrow typed `Hardware1` service instead of running the GUI as root.

`main` is the canonical development branch. The primary development environment is Arch Linux; the repository has no Nix/NixOS build dependency.

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
- systemd, D-Bus and polkit integration;
- fake-system/private-P2P integration tests for privileged boundaries.

The project still contains unfinished or deliberately disabled product surfaces. The active completion queue is [`TODO.md`](TODO.md). A feature is considered finished only when the user-visible flow is connected end to end or the unused shell has been removed.

The current local development candidate is on `agent/finish-v01`. Its software verification gate (`scripts/verify full`) is green, but the project is not yet declared released: target-session mutation evidence and clean package lifecycle validation remain tracked in `TODO.md`.

## Arch Linux development setup

Rust workspace: edition 2024, MSRV/toolchain 1.87.

Install the native build/runtime dependencies:

```bash
sudo pacman -S --needed \
  base-devel rustup pkgconf \
  fontconfig freetype2 libglvnd \
  libx11 libxcursor libxrandr libxi \
  libxkbcommon libxkbcommon-x11 \
  wayland wayland-protocols \
  dbus openssl systemd polkit upower \
  glib2 cairo pango gdk-pixbuf2

rustup toolchain install 1.87 --profile minimal --component rustfmt clippy
```

The repository's `rust-toolchain.toml` selects Rust 1.87 automatically inside the checkout.

Run the GUI during development:

```bash
cargo run -p orbis-ui --bin orbis-control
```

Build the release workspace:

```bash
cargo build --workspace --release --locked
```

Install the current checkout as a local Arch system integration build:

```bash
bash packaging/install-arch.sh
```

This installs the four production binaries under `/usr/local/bin`, the system/user systemd units, D-Bus policy, polkit actions and desktop/AppStream metadata. It is a local developer installer, not a pacman-owned release package.

To update only the privileged helper during development:

```bash
bash packaging/deploy-dev-hardwared.sh
```

## Verification

Use real build/test checks rather than documentation/source-marker contracts:

```bash
scripts/verify crate orbis-ui   # targeted crate while iterating
scripts/verify quick            # fmt + workspace check
scripts/verify task             # fmt + check + tests + clippy
scripts/verify full             # task checks + release build + packaging asset validation
```

Do not run the full suite after every small edit. Implement a coherent batch, run targeted checks, fix failures, and use broader verification at the end of the vertical slice.

CI uses the pinned Rust toolchain and normal Linux system packages; it does not use Nix.

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
Comparative project decisions are recorded in [`docs/research/comparative-projects.md`](docs/research/comparative-projects.md); they guide ownership and state semantics but do not authorize copying external code or enabling unsupported mutations.

## Repository workflow for AI agents

[`AGENTS.md`](AGENTS.md) is intentionally product-first: agents are expected to complete coherent vertical work, continue across necessary crates, and stop only at a real blocker. Documentation maintenance and source-marker test generation are not default development work.

When asked simply to continue or finish the project, start from [`TODO.md`](TODO.md), verify the actual source/runtime state, complete the highest-priority actionable slice, and continue to the next related item instead of stopping after a micro-fix.

## License

GPL-3.0-or-later.

Orbis Control is an independent project and is not affiliated with ASUSTeK Computer Inc.
