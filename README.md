# Orbis Control

Orbis Control — Linux-first приложение на Rust + Slint для управления и наблюдения за возможностями ASUS ROG/TUF/Zephyrus. Проект Wayland-first; X11 используется как compatibility path там, где это имеет смысл. Privileged hardware mutations проходят только через узкий typed `Hardware1` boundary с per-capability polkit.

## Статус проекта

Версия workspace: `0.1.0`. Активная интеграционная ветка — `development`; она консолидирует прежнюю линию `asus-hardware-validation-20260821` (head PR #129, `e8b611e`) и содержит более новый UI/backend/runtime слой, чем текущий `main`. `main` остаётся последней консолидированной release-базой до отдельной интеграции.

Текущие source-level production slices в активной ветке:

- Battery Charge Limit: Session1 read + controlled Hardware1 mutation + authoritative read-back;
- Performance: Session1/kernel read + Hardware1 mutation + authoritative read-back;
- независимые GPU Power / physical MUX / access-policy reads;
- read-only sysfs telemetry;
- profile-specific fan reads через Session1/asusd и active-curve reads через sysfs;
- immutable capability registry с отдельным read/write evidence;
- bounded provider execution для public capability probes, `orbisctl` и основных worker-owned authoritative reads;
- XDG preferences, autostart, window state, tray/close lifecycle;
- privacy-bounded Diagnostics refresh/export/copy;
- read-only `orbisctl status` и versioned `orbisctl status --json`;
- Slint UI с отдельными quick/advanced surfaces и fail-closed disabled controls;
- NixOS package/module, desktop/AppStream metadata и support-matrix tooling.

Некоторые функции намеренно отключены. Product GPU mode, fan writes/reset, Panel/Keyboard/Aura product writes, unattended Automation execution, Display modeset и self-update не включаются только потому, что существует похожий backend/API. Для них требуется отдельное concept-specific evidence и explicit promotion.

## Release gate

Release сейчас **BLOCKED**. Основной внешний blocker — #106: GitHub Actions не выполняет trustworthy repository jobs. Локальный Rust/Cargo toolchain (flake devShell) доступен: на текущей ревизии `cargo fmt/check/test/clippy --locked` выполнялись успешно, а `python3 scripts/verify-static` проходит после `317f22d`. Это source/test-level evidence для точной ревизии; оно не является runtime/package/hardware verification и не отменяет #106.

До release обязательны:

- executable `cargo fmt/check/test/clippy --locked` на точной revision;
- executable `nix flake check` и package acceptance;
- GUI euid-0 guard (#125);
- завершение timeout/unknown-outcome hardening (#123);
- отсутствие известных unsafe writes в enabled product path;
- revision-scoped hardware evidence для заявляемых hardware features;
- решение application identity #124;
- после восстановления CI — required checks / protection для `main` (#114).

Точный статус: [`docs/current-state.md`](docs/current-state.md). План: [`docs/roadmap.md`](docs/roadmap.md).

## Архитектура

Read path:

```text
UPower / kernel / supergfxd / asusd / read-only sysfs / Wayland observation
→ orbis-sessiond / typed providers
→ Session1 / application runtime
→ worker → GUI / read-only CLI / diagnostics
```

Privileged mutation path:

```text
original GUI/application caller
→ Hardware1 system bus
→ capability-specific polkit
→ narrow typed backend
→ authoritative read-back / explicit Pending / fail-closed error
```

Ключевые invariants:

- GUI — обычный user-session process; прямой root запуск должен быть отклонён (#125);
- `sessiond` не является privileged mutation deputy;
- generic root/sysfs/shell proxy отсутствует;
- read/write evidence независимы;
- `Unsupported`, `BackendMissing`, `TemporarilyUnavailable`, `PermissionDenied` и `Unknown` не взаимозаменяемы;
- `Accepted != Applied`;
- model/DMI name — hint, а не доказательство support;
- capability refresh публикуется whole-swap generation;
- mutation timeout после возможного dispatch не должен автоматически считаться обычным failure/retry;
- live hardware claims всегда revision-scoped.

Подробнее: [`docs/architecture.md`](docs/architecture.md).

## Workspace

- `orbis-core` — domain types и invariants;
- `orbis-config` — XDG preferences/state/desired-state foundations;
- `orbis-capabilities` — capability evidence и immutable registry snapshots;
- `orbis-providers` — typed platform/provider implementations;
- `orbis-application` — application services, command/read-back composition, diagnostics collector;
- `orbis-session-protocol` / `orbis-session-client` / `orbis-sessiond` — user-session read path;
- `orbis-hardwared` — narrow privileged Hardware1 service;
- `orbis-ui` — Slint UI, production composition и current worker runtime;
- `orbis-cli` — read-only CLI;
- `orbis-test-support` — fixtures/screenshots/tests; removal from the default GUI release dependency graph remains #115.

Production worker в активной ветке — `crates/orbis-ui/src/worker_runtime.rs`; legacy large worker files не должны использоваться как source of truth для новых runtime fixes.

## Development / verification

Rust toolchain contract: **1.87**.

```bash
nix develop
scripts/verify-static
scripts/verify quick
scripts/verify task
scripts/verify full
```

Intended Rust gates:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

`scripts/verify-static` — только source-level safety net для среды без toolchain. Он не заменяет компиляцию.

Дополнительные ограничения evidence:

- `nix flake check --no-build` подтверждает только evaluation flake, а не VM/build/package acceptance;
- локальные Cargo-результаты действительны для точной ревизии и не восстанавливают hosted CI (#106);
- новые изменения ветки `development` не имеют полноценного live hardware evidence.

## Документация

Начните с [`docs/README.md`](docs/README.md). Canonical hierarchy:

- [`docs/current-state.md`](docs/current-state.md) — current source status;
- [`docs/architecture.md`](docs/architecture.md) — архитектурный контракт;
- [`docs/verification.md`](docs/verification.md) — evidence policy;
- [`docs/release-evidence-taxonomy.md`](docs/release-evidence-taxonomy.md) — уровни claims;
- [`docs/roadmap.md`](docs/roadmap.md) — активный backlog/order;
- [`docs/backend-completion-status.md`](docs/backend-completion-status.md) — UI/backend connection summary;
- [`docs/beta-acceptance-checklist.md`](docs/beta-acceptance-checklist.md) — текущие beta gates;
- [`docs/history.md`](docs/history.md) — historical/superseded records.

Не храните в project docs chat/session transcripts, secrets, private runtime dumps и одноразовые validation trigger artifacts.

## Лицензия

GPL-3.0-or-later.

Orbis Control — независимый проект, не связанный с ASUSTeK Computer Inc.
