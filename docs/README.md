# Documentation Index

Этот индекс определяет роли документов и source-of-truth hierarchy Orbis
Control. Chat transcripts и старые session summaries не являются project source
of truth.

## Быстрый вход

После корневого [`AGENTS.md`](../AGENTS.md) новая development session читает:

1. [`current-state.md`](current-state.md) — что реально существует сейчас;
2. [`architecture.md`](architecture.md) — текущие boundaries и invariants;
3. [`roadmap.md`](roadmap.md) — следующий порядок работ;
4. релевантный [ADR](adr/) или hardware evidence только по задаче.

## Source-of-truth hierarchy

### Current implementation facts

1. Production code и tests.
2. [`current-state.md`](current-state.md).

### Architectural intent

1. Accepted ADR.
2. [`architecture.md`](architecture.md).

### Hardware facts

1. Dated live hardware audit / immutable probe evidence.
2. Hardware fixtures в [`tests/fixtures/hardware/`](../tests/fixtures/hardware/).
3. Historical research reports.

Inference, product defaults и mock values не становятся hardware facts. При
конфликте current code не переписывает прошлое hardware snapshot: новый probe
создаёт новый dated evidence artifact.

### Future work

[`roadmap.md`](roadmap.md).

### Agent rules

Корневой [`AGENTS.md`](../AGENTS.md).

### Temporary handoff

Корневой `HANDOFF.md` — только navigation aid между сессиями. Он не заменяет
current-state, architecture, roadmap или ADR и может устареть.

## Карта документов

| Документ | Классификация | Как использовать |
|---|---|---|
| [`current-state.md`](current-state.md) | CURRENT STATUS | Operational baseline текущего HEAD |
| [`architecture.md`](architecture.md) | CURRENT DESIGN | Boundaries, semantics, safety contracts |
| [`roadmap.md`](roadmap.md) | FUTURE PLAN | Milestones и порядок работ |
| [`adr/`](adr/) | ADR | Принятые архитектурные решения |
| [`provider-matrix.md`](provider-matrix.md) | CURRENT DESIGN + dated evidence | Target provider strategy; не implementation status |
| [`feature-matrix.md`](feature-matrix.md) | HISTORICAL SNAPSHOT | Stage 0 Linux feasibility, не список готовых функций |
| [`research-report.md`](research-report.md) | HARDWARE EVIDENCE / HISTORICAL SNAPSHOT | Audit от 2026-08-06; факты не ретушировать |
| [`ui-reference.md`](ui-reference.md) | UI REFERENCE | Исторический G-Helper layout reference |
| [`ui-measurements.json`](ui-measurements.json) | UI REFERENCE | Machine-readable dated measurements |
| [`threat-model.md`](threat-model.md) | CURRENT/FUTURE SECURITY DESIGN | Threat baseline; часть mitigations ещё planned |

Accepted ADR:

- [`0001-rust-and-slint.md`](adr/0001-rust-and-slint.md) — Rust + Slint;
- [`0002-daemon-boundaries.md`](adr/0002-daemon-boundaries.md) — GUI/sessiond/
  optional hardwared boundaries;
- [`0003-gpu-provider-strategy.md`](adr/0003-gpu-provider-strategy.md) — GPU
  concepts и backend strategy;
- [`0004-authoritative-read-only-session.md`](adr/0004-authoritative-read-only-session.md)
  — read-only session semantics, unknown bounds, no-cache reads и lifecycle;
- [`0005-split-gpu-provider-capabilities.md`](adr/0005-split-gpu-provider-capabilities.md)
  — split GPU provider capabilities по hardware concepts.

Hardware fixtures FA707NV собраны 2026-08-06 read-only. В частности, наличие
`xyz.ljones.Platform` и `xyz.ljones.FanCurves` доказано introspection evidence.
Значение charge threshold 80 было observed, но hardware min/max/step этим не
доказаны. Историческое поле range в expected-capabilities fixture не должно
интерпретироваться как current production bounds.

## Правила обновления

- Изменился architecture contract → обновить `architecture.md` и, если решение
  значимое, добавить/заменить ADR.
- Завершён vertical slice → обновить `current-state.md`.
- Изменились приоритеты или порядок → обновить `roadmap.md`.
- Получен новый hardware probe → добавить новый dated snapshot; старый не
  переписывать под новое состояние.
- Mechanical refactor → документацию менять только если изменился externally
  visible state, contract или source-of-truth link.
- Feasibility не отмечать как IMPLEMENTED без production code/tests.
