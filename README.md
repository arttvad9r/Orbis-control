# Orbis Control

Orbis Control — нативное Rust + Slint приложение для управления и наблюдения за
возможностями ASUS-ноутбуков на Linux. Проект Wayland-first; X11 поддерживается
как compatibility mode. GUI не запускается от root и не выполняет direct
hardware I/O.

## Текущий статус

Проект находится в ранней development стадии (`0.1.0`). Реализованы domain,
provider и application boundaries, Slint mock UI и первый **read-only MVP**:
production GUI реально показывает Battery Charge Limit, Performance Mode и
GPU Power/MUX/Access через session path:

```text
UPower / kernel platform_profile / supergfxd / ASUS Armoury sysfs
→ orbis-sessiond → session D-Bus → orbis-session-client → GUI
```

Путь протестирован и live-validated как Nix-installed systemd user service с
`Type=dbus` и clean SIGTERM shutdown (daemon absent → честный Unavailable без
mock fallback; daemon present → Ready со значениями, совпадающими с
authoritative backends). Все mutation controls в production read-only/disabled
(Battery slider, Performance cards, GPU Eco/Standard/Ultimate/Optimized);
product GPU mode пока mock-only внутри legacy code. Hardware mutations не
реализованы.

Точный статус по областям: [`docs/current-state.md`](docs/current-state.md).

## Архитектура

```text
Slint UI → sequential worker → AppService → provider traits
                                      |
                                      +→ session client → sessiond → UPower / kernel / supergfxd / Armoury (real reads)
                                      +→ MockProvider (product GpuMode / legacy / offscreen-mock)
```

- Backend state обновляется только из authoritative reads/read-back.
- Architecture capability-driven: unknown не подменяется unsupported или
  product defaults.
- `orbis-sessiond` — user daemon.
- `orbis-hardwared` не входит в workspace и не вводится без доказанной
  privileged hardware operation.

Подробнее: [`docs/architecture.md`](docs/architecture.md).

## Workspace

Основные crates: `orbis-core`, `orbis-config`, `orbis-capabilities`,
`orbis-providers`, `orbis-application`, `orbis-session-protocol`,
`orbis-session-client`, `orbis-sessiond`, `orbis-ui`, `orbis-cli` и
`orbis-test-support`.

## Development

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

## Документация

Начните с [`docs/README.md`](docs/README.md): там определены source-of-truth
hierarchy и роли current/historical документов. План развития —
[`docs/roadmap.md`](docs/roadmap.md).

## Лицензия

GPL-3.0-or-later.

Orbis Control — независимый проект, не связанный с ASUSTeK Computer Inc.
