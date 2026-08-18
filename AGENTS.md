# AGENTS.md — Orbis Control

Permanent working contract for AI agents in this repository. Task-specific
instructions take priority; when they conflict, choose the safer and narrower
action. Do not continue to the next architectural task automatically.

## Project scope

- Orbis Control: lightweight system application for ASUS laptops on Linux
  (G-Helper-like), written in Rust with a Slint GUI.
- Wayland-first; X11 is a compatibility mode.
- GUI must not run as root and must not perform direct privileged hardware I/O.
- Read-only/session interaction and privileged mutation are separate boundaries:
  the GUI uses session/client providers for reads and calls the narrow Hardware1
  system-bus API directly for proven mutations so the original caller identity
  reaches polkit.
- Architecture is capability-driven: capabilities are probed, not assumed.
- Hardware-specific code must not leak into the UI or domain layer.

## Workspace / crate structure

Rust workspace (`resolver = 3`, edition 2024, MSRV 1.85). Crates:

- `orbis-core` — domain types and their invariants.
- `orbis-config` — configuration.
- `orbis-capabilities` — capability probing/detection.
- `orbis-providers` — provider traits plus real/read-only and compatibility backends.
- `orbis-application` — application layer.
- `orbis-sessiond` — unprivileged user-session D-Bus daemon/read boundary.
- `orbis-session-protocol` — D-Bus protocol DTOs.
- `orbis-session-client` — session + Hardware1 client/provider composition.
- `orbis-ui` — Slint UI (src + `ui/` slint files).
- `orbis-cli` — CLI.
- `orbis-test-support` — test support.
- `orbis-hardwared` — narrow privileged Hardware1 system-bus daemon, in the
  workspace since ADR 0006 proved the first privileged mutation boundary.

## Architectural boundaries

- `orbis-core` owns domain types and invariants.
- Provider traits separate domain/application from concrete backends.
- Session D-Bus protocol, client and daemon stay separate crates; the
  application layer does not depend on the daemon implementation.
- `orbis-sessiond` remains unprivileged and must never become a mutation deputy.
  Proven mutations follow original application caller → Hardware1 → polkit
  (`system-bus-name`) → bounded backend.
- `orbis-hardwared` exposes only semantic typed mutations. It must never become
  a generic sysfs/filesystem/shell/D-Bus proxy.
- D-Bus DTOs are untrusted input, validated at the wire/domain boundary.
- Use shared constants for bus names, object paths, interface names; don't
  duplicate them.
- Authoritative reads must not be hidden by an implicit property cache; values
  that must stay fresh use explicit no-cache semantics, and that is tested.
- GPU product policy, physical MUX, access policy, runtime power state and
  pending/action requirement are separate concepts; don't collapse them.
- Unknown/Unsupported/Unavailable/ReadOnly are distinct evidence states. Never
  silently convert insufficient evidence into a known state.
- asusd/supergfxd compatibility backends are used only where their typed
  semantics have been evidenced. Prefer standard kernel ABI for reads/control
  when ownership and semantics are proven.

## Safety defaults

- Hardware writes, sysfs writes, privileged commands and mutation D-Bus methods
  are forbidden without explicit permission from the specific task.
- Read-only capability must never be presented as write capability.
- Unsupported mutations return `Unsupported` honestly; never simulate success.
- Do not use `sudo` or real system/session bus in tests without explicit task
  permission.
- Do not run real UPower/asusd/supergfxd, external D-Bus daemons, Docker,
  Podman, VMs, or broad ignored tests without explicit task permission.
- Battery charge-limit, performance-profile, GPU/MUX/power mutations, direct
  real sysfs writes, real D-Bus mutation methods, and live ASUS hardware
  manipulation require explicit permission. VM/fake-system validation does not
  prove real hardware behavior.
- Fan curve writes are owned by typed asusd through Hardware1; never add direct
  sysfs fan writes or bypass Hardware1 for production mutation.
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

## Development environment

- The project development environment is the flake `devShell`. In normal
  interactive work, entering this repository through the approved `.envrc`
  activates it automatically via `direnv` and `nix-direnv`.
- `.envrc` is part of the project contract and must remain exactly `use flake`.
  Do not put secrets, credentials, manual `PATH` changes, duplicated toolchain
  setup, or project commands there. `.direnv/` is local cache state and is not
  committed.
- `rustc`, `cargo`, `rustfmt`, `clippy`, `rust-analyzer`, `slint-lsp`, and
  native build dependencies come from the project devShell, not the global
  workstation environment. OpenCode is global, but inherits this environment
  when started from the repository after direnv activation.

## Build / check / test commands

Verification tiers: выбирай минимальный tier, который реально покрывает
изменение. Не ослабляй correctness/security tests — tier выбирается по охвату,
а не для экономии.

`rustfmt 1.97.1` на текущем dev environment может аварийно завершаться в
`--check` при печати Unicode-heavy diff. Поэтому сначала форматируй, затем
проверяй формат. Не трактуй такой rustfmt SIGABRT как сбой Orbis daemon.

### FAST — default для обычного изменения одного crate

Во время разработки используй targeted checks. Не гоняй весь workspace после
каждой правки.

```bash
cargo fmt --all
cargo check -p <affected-crate> --all-targets
cargo test -p <affected-crate>
cargo clippy -p <affected-crate> --all-targets -- -D warnings
git diff --check
```

### INTEGRATION — изменение пересекает несколько crates / service boundaries

Полный workspace tier запускается один раз перед commit/acceptance:

```bash
cargo fmt --all
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

Для Rust-only изменений полный `nix flake check` не обязателен: он включает
build-heavy Nix derivations и VM checks. Для изменений в `flake.nix`,
`packaging/nix`, systemd, polkit или D-Bus сначала выполни подходящий Cargo
tier, затем targeted Nix validation:

```bash
nix flake check --no-build --system x86_64-linux
```

Для system integration выбирай соответствующий существующей boundary VM check,
а не запускай все VM checks без необходимости:

```text
checks.x86_64-linux.hardwared-lifecycle
checks.x86_64-linux.performance-mutation-vm
checks.x86_64-linux.battery-mutation-vm
```

Standalone development deployment deliberately owns only the hardwared binary
and `/etc/systemd/system/orbis-hardwared.service`. Static D-Bus/polkit policy is
registered separately by the policy-only NixOS module. Ordinary host rebuilds
must not own or restart the standalone daemon lifecycle.

### Docs-only / shell-only

```bash
git diff --check
# + релевантный rg/bash -n по затронутым файлам
```

Никаких workspace cargo/nix checks без отдельной причины.

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
- After a commit, verify the commit contains exactly the intended files and
  pre-existing baseline paths remain unchanged; unrelated baseline changes may
  remain in the working tree.
- Commit messages: `<scope>: <imperative summary>` (scopes like `ui:`,
  `session:`, `sessiond:`, `hardwared:`).

## Known project gotchas

- `orbis-hardwared` is a workspace member. Its privileged surface must stay
  narrow; adding a new mutation requires evidence for semantics, ownership,
  authorization and authoritative read-back.
- Slint UI files live in `ui/`; Rust glue in `crates/orbis-ui/src`. Theme tokens
  live in `ui/themes/dark.slint`. See `docs/ui-reference.md` and
  `docs/ui-measurements.json`.
- Dev environment: `nix develop` provides cargo/rustc/rustfmt/clippy and
  headless-Slint test env. Do not install a global Rust toolchain.
- Production providers already include real read-only sysfs/session backends and
  typed asusd/supergfxd compatibility adapters. Do not reintroduce production
  mock fallback.
- Fan curve sysfs values are raw PWM `0..255`, not percentages.
- GPU product policy (Eco/Standard/Ultimate/Optimized) is not proven merely by
  exposing backend-level supergfxd modes; keep product controls disabled until
  evidence establishes the mapping and lifecycle semantics.

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
`orbis-slint-ui`, `orbis-hardware-safety`, and `orbis-system-integration`.
Load UI or system-integration guidance only when the task matches its scope;
load hardware-safety as well when system-integration work changes hardware
semantics. Ordinary Rust/domain work follows this file directly.
