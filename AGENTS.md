# AGENTS.md — Orbis Control

This file is the permanent execution contract for AI agents working on Orbis Control.

## Primary objective

**Ship a working application.** Prefer finished user-visible behavior over plans, reports, document maintenance, speculative abstractions, or tiny isolated patches.

When the user asks to continue, finish, make something work, or gives an equivalent broad instruction, complete the whole coherent work packet across every required layer. Do not stop after the first edit, test, refactor, or subtask if the user-visible flow is still unfinished.

## Canonical completion queue

[`TODO.md`](TODO.md) is the **single project-completion queue**.

For broad completion work:

1. read `TODO.md`;
2. start with the earliest incomplete top-level block that can actually be advanced;
3. inspect production source before assuming the plan is current;
4. implement coherent vertical slices across UI/runtime/provider/daemon boundaries as needed;
5. run targeted checks after meaningful batches and broader checks when the slice is complete;
6. continue until a product-sized checkpoint or a real blocker is reached.

Do not create a parallel roadmap, remediation plan, status matrix, evidence ledger, or second backlog. Do not turn TODO sub-bullets into dozens of checkboxes. Update a top-level TODO checkbox only when its stated exit condition is genuinely satisfied.

A specific user request overrides TODO order. After completing it, return to the canonical queue when the broader goal is still to finish the product.

## Product-surface rule

The current intentional UI is a **product contract**, not a map of already implemented backend support.

- Do not remove, hide, simplify away, or permanently disable an intended control merely because its backend is unfinished.
- Complete the backend and state model behind the UI.
- If the actual machine lacks the capability, show an honest unsupported/read-only/unavailable state based on runtime evidence.
- An enabled user action must never be a placeholder, silent no-op, fake success, or fixture-backed production value.
- Planned product UI may exist before backend completion; that is not permission to pretend the operation already works.

## Definition of done

A feature/fix is done when, as applicable:

- the production path works end to end;
- UI state comes from real observation rather than guessed/default/mock state;
- writes use the correct ownership boundary and have honest applied/accepted/pending/error semantics;
- unsupported/degraded states are represented accurately;
- relevant regression tests protect real behavior;
- affected code builds/checks successfully;
- the user can exercise the completed slice without agent-only setup.

A passing unit test or a polished UI alone is not enough if the visible action is still disconnected.

## Architecture and safety invariants

Preserve these boundaries while simplifying everything else as needed:

- GUI runs unprivileged.
- Privileged hardware mutation belongs behind the narrow typed `Hardware1` system-bus service and capability-specific authorization.
- `orbis-sessiond` is a user-session/read boundary and must not become a privileged mutation deputy.
- Never add a generic privileged filesystem/sysfs/shell/D-Bus proxy or caller-provided command/path execution surface.
- Runtime evidence determines capability support; model names alone do not.
- Read support and write support are independent.
- Desired, observed and pending state are distinct; loading configuration must not itself mutate hardware.
- `ApplyResult::Accepted` is not `Applied`.
- A mutation timeout after possible dispatch is an unknown outcome; never blindly retry it.
- Unsupported or blocked mutations fail honestly; never simulate success.
- Keep unsafe Rust forbidden unless there is an explicitly reviewed necessity.

Real hardware writes, privileged commands and mutation calls require explicit user authorization for that real-hardware validation session. This restriction does not prevent implementing, refactoring, compiling or testing the software path with fakes, mocks or private D-Bus peers.

## Development and verification

Primary development platform: **Arch Linux**. Production code should stay distribution-agnostic where practical.

Do not introduce Nix/NixOS as a project build dependency. Historical NixOS evidence may remain reference material, but current release claims require current release validation.

Use the smallest useful check during implementation and the broader suite at product checkpoints:

```bash
scripts/verify crate <affected-crate>
scripts/verify quick
scripts/verify task
scripts/verify full
```

Direct Cargo commands are fine. Do not repeatedly run `scripts/verify full` after tiny edits. Do not weaken safety-critical tests or lints merely to get green output.

## When to stop

Stop for the user only when progress genuinely requires something unavailable or an explicit real-world decision, for example:

- credentials/secrets/account access;
- a required external file or physical device;
- an irreversible/destructive action not authorized;
- a materially different product decision with no defensible default;
- explicit authorization for live hardware mutation or release/merge/tag publication.

Do **not** stop merely because another crate/file also needs changes, one test passed, the implementation is broad, documentation is stale, or a safe software path can be completed without live hardware.

## Source of truth

For current behavior use, in order:

1. production source code;
2. executable tests/build configuration;
3. `TODO.md` for remaining product work;
4. current GitHub issues when still relevant;
5. stable architecture/product/ADR documentation;
6. historical plans/evidence only as background.

Do not spend a development session reconciling documents unless the user specifically asks for documentation work or a materially false instruction is blocking implementation.

## Project map

- `orbis-core` — domain types and invariants
- `orbis-config` — preferences/state persistence
- `orbis-capabilities` — capability evidence
- `orbis-providers` — provider traits and concrete read/write backends
- `orbis-application` — application services/use cases
- `orbis-sessiond` / protocol / client — user-session boundary
- `orbis-hardwared` — privileged typed mutation daemon
- `orbis-ui` — Slint UI and production composition
- `orbis-cli` — CLI
- `orbis-test-support` — test fixtures only

The active UI worker path is under `crates/orbis-ui/src/worker_runtime.rs`; Slint files live under `ui/`.

Optional project skills under `.opencode/skills/` may be used for substantial Slint, hardware-safety or system-integration work. They supplement this contract; they do not replace the goal of completing the coherent user-requested product work.
