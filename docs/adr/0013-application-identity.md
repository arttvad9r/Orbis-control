# ADR 0013: Permanent Application Identity

- Статус: **Принято** (2026-08-25)
- Дата: 2026-08-25

## Контекст

До stable release нужно зафиксировать reverse-DNS identity, которая уже
используется в desktop metadata, D-Bus names, polkit actions, autostart,
пользовательском состоянии и system services. Текущий GitHub owner не должен
становиться скрытым владельцем ABI.

## Решение

Постоянная canonical identity проекта — **`io.github.orbiscontrol.Orbis`**.

Связанные имена `io.github.orbiscontrol.*` считаются частью публичного ABI и
не меняются при смене GitHub-организации, зеркала или владельца репозитория.
Миграция identity перед stable release не требуется.

## Последствия

- D-Bus service/interface names, polkit action IDs, desktop/AppStream IDs,
  autostart filename и persisted XDG state сохраняют текущий namespace.
- Новые public names должны использовать этот namespace.
- Смена hosting/repository owner сама по себе не является основанием для
  переименования ABI; любое исключение требует отдельного ADR и migration
  inventory.
