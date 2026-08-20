# Documentation Index

Этот индекс определяет source-of-truth hierarchy документации Orbis Control.
Chat/session transcripts, одноразовые validation instructions и старые planning
snapshots не являются project source of truth.

## Быстрый вход

После корневого [`AGENTS.md`](../AGENTS.md) новая development session читает:

1. [`product.md`](product.md) — продуктовая цель, user outcomes и non-goals;
2. [`current-state.md`](current-state.md) — что реально существует сейчас;
3. [`architecture.md`](architecture.md) — текущие boundaries и invariants;
4. [`verification.md`](verification.md) — какой evidence нужен для claims;
5. [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md) — уровни release evidence;
6. [`roadmap.md`](roadmap.md) — порядок незавершённой работы;
7. релевантный [ADR](adr/) или hardware evidence только по конкретной задаче.

Если найден dated audit, старый remediation plan или implementation snapshot,
сначала проверьте [`history.md`](history.md): такие документы сохраняются для
provenance и не должны переопределять current state.

## Source-of-truth hierarchy

### Current implementation facts

1. Production code и tests.
2. [`current-state.md`](current-state.md).

Открытый Issue/PR описывает planned или proposed work и не становится current
behavior до интеграции. Source inspection может доказать `IMPLEMENTED`, но не
`TESTED`, `PACKAGED` или `LIVE-VALIDATED` без соответствующего execution evidence.

### Architectural intent

1. Accepted ADR.
2. [`architecture.md`](architecture.md).
3. [`threat-model.md`](threat-model.md) для security assumptions/mitigations.

### Hardware facts

1. Dated live hardware audit / immutable probe evidence.
2. Hardware fixtures в [`tests/fixtures/hardware/`](../tests/fixtures/hardware/).
3. Historical research reports.

Inference, product defaults и mock values не становятся hardware facts. При
конфликте current code не переписывает прошлое hardware snapshot: новый probe
создаёт новый dated evidence artifact.

### Future work

[`roadmap.md`](roadmap.md) + GitHub Issues. Длинный design/remediation document не
должен быть единственным местом, где существует незавершённая задача.

### Historical records

[`history.md`](history.md) классифицирует dated audits, superseded plans и
historical evidence. Они сохраняются как engineering record, но не являются
описанием текущего HEAD.

### Agent rules

Корневой [`AGENTS.md`](../AGENTS.md).

## Canonical documents

| Документ | Роль | Что считать authoritative |
|---|---|---|
| [`product.md`](product.md) | PRODUCT INTENT | User outcomes, invariants, non-goals |
| [`current-state.md`](current-state.md) | CURRENT STATUS | Operational baseline текущей production-линии |
| [`architecture.md`](architecture.md) | CURRENT DESIGN | Boundaries, semantics, safety contracts |
| [`verification.md`](verification.md) | VERIFICATION CONTRACT | Evidence profiles и claims policy |
| [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md) | RELEASE EVIDENCE | `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN` |
| [`roadmap.md`](roadmap.md) | FUTURE PLAN | Milestones и порядок работ |
| [`threat-model.md`](threat-model.md) | SECURITY MODEL | Threats, trust boundaries, mitigations |
| [`support-matrix-schema.md`](support-matrix-schema.md) | SUPPORT CONTRACT | Формат read/write evidence matrix |
| [`provider-matrix.md`](provider-matrix.md) | PROVIDER DESIGN | Provider strategy; не implementation-status substitute |
| [`history.md`](history.md) | HISTORICAL INDEX | Где старые документы расходятся с current truth |
| [`adr/`](adr/) | ARCHITECTURAL DECISIONS | Accepted/reviewable Markdown ADR |

## Supporting references

- [`feature-matrix.md`](feature-matrix.md) — historical Stage 0 Linux feasibility snapshot;
- [`research-report.md`](research-report.md) — dated hardware/research evidence;
- [`ui-reference.md`](ui-reference.md) и [`ui-measurements.json`](ui-measurements.json) — UI reference material;
- [`ci-validation-matrix.md`](ci-validation-matrix.md) — intended CI coverage, но не evidence того, что Actions действительно выполнился;
- [`beta-acceptance-checklist.md`](beta-acceptance-checklist.md) — release acceptance checklist.

Остальные крупные design/remediation документы классифицированы в
[`history.md`](history.md), если они описывают завершённый или superseded этап.

## Accepted ADR

- [`0001-rust-and-slint.md`](adr/0001-rust-and-slint.md) — Rust + Slint;
- [`0002-daemon-boundaries.md`](adr/0002-daemon-boundaries.md) — исходные GUI/sessiond/hardwared boundaries; privileged-write часть уточнена более поздними ADR;
- [`0003-gpu-provider-strategy.md`](adr/0003-gpu-provider-strategy.md) — GPU concepts и backend strategy;
- [`0004-authoritative-read-only-session.md`](adr/0004-authoritative-read-only-session.md) — read-only session semantics;
- [`0005-split-gpu-provider-capabilities.md`](adr/0005-split-gpu-provider-capabilities.md) — split GPU capabilities;
- [`0006-privileged-performance-write.md`](adr/0006-privileged-performance-write.md) — privileged Performance path;
- [`0007-battery-mutation-backend.md`](adr/0007-battery-mutation-backend.md) — Battery mutation ownership;
- [`0008-supergfxd-staged-gpu-mutation.md`](adr/0008-supergfxd-staged-gpu-mutation.md) — staged GPU mutation contract;
- [`0009-native-asus-eco-backend.md`](adr/0009-native-asus-eco-backend.md) — native ASUS Eco preflight foundation;
- [`0010-architecture-evolution.md`](adr/0010-architecture-evolution.md) — architecture evolution verdict;
- [`0011-fan-curve-write-ownership.md`](adr/0011-fan-curve-write-ownership.md) — asusd ownership boundary для fan writes;
- [`0012-apply-result-accepted-semantics.md`](adr/0012-apply-result-accepted-semantics.md) — `Accepted != Applied`.

ADR directory содержит reviewable Markdown decisions. Binary archives и
дублирующие generated bundles рядом с ADR не являются частью документационного
контракта.

## Правила обновления

- Изменился architecture contract → обновить `architecture.md` и при значимом решении ADR.
- Завершён vertical slice или изменился release blocker → обновить `current-state.md`.
- Изменились приоритеты/очередность → обновить `roadmap.md`.
- Получен hardware probe → добавить новый dated snapshot; старый не ретушировать.
- Dated audit устарел → сохранить его как snapshot и описать supersession в `history.md`.
- Mechanical refactor → менять docs только если изменился visible state, contract или source-of-truth link.
- Feasibility не помечать `IMPLEMENTED` без production code.
- В репозиторий не добавлять chat/session transcripts, локальные handoff dumps, секреты или одноразовые validation archives.
