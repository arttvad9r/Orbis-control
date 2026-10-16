# Orbis Control

Rust + Slint Linux application for ASUS ROG/TUF/Zephyrus laptops. Goal: ship complete user-visible behavior, not plans or isolated internal patches.

## Work queue and sources
- `PLAN.md` is the only execution queue. For broad requests such as continue/finish, start at the earliest actionable unchecked phase and keep working through a coherent vertical slice.
- Current behavior source order: production code → executable tests/build config → `PLAN.md` → relevant current issues → README/ADR/stable docs.
- Do not create parallel roadmaps, status matrices, remediation logs or evidence dumps unless explicitly requested.
- Historical branches/documents are background unless the task says otherwise.

## Development
- Canonical environment: Arch Linux, native Rust/Cargo and normal Linux systemd/D-Bus/polkit assets. Do not introduce Nix infrastructure.
- Cross-crate changes are expected when required for a complete user-visible flow.
- Project map: `orbis-core` domain; `orbis-config` persistence; `orbis-capabilities`; `orbis-providers`; `orbis-application`; session protocol/daemon/client; privileged `orbis-hardwared`; `orbis-ui`; `orbis-cli`; `orbis-test-support`.
- Active UI worker: `crates/orbis-ui/src/worker_runtime.rs`; Slint under `ui/`.

## Hardware safety invariants
- GUI is unprivileged. Hardware mutation belongs only behind the narrow typed `Hardware1` system-bus service with capability-specific authorization.
- Never add generic privileged filesystem/sysfs/shell/D-Bus proxies or caller-provided command/path execution.
- Capability support comes from runtime evidence, not laptop model guesses; read and write support are independent.
- Desired, Observed and Pending are distinct. Loading configuration must not mutate hardware. `ApplyResult::Accepted` does not mean `Applied`.
- Mutation timeout after possible dispatch is an unknown outcome: never blindly retry or simulate success. Unsupported/blocked mutations fail honestly.
- Keep unsafe Rust forbidden unless explicitly requested and reviewed.
- Execute real hardware writes/privileged mutation only when the user's task explicitly authorizes that exact live-hardware action. Fakes/mocks/private D-Bus peers remain valid for software development.

## Verification
- Targeted: `scripts/verify crate <affected-crate>`; quick: `scripts/verify quick`; task gate: `scripts/verify task`; release-oriented: `scripts/verify full`.
- Do not weaken safety tests/lints or add tests that merely restate implementation details.

- Project-specific reference skills: `.opencode/skills/orbis-slint-ui`, `.opencode/skills/orbis-hardware-safety`, `.opencode/skills/orbis-system-integration`.

## Git
- `main` holds released code only; every merge into `main` is a release tagged `vX.Y.Z`.
- `dev` is the integration branch and must always build and pass checks.
- Work in `feat/<name>` / `fix/<name>` from `dev`; several branches may run in parallel. Merge verified work into `dev`, then delete the branch.
