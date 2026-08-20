# AGENTS.md — Orbis Control

Permanent working contract for AI agents in this repository. Task-specific instructions take priority; when instructions are ambiguous, choose the safer and narrower action. Current project status comes from `docs/current-state.md`; historical plans do not override current source.

## Project scope

- Orbis Control is a lightweight Linux system application for ASUS ROG/TUF/Zephyrus laptops, written in Rust with a Slint GUI.
- Wayland-first; X11 is a compatibility path where the platform can actually provide the requested behavior.
- GUI is a normal user-session application. It must not perform direct privileged hardware I/O; interactive euid-0 execution must be rejected before release (#125).
- Read/session interaction and privileged mutation are separate boundaries.
- Proven mutations preserve original application caller identity through the narrow `Hardware1` system-bus API and capability-specific polkit.
- Architecture is capability/evidence-driven: support is probed, not assumed from model names.
- Hardware-specific implementation details must not leak into UI/domain policy as implicit support claims.

## Workspace / crate structure

Rust workspace: resolver 3, edition 2024, MSRV **1.87**.

- `orbis-core` — domain types and invariants.
- `orbis-config` — hardened XDG preferences/state/desired-state foundations.
- `orbis-capabilities` — capability evidence and immutable registry snapshots.
- `orbis-providers` — provider traits and production/test implementations.
- `orbis-application` — application services/use cases/read-back composition.
- `orbis-sessiond` — unprivileged user-session D-Bus daemon/read boundary.
- `orbis-session-protocol` — Session1 wire DTOs/contracts.
- `orbis-session-client` — Session1 providers and caller-preserving Hardware1 client composition.
- `orbis-ui` — Slint UI, UI runtime, production composition and worker integration.
- `orbis-cli` — read-only CLI; `status` and versioned `status --json` exist. Do not add mutation commands without a separate explicit design/task.
- `orbis-test-support` — fixtures/screenshots/tests; normal GUI release dependency cleanup is tracked by #115.
- `orbis-hardwared` — narrow privileged Hardware1 system-bus daemon.

The active production worker implementation is `crates/orbis-ui/src/worker_runtime.rs`. Legacy large worker files are not the source of truth for new runtime fixes.

## Architectural boundaries

- `orbis-core` owns domain types and invariants.
- Provider traits separate domain/application from concrete backends.
- Session protocol/client/daemon remain separate crates; `orbis-sessiond` must never become a privileged mutation deputy.
- Proven mutation flow is: original application caller → Hardware1 → polkit (`system-bus-name`) → narrow typed backend → authoritative read-back / explicit Pending / fail-closed error.
- `orbis-hardwared` exposes only semantic typed mutations. Never add a generic sysfs/filesystem/shell/D-Bus proxy or caller-provided path/command execution.
- D-Bus DTOs are untrusted input and are validated at wire/domain boundaries.
- Authoritative reads that must remain fresh use explicit no-cache/read semantics.
- Read and write evidence are independent. UI writability is derived from operation-level/equivalent typed write evidence, not from overall capability status, model name or implementation presence.
- GPU product policy, physical MUX, access policy, runtime power and pending/action requirement are separate concepts.
- `Unsupported`, `BackendMissing`, `TemporarilyUnavailable`, `PermissionDenied`, `ReadOnly` and `Unknown` are not interchangeable.
- Desired, Observed and Pending are independent. Loading persisted/default config must never itself apply hardware state.
- `ApplyResult::Accepted` is not `Applied`.
- Mutation timeout after a request may have been dispatched is an **unknown outcome**; do not blindly retry it.

## Safety defaults

- Hardware/sysfs writes, privileged commands and mutation D-Bus calls are forbidden unless the task explicitly requires that exact mutation work and existing product/evidence gates permit it.
- Read-only capability must never be presented as write capability.
- Unsupported/blocked mutations must appear honestly; never simulate success.
- Do not use `sudo` or real system/session bus in automated tests without explicit task authorization.
- Do not use real UPower/asusd/supergfxd, Docker/Podman/VMs or broad ignored tests unless the task explicitly calls for controlled integration validation.
- VM/fake-system validation never proves physical ASUS hardware behavior.
- Fan curve writes/reset remain hard-blocked. Do not re-enable them until #104/#105/#109/#116 are resolved, exact-build executable checks pass and required controlled hardware validation is recorded.
- Raw/product GPU mutation, Panel/Keyboard/Aura product writes, unattended Automation, Display modeset and self-update remain independent promotion gates. Typed implementation code does not authorize them.
- Do not add `unsafe`; keep existing `forbid`/`deny unsafe_code` lints. Never weaken lint/tests/safety gates to make a check pass.

## Source-of-truth discipline

Before modifying behavior, read:

1. `docs/current-state.md`;
2. `docs/architecture.md`;
3. relevant ADR/provider/protocol document;
4. relevant open issue when one exists.

Use `docs/roadmap.md` for future ordering and `docs/beta-acceptance-checklist.md` for release gates. Dated audits/remediation plans are historical unless current docs explicitly promote them.

Source inspection proves at most `IMPLEMENTED`. `TESTED`, `PACKAGED` and `LIVE-VALIDATED` require their corresponding executed evidence from `docs/verification.md` / `docs/release-evidence-taxonomy.md`.

## Scope discipline

- Check `git status --short` before edits when a checkout is available.
- Do not perform incidental broad refactors unrelated to the active task.
- Public/protocol API changes require explicit justification and migration/compatibility consideration.
- Add dependencies only for a clear architectural need; do not change `Cargo.lock` casually.
- If the required safe implementation cannot be validated or patched reliably in the available environment, leave the product path fail-closed and document the exact blocker rather than inventing a shortcut.

## Development environment

- Canonical environment is the flake `devShell`; `.envrc` must remain exactly `use flake`.
- Rust/Slint/native tools come from the project devShell, not an ad-hoc global toolchain.
- `.direnv/` is local cache state and is not committed.
- If the environment lacks Rust/Cargo/Slint/Nix, report executable checks as `BLOCKED`/`NOT_RUN`; do not infer PASS from source/static checks.

## Verification

Always run the smallest tier that actually covers the change, but repository/release claims remain lockfile-strict.

### Layer 0 — source/static contracts

No Rust/Nix required:

```bash
python3 scripts/verify-static
```

This is a fail-fast source check only. It does not prove Rust/Slint/Nix compilation or runtime behavior.

### FAST — narrow crate change

```bash
python3 scripts/verify-static
cargo fmt --all
cargo check -p <affected-crate> --all-targets --locked
cargo test -p <affected-crate> --locked
cargo clippy -p <affected-crate> --all-targets --locked -- -D warnings
git diff --check
```

### INTEGRATION — cross-crate/protocol/runtime change

```bash
python3 scripts/verify-static
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### FULL — release/package/system-service/polkit/D-Bus change

INTEGRATION plus targeted Nix checks and final:

```bash
nix flake check --max-jobs 1 --cores 4
```

Current system-integration checks include:

```text
checks.x86_64-linux.hardwared-lifecycle
checks.x86_64-linux.performance-mutation-vm
checks.x86_64-linux.battery-mutation-vm
```

Automated integration uses fake/VM state and must not mutate real ASUS hardware.

### Docs/repository-only

```bash
python3 scripts/verify-static
git diff --check
```

Use additional targeted standard-library/shell syntax checks when relevant.

### Verification notes

- Heavy Nix commands use `--max-jobs 1 --cores 4`.
- Async/integration operations that can hang need bounded timeouts; do not use `sleep`/retry to mask races.
- Provider read timeouts must preserve local evidence classes. Generic timeout helpers must not automatically retry mutations.
- When cache semantics matter, verify distinct sequential authoritative values.
- Assert error classes and call ordering/counts where short-circuit/lifecycle correctness matters.
- A GitHub Actions run object is not verification evidence by itself. A job that fails before Checkout, has `steps=null`/empty steps or no executable logs is an infrastructure blocker, not a repository test result (#106).

## Git / repository policy

- Never use `git add .`; stage explicit files when working from a checkout.
- Do not amend/rebase/reset/force-push/delete others' unique work without explicit authorization.
- Integration/cleanup tasks may create commits/PRs when the user has authorized repository maintenance; record exactly what was changed.
- Prefer Draft PRs for large unvalidated integration lines. Do not merge them merely because GitHub reports `mergeable=true`.
- Commit messages: `<scope>: <imperative summary>` (`ui:`, `session:`, `hardwared:`, `docs:`, `verify:`, `repo:`).
- `main` required checks remain deferred until #106 is actually fixed; see #114.
- Obsolete remote branch cleanup is tracked by #118. Do not bulk-delete refs with unique commits without confirming they are superseded or integrated.

## Known project gotchas

- `orbis-hardwared` is a workspace member; every new mutation requires typed semantics, ownership, authorization and read-back evidence.
- Slint files live under `ui/`; Rust UI glue/runtime lives under `crates/orbis-ui/src`.
- Production mock fallback is forbidden. The remaining normal `orbis-test-support` GUI dependency is a bootstrap/dependency-graph defect tracked by #115, not permission to use fixtures as hardware state.
- Fan curve sysfs values are raw PWM `0..255`, not percentages.
- Stored asusd fan curve points, their `enabled` state and the active sysfs curve are distinct evidence concepts.
- Per-fan profile reads exist, but the aggregate FanCurves capability still has a CPU→GPU inference gap (#109).
- GPU product modes Eco/Standard/Ultimate/Optimized are product policies, not raw supergfxd aliases.
- Legacy `AppConfig`/path helpers are compatibility-only and must not become reconciliation/new-persistence foundations (#113).
- Telemetry `Ok` is not automatically useful/fresh evidence; coverage semantics remain #117.
- Support-matrix artifacts are documentation/release evidence only and must never become model-name runtime support inference.

## Reporting

Final engineering reports should state:

1. files/issues/PRs changed;
2. implemented behavior;
3. preserved safety/architecture invariants;
4. tests/contracts added or changed;
5. **executed** check results separately from source/static inspection;
6. repository/branch/PR state;
7. commits/merges created;
8. intentionally blocked work and exact reasons.

Detailed reporting is required for public/protocol API changes, privileged behavior, hardware operations, dependency resolution, migrations and diagnostic stops.

## Project skills

Project-specific procedures live in `.opencode/skills/`:

- `orbis-slint-ui`;
- `orbis-hardware-safety`;
- `orbis-system-integration`.

Load the matching procedure when the task enters that scope; hardware-safety guidance is mandatory for changes that affect real hardware semantics.
