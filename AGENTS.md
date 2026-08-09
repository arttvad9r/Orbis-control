# AGENTS.md — Orbis Control

Permanent working contract for AI agents in this repository. Task-specific
instructions take priority; when they conflict, choose the safer and narrower
action. Do not continue to the next architectural task automatically.

## Project scope

- Orbis Control: lightweight system application for ASUS laptops on Linux
  (G-Helper-like), written in Rust with a Slint GUI.
- Wayland-first; X11 is a compatibility mode.
- GUI must not run as root and must not perform direct hardware I/O.
- All system interaction lives behind provider/service boundaries; the GUI
  talks to providers and services through application/provider abstractions.
- Architecture is capability-driven: capabilities are probed, not assumed.
- Hardware-specific code must not leak into the UI or domain layer.

## Workspace / crate structure

Rust workspace (`resolver = 3`, edition 2024, MSRV 1.85). Crates:

- `orbis-core` — domain types and their invariants.
- `orbis-config` — configuration.
- `orbis-capabilities` — capability probing/detection.
- `orbis-providers` — provider traits and mock backends.
- `orbis-application` — application layer.
- `orbis-sessiond` — user-session D-Bus daemon.
- `orbis-session-protocol` — D-Bus protocol DTOs.
- `orbis-session-client` — session client.
- `orbis-ui` — Slint UI (src + `ui/` slint files).
- `orbis-cli` — CLI.
- `orbis-test-support` — test support.
- `orbis-hardwared` — deliberately NOT in the workspace (ADR 0002).

## Architectural boundaries

- `orbis-core` owns domain types and invariants.
- Provider traits separate domain/application from concrete backends.
- Session D-Bus protocol, client and daemon stay separate crates; the
  application layer does not depend on the daemon.
- The user-session daemon must not gain root without proven need; a separate
  privileged hardware daemon is only considered after a concrete operation is
  confirmed to require privileges (ADR 0002).
- D-Bus DTOs are untrusted input, validated at the wire/domain boundary.
- Use shared constants for bus names, object paths, interface names; don't
  duplicate them.
- Authoritative reads must not be hidden by an implicit property cache; values
  that must stay fresh use explicit no-cache semantics, and that is tested.
- GPU MUX, GPU access policy, GPU power state and GPU requirement are separate
  capabilities; don't collapse them into one value.

## Safety defaults

- Hardware writes, sysfs writes, privileged commands and mutation D-Bus methods
  are forbidden without explicit permission from the specific task.
- Read-only capability must never be presented as write capability.
- Unsupported mutations return `Unsupported` honestly; never simulate success.
- Do not use `sudo`. Do not run `systemctl`, `busctl`, real UPower/asusd/
  supergfxd, an external D-Bus daemon, Docker, Podman or a VM without a direct
  instruction. Do not touch the real system/session bus in tests; use private
  P2P transport for D-Bus integration tests by default.
- Do not add `unsafe`; keep existing `forbid`/`deny unsafe_code` lints. Do not
  weaken lint policy or tests to make a check pass.

## Scope discipline

- Before making changes, check `git status --short`.
- Change only files explicitly allowed by the task prompt. No incidental
  refactoring; do not format or rewrite unrelated files.
- Do not change public API without explicit permission.
- Do not add dependencies without a clear need and permission. Do not change
  `Cargo.lock` unless it is the mandatory result of legitimate resolution.
- If a task requires a forbidden file or API change, stop and report the exact
  reason. Do not bypass the production path with a test-only shortcut.

## Build / check / test commands

Verification tiers: выбирай минимальный tier, который реально покрывает
изменение. Не ослабляй correctness/security tests — tier выбирается по охвату,
а не для экономии.

### FAST — default для обычного изменения одного crate

```bash
cargo fmt --all -- --check
cargo check -p <affected-crate> --all-targets
cargo test -p <affected-crate>
cargo clippy -p <affected-crate> --all-targets -- -D warnings
git diff --check
```

### INTEGRATION — изменение пересекает несколько crates / service boundaries

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

### FULL — только для milestone/release acceptance, Nix/package/module изменений,
перед контролируемой live hardware mutation, или когда task явно требует

INTEGRATION + (при необходимости) тяжёлые Nix commands:

```bash
nix build .#orbis-control --max-jobs 1 --cores 4
nix flake check --max-jobs 1 --cores 4
```

### Docs-only

```bash
git diff --check
# + релевантный rg по затронутым docs
```

Никаких cargo/nix checks без отдельной причины.

### Notes

- Для тяжёлых Nix commands всегда использовать `--max-jobs 1 --cores 4`
  (предотвращает OOM / SIGKILL 137 на рабочих машинах).
- Интеграционные тесты, которые могут deadlock во время handshake, обязаны
  использовать bounded timeout; timeout не заменяет корректную обработку
  lifecycle.
- Не используй `sleep`, polling или retry для маскировки races/deadlocks.
- Когда важна cache semantics, проверяй свежие authoritative reads минимум
  двумя последовательными различающимися значениями. Ассертируй класс
  ошибки, не только факт ошибки. Ассертируй счётчики/порядок вызовов, если
  short-circuit важен.

## Git policy

- Never use `git add .`. Stage only an explicit list of files.
- Do not create a commit unless the task prompt explicitly asks for it.
- Do not amend, rebase, reset, force push or remove others' changes without
  direct permission.
- After a commit, verify the working tree is clean and the commit contains
  exactly the intended files.
- Commit messages: `<scope>: <imperative summary>` (scopes like `ui:`,
  `session:`, `sessiond:`).

## Known project gotchas

- `orbis-hardwared` exists in `crates/` but is NOT in the workspace — do not
  add it to members without an ADR decision.
- Slint UI files live in `ui/`; Rust glue in `crates/orbis-ui/src`. Theme tokens
  live in `ui/themes/dark.slint`. See `docs/ui-reference.md` and
  `docs/ui-measurements.json`.
- Dev environment: `nix develop` provides cargo/rustc/rustfmt/clippy and
  headless-Slint test env. Do not install a global Rust toolchain.
- Provider implementations for real sysfs/asusd are scheduled for later stages;
  `orbis-providers` currently has traits + mock only.

## Reporting

Keep the final report short:

1. Changed files. 2. What was implemented. 3. Key architectural invariants.
4. Tests added or changed. 5. Check results. 6. `git status --short`.
7. Whether a commit was created. 8. What was intentionally left out of scope.

A detailed report is only required for: diagnostic stops, public API changes,
protocol/ABI changes, privileged or hardware operations, real system/session
bus bootstrap, and migration or dependency-resolution issues.

## Skills

Project-specific procedures live in `.opencode/skills/`:
`orbis-slint-ui` and `orbis-hardware-safety`. Load one only when the task
matches its scope; ordinary Rust work follows this file directly.
