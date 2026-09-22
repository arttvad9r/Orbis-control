# Orbis Control — planning log

Формат: одно запись = одно существенное решение. Записи дополняются; источник истины —
контракты в репо и доска `orbis-v01`, не этот журнал.

## 2026-09-21 — D-008..D-011 (записаны в DECISIONS.md репо)

- **D-008**: разрешение на bounded live-hardware round-trip валидацию выдано заранее
  (финальный гейт v0.1, восстановление значений обязательно).
- **D-009**: цель итерации — первый релиз v0.1 (PKGBUILD + installed smoke + тег).
  Draft PR #130 закрыт как устаревший.
- **D-010**: ветка `implementation/current-plan` запушена в origin; `main` не двигаем до RC.
- **D-011**: основной backend-референс — G-Helper-linux, `/home/artt/g-helper-linux`
  (upstream `utajum/g-helper-linux`). Затык → сначала изучить модуль референса
  (`src/Platform/Linux/Asus/`, `src/Fan/`, `src/Gpu/`, `src/Battery/`, `src/Mode/`),
  адаптировать под Rust/capability/Hardware1; слепое копирование запрещено.

## 2026-09-21 — Топология исполнения

Выбор: **один implementation-поток (developer) → независимая QA → planner release-гейт**.
Один общий workspace (`dir:`), карты последовательные, параллельных писателей нет.

Карты доски `orbis-v01`:

| id | роль | суть | зависит от |
|---|---|---|---|
| t_3e04f428 (A) | developer | верификация этапов 1–3 против AC, фиксация вердиктов | — |
| t_b16d7354 (B) | developer | fan curves production (этап 1) | A |
| t_51a6ba86 (C) | developer | power limits + boost (этапы 2–4) | A |
| t_89ca2ac9 (D) | developer | GPU modes / MUX (этап 5) | C |
| t_5b9d96c6 (QA) | qa | liveness, инвентаризация, визуальные состояния, честность capability | B, C, D |
| t_37e1764f (E) | developer | Arch-пакет + установленный кандидат + smoke (этапы 6–7 FINISH_PLAN) | QA |
| t_9f6592c8 (R) | planner | терминальный release-гейт v0.1, live round-trip (D-008), bump, тег | E |

Почему так: работа невелика и последовательна по одной ветке кода; параллельные worktree
не нужны. Карта A сначала доказывает, что уже сделано (не переписывать работающее), B–D
закрывают реальные разрывы по очереди IMPLEMENTATION_PLAN.

Контракты: `acceptance-contract.yaml` (19 сценариев SC-*), `interaction-contract.md`,
`docs/qa-coverage.txt` — валидированы board-protocol, коммит bb1c854.

## 2026-09-21 — Базовое состояние (проверено прогонами)

- `scripts/verify task` (fmt+check+test+clippy -D warnings) — зелёный;
- `cargo build --workspace --release --locked` — зелёный, 4 бинарника;
- рабочее дерево чистое; HEAD реализации: 06fbf16 (после bb1c854 — bb1c854);
- доска: орбис-v01, карта A запущена диспетчером сразу после создания.

## Следующее действие

Мониторинг карты A (t_3e04f428): вердикты AC этапов 1–3 → разблокировка B и C.

## 2026-09-22 — UI-линия: параллельный visual-repair поток

Решение:

Жалоба пользователя (симметрия, размерности, логика построения) оформлена отдельной UI-линией
на доске orbis-v01: UI-1 аудит (t_5fc90d28) -> UI-2 исправление (t_29073c31) -> UI-QA приёмка
(t_ccbe5158), все в worktree-ветке wt/ui-symmetry от ui-профиля. Слияние с функциональной линией —
отдельной картой INT (t_3864d46e, developer) после завершения A..D и PASS UI-QA; release R (t_9f6592c8)
теперь зависит и от INT.

Границы: правки только ui/*.slint + стилевые константы; поведение контролов, backend-крейты,
навигация и фреймворк не меняются (D-006: исправление дефектов audited UI, не новый дизайн).

Почему:

Два писателя в одном чекауте запрещены; worktree развязывает линии, а INT-карта делает слияние
управляемым и проверяемым (build+verify+smoke на объединённой ветке).
# Orbis Control — planning log

Формат: одно запись = одно существенное решение. Записи дополняются; источник истины —
контракты в репо и доска `orbis-v01`, не этот журнал.

## 2026-09-21 — D-008..D-011 (записаны в DECISIONS.md репо)

- **D-008**: разрешение на bounded live-hardware round-trip валидацию выдано заранее
  (финальный гейт v0.1, восстановление значений обязательно).
- **D-009**: цель итерации — первый релиз v0.1 (PKGBUILD + installed smoke + тег).
  Draft PR #130 закрыт как устаревший.
- **D-010**: ветка `implementation/current-plan` запушена в origin; `main` не двигаем до RC.
- **D-011**: основной backend-референс — G-Helper-linux, `/home/artt/g-helper-linux`
  (upstream `utajum/g-helper-linux`). Затык → сначала изучить модуль референса
  (`src/Platform/Linux/Asus/`, `src/Fan/`, `src/Gpu/`, `src/Battery/`, `src/Mode/`),
  адаптировать под Rust/capability/Hardware1; слепое копирование запрещено.

## 2026-09-21 — Топология исполнения

Выбор: **один implementation-поток (developer) → независимая QA → planner release-гейт**.
Один общий workspace (`dir:`), карты последовательные, параллельных писателей нет.

Карты доски `orbis-v01`:

| id | роль | суть | зависит от |
|---|---|---|---|
| t_3e04f428 (A) | developer | верификация этапов 1–3 против AC, фиксация вердиктов | — |
| t_b16d7354 (B) | developer | fan curves production (этап 1) | A |
| t_51a6ba86 (C) | developer | power limits + boost (этапы 2–4) | A |
| t_89ca2ac9 (D) | developer | GPU modes / MUX (этап 5) | C |
| t_5b9d96c6 (QA) | qa | liveness, инвентаризация, визуальные состояния, честность capability | B, C, D |
| t_37e1764f (E) | developer | Arch-пакет + установленный кандидат + smoke (этапы 6–7 FINISH_PLAN) | QA |
| t_9f6592c8 (R) | planner | терминальный release-гейт v0.1, live round-trip (D-008), bump, тег | E |

Почему так: работа невелика и последовательна по одной ветке кода; параллельные worktree
не нужны. Карта A сначала доказывает, что уже сделано (не переписывать работающее), B–D
закрывают реальные разрывы по очереди IMPLEMENTATION_PLAN.

Контракты: `acceptance-contract.yaml` (19 сценариев SC-*), `interaction-contract.md`,
`docs/qa-coverage.txt` — валидированы board-protocol, коммит bb1c854.

## 2026-09-21 — Базовое состояние (проверено прогонами)

- `scripts/verify task` (fmt+check+test+clippy -D warnings) — зелёный;
- `cargo build --workspace --release --locked` — зелёный, 4 бинарника;
- рабочее дерево чистое; HEAD реализации: 06fbf16 (после bb1c854 — bb1c854);
- доска: орбис-v01, карта A запущена диспетчером сразу после создания.

## Следующее действие

Мониторинг карты A (t_3e04f428): вердикты AC этапов 1–3 → разблокировка B и C.

## 2026-09-22 — UI-линия: параллельный visual-repair поток

Решение:

Жалоба пользователя (симметрия, размерности, логика построения) оформлена отдельной UI-линией
на доске orbis-v01: UI-1 аудит (t_5fc90d28) -> UI-2 исправление (t_29073c31) -> UI-QA приёмка
(t_ccbe5158), все в worktree-ветке wt/ui-symmetry от ui-профиля. Слияние с функциональной линией —
отдельной картой INT (t_3864d46e, developer) после завершения A..D и PASS UI-QA; release R (t_9f6592c8)
теперь зависит и от INT.

Границы: правки только ui/*.slint + стилевые константы; поведение контролов, backend-крейты,
навигация и фреймворк не меняются (D-006: исправление дефектов audited UI, не новый дизайн).

Почему:

Два писателя в одном чекауте запрещены; worktree развязывает линии, а INT-карта делает слияние
управляемым и проверяемым (build+verify+smoke на объединённой ветке).

## 2026-09-22 — D-012 полный паритет с G-Helper
Пользователь: «Orbis должен уметь всё то же самое, без каких либо оправданий».
Собран feature-инвентарь G-Helper (36 групп), сравнён с Orbis: ~16 YES/PART,
~20 NO. Разбит на волны P1..P6 (после v0.1). Матрица: docs/parity/g-helper-parity.md.
SPEC-запреты, конфликтующие с D-012, сняты; инварианты архитектуры сохранены.

## 2026-09-22 — INT merge accepted; QA-card splitting pattern

**Decision:** INT merged candidate `5f0d3da` accepted as the v0.1 candidate identity; packaging (E) now
gated on INT as well as on QA-verdict.

**Context:** INT merged the QA-PASSed UI line (`f45f55d`) into the functional line (`5ca36ab`) with
`--no-ff`, no conflicts. Because a merge can silently drop a fix, I did not accept the worker's report:
I verified G03 survival directly (`git diff f45f55d HEAD -- ui/audited/sections/about.slint` -> identical;
`width: parent.width` -> 0 matches, while base `5ca36ab` still had both lines) and ran a calibrated pixel
probe on the merged renders (0 px outside the card on all 4 About variants; the defect state measured
+12..+14 px). All 29 merged renders are byte-identical to the QA-PASSed renders, so the merge introduced
no visual drift.

**Options considered:** (a) accept the INT worker's verdict as-is; (b) re-run the whole QA matrix on the
merged candidate (~1h, previously timed out twice); (c) targeted planner spot-check of the merge-invariant
properties. Chose (c): the merge touched no UI logic beyond wiring, and the one risky property (fix survival)
is directly observable and cheap to prove.

**Pattern recorded (used twice, worked twice):** QA cards on this board repeatedly exhaust their iteration
budget *after* collecting evidence but *before* writing the typed verdict (UI-QA2: 82 renders; functional QA:
38 live shots + probes + verify logs). The repair is not a re-run but a small `*-verdict` card scoped to
analysis-only (explicit ban on re-render/re-build/re-launch), with artifact paths pre-injected in the contract.
Both times the evidence was real and the verdict landed on the first run.

**Gate-contract lessons (durable):** the completion gate reads the RUN envelope and requires (1) `expected`
text byte-equal to the card's `expected_observable`, (2) evidence references to FILES, never directories,
(3) run_id/task_id present, (4) every referenced path to exist at gate time. Backfills that paraphrase
`expected` are rejected as `CHECK_EXPECTED_REWRITTEN`; directory refs as `EVIDENCE_NOT_FOUND`.

**Forecloses:** nothing about scope. It does establish that merge-invariant acceptance evidence must be
produced by the planner, not inherited from the branch that was merged.
