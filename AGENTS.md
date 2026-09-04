# AGENTS.md — Orbis Control

This file is the permanent execution contract for AI agents working on Orbis Control.

## Primary objective

**Ship a working application.** Prefer finished user-visible behavior over plans, reports, document maintenance, speculative abstractions, or tiny isolated patches.

When the user asks to continue development, finish the project, fix a feature, or make the application work, treat that as authorization to complete the whole coherent work packet across every required layer. Do not stop after the first successful edit, test, refactor, or subtask.

A normal work cycle is:

1. inspect the relevant source and current failures;
2. choose the next coherent vertical slice that moves the product toward working state;
3. implement it across all necessary crates/UI/runtime boundaries;
4. run targeted checks after substantial implementation, not after every tiny edit;
5. fix failures and continue;
6. run broader verification once the slice is complete;
7. continue to the next directly related unfinished slice when the user's goal still requires it.

Do not end a session merely because one intermediate task passed. End at a meaningful product checkpoint or a real blocker.

## Canonical completion plan

For general project-completion work, [`FINISH_PLAN.md`](FINISH_PLAN.md) is the **single canonical execution queue**.

When the user says `continue`, `finish`, `make it work`, or gives an equivalent broad instruction:

1. read `FINISH_PLAN.md` before choosing work;
2. start at the earliest unchecked phase that can be advanced in the current environment;
3. verify items that may already be complete in current source, mark them complete, and continue rather than stopping with a status report;
4. work through coherent vertical slices until reaching a meaningful checkpoint or a real blocker;
5. update plan checkboxes only after the stated behavior has actually been verified.

Do not create a parallel roadmap, remediation plan, status matrix, or alternative backlog for the same completion goal. `TODO.md` is only a pointer to `FINISH_PLAN.md`.

A specific user request always overrides queue order for that requested task. After completing the explicit task, return to the canonical finish plan when the user's broader goal is still to finish the project.

## When to stop

Stop and ask the user only when progress genuinely requires information or authority that cannot be inferred safely, for example:

- a credential, secret, account access, external file, or device that is unavailable;
- a destructive or irreversible real-world action that was not authorized;
- a product decision with multiple materially different outcomes and no defensible default;
- physical hardware validation required to make a live-hardware claim.

The following are **not** reasons to stop:

- one test passed;
- one file was fixed;
- another crate also needs changes;
- the implementation is larger than expected;
- documentation is stale;
- a nonessential check/tool is unavailable;
- a safe implementation can be completed without live hardware.

If a check is unavailable, use the strongest available alternative, state the limitation at the end, and keep implementing work that does not depend on that check.

## Development platform

The canonical developer environment is **Arch Linux** with the native Rust toolchain and system packages documented in `README.md`.

- Do not introduce Nix, NixOS modules, flakes, derivations or Nix-only verification as project dependencies.
- Use Cargo for Rust build/test/lint work.
- Use normal Linux/systemd/D-Bus/polkit assets for system integration.
- Keep production code distribution-agnostic where practical; Arch is the primary development/package target, not an excuse to hard-code user-machine paths into domain code.
- If CI runs on another Linux distribution, treat it as a portability build environment only. It must exercise the same Cargo code and distro-neutral integration assets.

## Product-first rules

- Production code and working user flows have priority over documentation.
- Cross-crate and cross-layer changes are expected when required for a complete feature. Do not artificially constrain a vertical slice to one file or one crate.
- Refactor when it materially simplifies or enables the target implementation. Do not avoid a necessary refactor merely because it is broad.
- Remove dead, obsolete, duplicate, placeholder, or counterproductive code when doing so simplifies the product. Existing code is not sacred.
- Prefer established Rust/Slint/Linux APIs and existing project abstractions where they fit. Do not invent infrastructure solely to avoid touching existing boundaries.
- Do not create a new design document, remediation plan, audit, status matrix, evidence taxonomy, validation trigger, or verification script unless the user explicitly requested it or the implementation truly cannot be maintained safely without it.
- Do not update status/roadmap documents after every small change. Documentation changes should be a small final part of a completed product change, and only when the documentation would otherwise become materially false.
- Historical plans under `docs/` are reference material, not execution instructions and not a backlog.

## Source of truth

For current behavior, use this order:

1. production source code;
2. executable tests and build configuration;
3. `FINISH_PLAN.md` for the ordered completion queue;
4. current GitHub issues when they describe still-relevant defects or acceptance criteria;
5. `README.md` and stable architecture/ADR documentation for intentional public invariants;
6. other documents only as background.

Do **not** spend a development session reconciling documents with one another unless the user specifically asks for documentation work.

`main` is the canonical development branch. Do not treat `development`, old PR branches, dated plans, or historical snapshots as a newer source of truth unless the user explicitly directs you there.

## Definition of done for a work packet

A feature/fix is done when, as applicable:

- the production path is implemented end to end;
- UI state and controls are connected to real application/runtime behavior rather than placeholders;
- errors and unsupported capabilities are represented honestly;
- no known placeholder/TODO in the target flow prevents normal use;
- relevant regression tests exist where they provide real value;
- affected code builds/checks successfully with the available toolchain;
- the completed slice can be exercised by the user without additional agent-only setup.

A passing unit test for an internal helper is not sufficient if the user-visible flow is still disconnected.

## Verification strategy

Verification supports implementation; it must not replace implementation.

During active development:

- run the smallest useful targeted command after a meaningful batch of changes;
- fix failures immediately and continue;
- avoid repeatedly running the entire workspace for tiny edits;
- do not add tests that merely restate implementation details or inflate coverage without protecting behavior;
- do not add source-marker contract scripts when an ordinary Rust/Slint/integration test can verify behavior;
- run broad workspace verification once a coherent slice is complete or before release-oriented work.

Use the repository runner when convenient:

```bash
scripts/verify crate <affected-crate>  # targeted crate
scripts/verify quick                   # fmt + workspace check
scripts/verify task                    # fmt + check + tests + clippy
scripts/verify full                    # task checks + release build + packaging assets
```

Direct Cargo commands are also fine. Do not repeatedly run `scripts/verify full` while still making small edits.

Do not weaken existing safety-critical tests or lints merely to get green output. Fix the underlying problem.

## Architecture and safety invariants

Orbis Control is a Rust + Slint Linux system application for ASUS ROG/TUF/Zephyrus laptops. Preserve these real safety boundaries while simplifying everything else as needed:

- GUI runs unprivileged; privileged hardware mutation belongs behind the narrow typed `Hardware1` system-bus service and capability-specific authorization.
- `orbis-sessiond` is a user-session/read boundary and must not become a privileged mutation deputy.
- Never add a generic privileged filesystem/sysfs/shell/D-Bus proxy or caller-provided command/path execution surface.
- Capability support is based on runtime evidence, not laptop model-name guesses.
- Read support and write support are independent.
- Desired, Observed and Pending are distinct states; loading configuration must not itself mutate hardware.
- `ApplyResult::Accepted` is not `Applied`.
- A mutation timeout after possible dispatch is an unknown outcome; never blindly retry it.
- Unsupported or blocked mutations must fail honestly; never simulate success.
- Keep unsafe Rust forbidden unless the user explicitly requests a reviewed exception and there is no safe alternative.

Real hardware writes, privileged commands, and mutation calls must only be executed when the user's task explicitly authorizes that exact real-hardware work. **This restriction does not prevent implementing, refactoring, compiling, or testing the software path with fakes, mocks, private D-Bus peers or other non-mutating test environments.** Lack of physical hardware blocks live-validation claims, not ordinary software development.

## Project map

Rust workspace, edition 2024, MSRV 1.87:

- `orbis-core` — domain types and invariants
- `orbis-config` — preferences/state/desired-state persistence
- `orbis-capabilities` — capability evidence and snapshots
- `orbis-providers` — provider traits and concrete backends
- `orbis-application` — application services/use cases
- `orbis-sessiond` / `orbis-session-protocol` / `orbis-session-client` — user-session boundary and protocol
- `orbis-hardwared` — narrow privileged mutation daemon
- `orbis-ui` — Slint UI, runtime, production composition
- `orbis-cli` — CLI
- `orbis-test-support` — dev/test fixtures only

The active UI worker implementation is `crates/orbis-ui/src/worker_runtime.rs`. Slint files live under `ui/`.

## Working style

- Inspect before editing, but do not spend the session only inspecting.
- Make changes in substantial coherent batches.
- Prefer one finished vertical slice over many tiny "safe" edits.
- Do not create separate commits for implementation, tests, and docs unless there is a real review reason. A coherent feature may be one normal commit.
- Do not produce progress reports after every microstep. User-facing reporting belongs at meaningful checkpoints and should focus on what now works, what was verified, and any genuine blockers.
- If you find obsolete project bureaucracy that actively obstructs development, simplify or remove it as part of the work rather than preserving it by default.

## Project-specific skills

Optional procedures live in `.opencode/skills/`:

- `orbis-slint-ui` — use for substantial Slint/UI work when useful;
- `orbis-hardware-safety` — use when changing real hardware semantics;
- `orbis-system-integration` — use for system service/polkit/D-Bus integration work.

These skills supplement this contract. They do not override the primary objective to complete the coherent user-requested work packet.
