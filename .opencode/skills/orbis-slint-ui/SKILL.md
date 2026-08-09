---
name: orbis-slint-ui
description: Реальные UI paths и конвенции Slint в Orbis Control, включая theme/token, проверку рендера. Используй при работе с UI в этом репозитории.
---

# orbis-slint-ui

## UI source

- Slint UI файлы: `ui/` — `app-window.slint`, `themes/dark.slint`,
  `components/{section-header,value-slider,mode-card}.slint`.
- Rust glue: `crates/orbis-ui/src/` — `main.rs`, `worker.rs`, `controller.rs`, `lib.rs`.
- Slint version: `slint = { version = "1", features = [..., "backend-winit-wayland",
  "backend-winit-x11", "renderer-winit-software", "software-renderer-systemfonts",
  "image-default-formats"] }`; сборка через `slint-build` в `crates/orbis-ui/build.rs`.

## Известные UI-конвенции

- `docs/ui-reference.md` — источник UI-спецификации (размеры окон, структура
  главного окна, компоненты). `docs/ui-measurements.json` — машиночитаемые замеры.
- Тема: тёмная (`ui/themes/dark.slint`); токены — через ре-используемые
  компоненты, без хардкода цветов в местах использования.

## Проверка / рендер

- Headless-рендер: flake devShell настраивает `QT_XKB_CONFIG_ROOT` и
  `XDG_DATA_DIRS` для Slint software rendering; возможен screenshot-тест
  (`target/ui-review`).
- После изменения `.slint`: убедись, что `cargo check`/`cargo build` проходит
  (slint-build пересобирает), и при возможности — скриншот/визуальная проверка.

## Правила callbacks/state

- Следуй существующей структуре `worker.rs`/`controller.rs`: UI не выполняет
  hardware I/O напрямую; данные приходят через провайдеры/сервисы.
- Callbacks из `.slint` обрабатываются в контроллере/воркере; не дублируй
  логику в разметке.
