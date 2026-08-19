# Orbis Control

Orbis Control — нативное Rust + Slint приложение для управления и наблюдения за
возможностями ASUS-ноутбуков на Linux. Проект Wayland-first; X11 поддерживается
как compatibility mode. GUI работает без root; privileged hardware mutations
проходят только через отдельный typed `Hardware1` boundary с polkit.

## Текущий статус

Проект находится в ранней development стадии (`0.1.0`). `main` является текущей
интеграционной линией после production-hardening pass.

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
- typed privacy-bounded diagnostics/export foundations;
- NixOS package/module, desktop/AppStream metadata и support-matrix tooling.

Некоторые функции намеренно остаются fail-closed. В частности, product GPU mode
(Eco/Standard/Ultimate/Optimized), power limits и неподтверждённые ASUS controls
не включаются без concept-specific evidence. Fan mutation/reset code существует,
но сейчас заблокирован в UI и packaged polkit policy из-за открытых safety-contract
дефектов; fan reads остаются доступны.

Release также заблокирован до восстановления исполняемого GitHub Actions CI и
успешного `nix flake check` на точном `main` revision.

Точный фактический статус и список blockers: [`docs/current-state.md`](docs/current-state.md).
План работ: [`docs/roadmap.md`](docs/roadmap.md).

## Архитектура

Read path:

```text
UPower / kernel / supergfxd / ASUS firmware attributes / asusd
→ orbis-sessiond → Session1
→ orbis-session-client / providers
→ application worker → GUI
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

- GUI не запускается от root и не имеет generic privileged proxy;
- `sessiond` остаётся read/session boundary и не является mutation deputy;
- `Unsupported`, `Unavailable`, `PermissionDenied` и `Unknown` не подменяют друг
  друга;
- read/write evidence хранится раздельно;
- `ApplyResult::Accepted` не считается `Applied`;
- model-name tables не заменяют runtime probes;
- device-specific/live claims всегда revision-scoped.

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
- `orbis-cli` — CLI crate (currently incomplete);
- `orbis-test-support` — test/demo fixtures.

## Development

Rust toolchain: **1.87**.

Dev environment:

```bash
nix develop
```

Основные проверки:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Nix:

```bash
nix flake check
nix build .#orbis-control
```

Не используйте наличие UI control, provider object или файла в sysfs как
доказательство write support. Для release/evidence правил см.
[`docs/release-evidence-taxonomy.md`](docs/release-evidence-taxonomy.md).

## Документация

Начните с [`docs/README.md`](docs/README.md): там определены роли current,
architecture, ADR и historical документов. Operational source of truth —
[`docs/current-state.md`](docs/current-state.md).

## Лицензия

GPL-3.0-or-later.

Orbis Control — независимый проект, не связанный с ASUSTeK Computer Inc.