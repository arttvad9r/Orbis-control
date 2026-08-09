---
name: orbis-rust-change
description: Реальный workflow изменения Rust-кода в Orbis Control: какие crates существуют, какие команды проверки обязательны, минимальный scope. Используй перед любой задачей, меняющей Rust-код в этом репозитории.
---

# orbis-rust-change

## Структура workspace

Rust workspace (`resolver = 3`, edition 2024, MSRV 1.85). Crates:

- `orbis-core` — domain types и их инварианты.
- `orbis-config` — конфигурация приложения.
- `orbis-capabilities` — обнаружение/проверка capability (`engine.rs`, `fixture.rs`).
- `orbis-providers` — trait'ы провайдеров и mock (`traits.rs`, `mock.rs`, `error.rs`); sysfs/asusd-провайдеры — Этап 3+.
- `orbis-application` — application layer.
- `orbis-sessiond` — пользовательский демон сессии (D-Bus).
- `orbis-session-protocol` — D-Bus protocol DTOs.
- `orbis-session-client` — клиент сессии.
- `orbis-ui` — Slint UI (`src/{main.rs,worker.rs,controller.rs}`, UI файлы в `ui/`).
- `orbis-cli` — CLI.
- `orbis-test-support` — тестовая поддержка.
- `orbis-hardwared` — НЕ включён в workspace (ADR 0002); не добавляй без решения.

## Обязательные проверки (реальные команды проекта)

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

- Сначала прогоняй узкий target изменённого crate, затем workspace-проверки.
- Интеграционные тесты с handshake — с bounded timeout; без `sleep`/polling
  для маскировки гонок.
- Кэш-семантика: две последовательные отличающиеся проверки для fresh reads.

## Scope discipline

- Меняй только файлы, разрешённые задачей; никаких incidental refactors.
- Не меняй публичный API без явного разрешения; не добавляй зависимости без
  нужды; не меняй `Cargo.lock` без легитимной причины.
- Сохраняй `forbid`/`deny unsafe_code` lints; не добавляй `unsafe`.
- Не ослабляй lint-политику, чтобы пройти проверку.
