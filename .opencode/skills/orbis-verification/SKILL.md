---
name: orbis-verification
description: Единый финальный pipeline проверки перед завершением задачи в Orbis Control. Не дублирует содержимое других skills, а собирает их команды в один порядок.
---

# orbis-verification

Выполняй этот pipeline перед объявлением задачи завершённой (Rust-код):

```bash
# 1. Формат
cargo fmt --all -- --check

# 2. Статические проверки / компиляция
cargo check --workspace

# 3. Lint
cargo clippy --workspace --all-targets -- -D warnings

# 4. Тесты
cargo test --workspace

# 5. Git-состояние
git status --short
git diff --check
git diff --stat
```

## Правила

- Каждая команда должна реально выполниться; не заявляй, что проверка прошла,
  если она не запускалась.
- Не ослабляй lint-политику и тесты, чтобы пройти.
- Для hardware/UI-специфичного кода дополнительно применяй
  `orbis-hardware-safety` и `orbis-slint-ui`.
- Docs-only изменения: достаточно `git diff --check` + проверки Markdown,
  полный cargo-прогон не требуется.

## Финальный отчёт (по AGENTS.md)

1. Изменённые файлы. 2. Что реализовано. 3. Ключевые инварианты.
4. Тесты добавлены/изменены. 5. Результаты проверок. 6. `git status --short`.
7. Был ли commit. 8. Что намеренно вне scope.
