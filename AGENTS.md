# AGENTS.md — Orbis Control

Permanent working contract for AI agents in this repository. Task prompts may
then carry only: the goal, the allowed files, step-specific invariants, required
tests, and the commit/stop condition.

Task-specific instructions always take priority over these defaults. When they
conflict, choose the safer and narrower action. Do not continue to the next
architectural task automatically after finishing one.

## Project scope

- The project is Orbis Control: a lightweight system application for ASUS
  laptops on Linux (G-Helper-like), written in Rust with a Slint GUI.
- The platform is Wayland-first; X11 is a compatibility mode.
- The GUI must not run as root and must not perform direct hardware I/O.
- All system interaction lives behind provider/service boundaries; the GUI
  talks to providers and services through application/provider abstractions.
- The architecture is capability-driven: capabilities are probed, not assumed.
- Hardware-specific code must not leak into the UI or domain layer.

## Architectural boundaries

- `orbis-core` owns domain types and their invariants.
- Provider traits separate domain/application from concrete backends.
- Session D-Bus protocol, session client and session daemon stay separate
  crates; the application layer does not depend on the daemon.
- The user-session daemon must not gain root without proven need; a separate
  privileged hardware daemon is only considered after a concrete operation is
  confirmed to require privileges (ADR 0002).
- D-Bus DTOs are untrusted input and are validated at the wire/domain boundary.
- Do not duplicate protocol bus names, object paths or interface names when
  shared constants already exist; use them.
- Authoritative reads must not be hidden by an implicit property cache. Values
  that must stay fresh use explicit no-cache semantics, and that is tested.
- GPU MUX, GPU access policy, GPU power state and GPU requirement are separate
  capabilities and must not be collapsed into one value.

## Safety defaults

- Hardware writes, sysfs writes, privileged commands and mutation D-Bus methods
  are forbidden without explicit permission from the specific task.
- Read-only capability must never be presented as write capability.
- Unsupported mutations return `Unsupported` honestly; never simulate success.
- Do not use `sudo`.
- Do not run `systemctl`, `busctl`, the real UPower/asusd/supergfxd, an external
  D-Bus daemon, Docker, Podman or a VM without a direct instruction.
- Do not touch the real system/session bus in tests unless the task explicitly
  requires it; use private P2P transport for D-Bus integration tests by default.
- Do not add `unsafe`; keep existing `forbid`/`deny unsafe_code` lints.
- Do not weaken lint policy or tests to make a check pass.

## Scope discipline

- Before making changes, check `git status --short`.
- Change only files explicitly allowed by the task prompt.
- Do not do incidental refactoring; do not format or rewrite unrelated files.
- Do not change public API without explicit permission.
- Do not add dependencies without a clear need and permission.
- Do not change `Cargo.lock` unless it is the mandatory result of a legitimate
  Cargo resolution.
- If a task requires a forbidden file or API change, stop and report the exact
  reason.
- Do not bypass the production path with a convenient test-only shortcut; test
  a production helper through its public entry point.
- Do not duplicate an existing source/provider/service composition.

## Diagnostic stop

Stop without further fixes when:

- task invariants contradict current production behavior;
- the required public API does not exist;
- a file outside the allowed list must change;
- the working tree contains unrelated changes;
- a test exposes an architectural defect but production changes are not allowed;
- the safe semantics of a hardware/system operation are unknown.

The diagnostic report must include:

- the exact stage of failure;
- the actual error variant or observed behavior;
- whether the call reached the intended layer;
- the minimal root cause;
- at most two candidate future fixes (not implemented);
- confirmation that forbidden changes and commits were not made.

## Testing policy

Base checks for Rust code changes:

```
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Additional rules:

- Run the narrow test target of the changed crate first, then workspace checks.
- Integration tests that can deadlock during handshake must use a bounded
  timeout; the timeout is not a substitute for correct lifecycle handling.
- Do not use `sleep`, polling or retry to mask races or deadlocks.
- When cache semantics matter, verify fresh authoritative reads with at least
  two sequential differing values.
- Assert the error class, not only that an error occurred.
- Assert call counts/order when short-circuit behavior matters.
- Docs-only changes do not require the full Cargo test suite unless the task
  prompt says otherwise: verifying the diff and Markdown content is enough.

## Git policy

- Never use `git add .`.
- Before staging, check `git status`, `git diff --stat` and the specific diffs.
- Stage only an explicit list of files.
- Before committing, check `git diff --cached --check` and
  `git diff --cached --name-only`.
- Do not create a commit unless the task prompt explicitly asks for it.
- Do not amend, rebase, reset, force push or remove others' changes without
  direct permission.
- After a commit, verify the working tree is clean and the commit contains
  exactly the intended files.

## Commit messages

Use the existing style:

```
<scope>: <imperative summary>
```

Example scopes: `ui:`, `session:`, `sessiond:`. Do not invent future commit
messages here; write the message that fits the task being committed.

## Agent execution style

- Each task is one bounded micro-step; aim for at most 30 meaningful
  tool/edit/check steps.
- Study only the minimal code needed, then make the minimal diff.
- Do not automatically start the next architectural task.
- Prefer the safer and narrower action on conflict.

## Reporting

Keep the final report short:

1. Changed files.
2. What was implemented.
3. Key architectural invariants.
4. Tests added or changed.
5. Check results.
6. `git status --short`.
7. Whether a commit was created.
8. What was intentionally left out of scope.

Do not repeat dozens of negative checks that are already fixed by this file.
A detailed report is only required for: diagnostic stops, public API changes,
protocol/ABI changes, privileged or hardware operations, real system/session
bus bootstrap, and migration or dependency-resolution issues.
