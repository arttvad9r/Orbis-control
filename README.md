# Orbis Control

Orbis Control — нативное Rust + Slint приложение для управления и наблюдения за
возможностями ASUS-ноутбуков на Linux. Проект Wayland-first; X11 поддерживается
как compatibility mode. Privileged hardware mutations проходят только через
отдельный typed `Hardware1` boundary с polkit.

## Текущий статус

Проект находится в ранней development стадии (`0.1.0`). `main` — текущая
интеграционная линия после production-hardening pass.

Подтверждённые production-направления включают:

- Battery Charge Limit read path через Session1/UPower;
- controlled Battery mutation через Hardware1/asusd с authoritative read-back;
- Performance read через kernel `platform_profile` и controlled mutation через
  Hardware1 с read-back;
- независимые GPU Power / physical MUX / access-policy reads;
- production sysfs telemetry polling;
- profile-specific fan reads через asusd и active-curve reads через sysfs;
- runtime capability registry с отдельным read/write evidence;
- versioned XDG preferences, window state и desired-state foundations;
- typed privacy-bounded diagnostics foundations;
- read-only `orbisctl status` поверх Session1 providers;
- NixOS package/module, desktop/AppStream metadata и support-matrix tooling.

Некоторые функции намеренно fail-closed. Product GPU mode
(Eco/Standard/Ultimate/Optimized), power limits и неподтверждённые ASUS controls
не включаются без concept-specific evidence. Fan mutation/reset implementation
остаётся недоступной через текущий production product path: UI/policy/backend
composition блокируют writes до исправления открытых safety-contract defects.
Fan reads остаются доступны.

Release заблокирован #106: GitHub Actions сейчас не предоставляет trustworthy
executable validation текущего `main`. Пока blocker существует, static review и
source inspection не считаются green CI. Final release также требует успешно
выполненных Cargo checks/tests/clippy, `nix flake check` и packaged acceptance на
точной release revision.

Точный operational status: [`docs/current-state.md`](docs/current-state.md).
Порядок работ: [`docs/roadmap.md`](docs/roadmap.md).

## Архитектура

Read path:

```text
UPower / kernel / supergfxd / ASUS firmware attributes / asusd
→ orbis-sessiond → Session1
→ orbis-session-client / providers
→ application worker → GUI / read-only CLI
```

Privileged mutation path:

```text
GUI/application original caller
→ Hardware1 system bus
→ per-capability polkit authorization
→ typed bounded backend
→ authoritative read-back where the operation can be confirmed
```

Основные правила:

- production GUI должен работать как обычный user-session process; raw euid-0
  rejection ещё не enforced и отслеживается в #125;
- `sessiond` остаётся read/session boundary и не является mutation deputy;
- `Unsupported`, `Unavailable`, `PermissionDenied` и `Unknown` не подменяют друг друга;
- read/write evidence хранится раздельно;
- `ApplyResult::Accepted` не считается `Applied`;
- model-name tables не заменяют runtime probes;
- device-specific/live claims всегда revision-scoped;
- отключённый product/backend не должен рекламироваться как writable только из-за
  существования ABI или provider implementation.

Подробнее: [`docs/architecture.md`](docs/architecture.md) и ADRs в
[`docs/adr/`](docs/adr/).

## Workspace

Основные crates:

- `orbis-core` — domain model и invariants;
- `orbis-config` — XDG preferences/state foundations;
- `orbis-capabilities` — capability registry/evidence;
- `orbis-providers` — typed platform/provider implementations;
- `orbis-application` — application services/commands/diagnostics collector;
- `orbis-session-protocol`, `orbis-session-client`, `orbis-sessiond` — read/session D-Bus path;
- `orbis-hardwared` — narrow privileged Hardware1 service;
- `orbis-ui` — Slint GUI + worker/composition;
- `orbis-cli` — read-only CLI surface; runtime validation/polish tracked by #119;
- `orbis-test-support` — test/demo fixtures; removal from default release graph tracked by #115.

## Development

Rust toolchain: **1.87**.

Dev environment:

```bash
nix develop
```

Canonical repository verification entrypoint:

```bash
scripts/verify quick
scripts/verify task
scripts/verify full
```

Underlying Rust checks include:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Full verification additionally includes Nix build/flake checks. Пока #106
недоступен, эти команды должны быть реально выполнены в доверенной executable
environment прежде чем результат можно будет назвать `TESTED`/green.

Не используйте наличие UI control, provider object или файла в sysfs как
доказательство write support. Для release/evidence правил см.
[`docs/release-evidence-taxonomy.md`](docs/release-evidence-taxonomy.md).

## Документация

Начните с [`docs/README.md`](docs/README.md). Он определяет canonical hierarchy и
отделяет current source of truth от dated audits/remediation snapshots.

- operational baseline: [`docs/current-state.md`](docs/current-state.md);
- architecture: [`docs/architecture.md`](docs/architecture.md);
- verification/evidence: [`docs/verification.md`](docs/verification.md);
- active plan: [`docs/roadmap.md`](docs/roadmap.md);
- historical/superseded records: [`docs/history.md`](docs/history.md).

Chat/session transcripts, local runtime handoffs, secrets и одноразовые generated
archives не должны храниться как project documentation.

## Лицензия

GPL-3.0-or-later.

Orbis Control — независимый проект, не связанный с ASUSTeK Computer Inc.
