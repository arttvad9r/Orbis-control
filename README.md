# Orbis Control

Лёгкое системное приложение для ноутбуков ASUS на Linux: компактный интерфейс в
стиле G-Helper для управления производительностью, GPU, экраном, подсветкой,
батареей, вентиляторами и автоматизацией — нативно, без Electron.

> Статус: **Этап 0/1 завершён (исследование и спецификация)**. Кода ещё нет.
> См. `docs/`.

## Компоненты

| Компонент | Назначение | Статус |
|---|---|---|
| `orbis-control` | GUI (Slint) | не начат (Этап 2) |
| `orbis-sessiond` | пользовательский демон | не начат (Этап 3+) |
| `orbisctl` | диагностический CLI | не начат |
| `orbis-hardwared` | опциональный root-helper | не создаётся до доказанной необходимости (ADR 0002) |

## Совместимость

Целевые дистрибутивы: Fedora, Arch Linux, Ubuntu LTS, Debian, openSUSE.
Основная платформа — Wayland; X11 — режим совместимости.
Поддерживаются ноутбуки ASUS с интерфейсами `asusd`/`asus-armoury`/kernel ABI
(см. `docs/hardware-support.md` — будет создан на Этапе 2).

## Документация

- `docs/research-report.md` — исследование upstream-проектов (commit hashes, лицензии, API)
- `docs/feature-matrix.md` — таблица функций G-Helper и их Linux-реализаций
- `docs/provider-matrix.md` — провайдеры и backend-интерфейсы
- `docs/ui-reference.md`, `docs/ui-measurements.json` — UI-спецификация по G-Helper
- `docs/architecture.md` — архитектура
- `docs/threat-model.md` — модель угроз
- `docs/adr/` — записи архитектурных решений

## Лицензия

GPL-3.0-or-later. Третьесторонние компоненты и происхождение идей — в
`THIRD_PARTY_NOTICES.md`.

## Дисклеймер

Orbis Control — независимый проект, не связан с ASUSTeK Computer Inc. Название,
логотипы ASUS/ROG/TUF и G-Helper не используются в основном имени/иконке/application ID.
