# Documentation Index

Этот индекс определяет роли документов и source-of-truth hierarchy Orbis
Control. Chat transcripts и старые session summaries не являются project source
of truth.

## Быстрый вход

После корневого [`AGENTS.md`](../AGENTS.md) новая development session читает:

1. [`product.md`](product.md) — что строим и какой user outcome нужен;
2. [`current-state.md`](current-state.md) — что реально существует сейчас;
3. [`architecture.md`](architecture.md) — текущие boundaries и invariants;
4. [`verification.md`](verification.md) — какой evidence нужен для claims;
5. [`roadmap.md`](roadmap.md) — следующий порядок работ;
6. релевантный [ADR](adr/) или hardware evidence только по задаче.

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
| [`product.md`](product.md) | PRODUCT INTENT | User outcomes, invariants и non-goals |
| [`current-state.md`](current-state.md) | CURRENT STATUS | Operational baseline текущего HEAD |
| [`architecture.md`](architecture.md) | CURRENT DESIGN | Boundaries, semantics, safety contracts |
| [`verification.md`](verification.md) | VERIFICATION CONTRACT | Evidence levels, profiles и claims policy |
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
  hardwared boundaries (положение о «hardwared не создаётся» частично
  superseded ADR 0006/0007/0008);
- [`0003-gpu-provider-strategy.md`](adr/0003-gpu-provider-strategy.md) — GPU
  concepts и backend strategy;
- [`0004-authoritative-read-only-session.md`](adr/0004-authoritative-read-only-session.md)
  — read-only session semantics, unknown bounds, no-cache reads и lifecycle;
- [`0005-split-gpu-provider-capabilities.md`](adr/0005-split-gpu-provider-capabilities.md)
  — split GPU provider capabilities по hardware concepts;
- [`0006-privileged-performance-write.md`](adr/0006-privileged-performance-write.md)
  — первый привилегированный write path (Performance profile) и введение
  `orbis-hardwared` в workspace;
- [`0007-battery-mutation-backend.md`](adr/0007-battery-mutation-backend.md)
  — Battery mutation backend через asusd (COMPLETED / LIVE-VALIDATED);
- [`0008-supergfxd-staged-gpu-mutation.md`](adr/0008-supergfxd-staged-gpu-mutation.md)
  — staged GPU mutation contract через supergfxd; live mutation ещё не
  реализована;
- [`0009-native-asus-eco-backend.md`](adr/0009-native-asus-eco-backend.md)
  — native ASUS Eco read-only preflight foundation;
- [`0010-architecture-evolution.md`](adr/0010-architecture-evolution.md)
  — архитектурный verdict после source audit (KEEP/EVOLVE/REPLACE/DEFER).

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
