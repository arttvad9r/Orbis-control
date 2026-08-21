# Documentation Index

Этот индекс определяет source-of-truth hierarchy документации Orbis Control. Chat/session transcripts, одноразовые validation triggers и старые planning snapshots не являются current project source of truth.

## Быстрый вход

После [`AGENTS.md`](../AGENTS.md) новая development session читает:

1. [`product.md`](product.md) — product intent и non-goals;
2. [`current-state.md`](current-state.md) — фактический source status активной integration line;
3. [`architecture.md`](architecture.md) — boundaries/invariants;
4. [`backend-completion-status.md`](backend-completion-status.md) — concise UI/backend connection summary;
5. [`verification.md`](verification.md) — evidence contract;
6. [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md) — допустимые claim levels;
7. [`roadmap.md`](roadmap.md) — текущая очередь незавершённой работы;
8. релевантный [ADR](adr/) или dated hardware evidence по конкретной задаче.

Если найден dated audit, старый remediation plan или implementation snapshot, сначала смотрите [`history.md`](history.md). Historical document сохраняет provenance и не переопределяет current code/status.

## Current branch vs release baseline

На 2026-08-21 активная интеграционная ветка — `development`; она консолидирует прежнюю линию `asus-hardware-validation-20260821` (head PR #129, `e8b611e`) и содержит более новый UI/runtime/backend слой, чем `main`. `current-state.md`, `architecture.md`, `backend-completion-status.md`, `roadmap.md` и `beta-acceptance-checklist.md` описывают именно этот source snapshot и явно отделяют его от executable/release evidence.

До merge `main` остаётся последней консолидированной release baseline. Наличие source work в integration branch не делает его автоматически `TESTED`/`PACKAGED`/`LIVE-VALIDATED`.

## Source-of-truth hierarchy

### Current implementation facts

1. Production source code and tests of the named revision.
2. [`current-state.md`](current-state.md).
3. [`backend-completion-status.md`](backend-completion-status.md) for concise UI/backend wiring status.

Issue/PR describes work tracking. Source inspection proves at most `IMPLEMENTED`; stronger claims require execution evidence.

### Architectural intent

1. Accepted ADR.
2. [`architecture.md`](architecture.md).
3. [`threat-model.md`](threat-model.md) for security assumptions/mitigations.

### Hardware facts

1. Dated live hardware audit / immutable probe evidence.
2. Hardware fixtures in [`tests/fixtures/hardware/`](../tests/fixtures/hardware/).
3. Historical research reports.

Model names, mock values and product defaults are never hardware proof. A new live probe creates new dated evidence; do not rewrite old evidence to fit current code.

### Future work

[`roadmap.md`](roadmap.md) + GitHub Issues. A long design/remediation document must not be the only place an unfinished task exists.

### Release acceptance

[`beta-acceptance-checklist.md`](beta-acceptance-checklist.md) is the current beta gate. [`beta-readiness-plan.md`](beta-readiness-plan.md) is historical and is indexed by `history.md`.

### Historical records

[`history.md`](history.md) classifies dated audits, superseded plans and historical evidence. They are engineering records, not descriptions of current HEAD.

## Canonical documents

| Document | Role | Authoritative for |
|---|---|---|
| [`product.md`](product.md) | PRODUCT INTENT | User outcomes, invariants, non-goals |
| [`current-state.md`](current-state.md) | CURRENT STATUS | Current integration source status and blockers |
| [`architecture.md`](architecture.md) | CURRENT DESIGN | Boundaries, semantics, safety contracts |
| [`backend-completion-status.md`](backend-completion-status.md) | BACKEND STATUS | Concise connected/blocked UI/backend surfaces |
| [`verification.md`](verification.md) | VERIFICATION CONTRACT | Evidence profiles and claims policy |
| [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md) | RELEASE EVIDENCE | `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN` |
| [`roadmap.md`](roadmap.md) | FUTURE PLAN | Milestones and work ordering |
| [`beta-acceptance-checklist.md`](beta-acceptance-checklist.md) | RELEASE GATE | Current beta acceptance gates |
| [`threat-model.md`](threat-model.md) | SECURITY MODEL | Threats, trust boundaries, mitigations |
| [`support-matrix-schema.md`](support-matrix-schema.md) | SUPPORT CONTRACT | Read/write evidence matrix format |
| [`provider-matrix.md`](provider-matrix.md) | PROVIDER DESIGN | Provider strategy, not implementation-status substitute |
| [`history.md`](history.md) | HISTORICAL INDEX | Superseded/historical material classification |
| [`adr/`](adr/) | ARCHITECTURAL DECISIONS | Accepted/reviewable decisions |

## Supporting references

- [`ci-validation-matrix.md`](ci-validation-matrix.md) — intended CI coverage, not proof that GitHub Actions ran;
- [`research-report.md`](research-report.md) — dated research/hardware context;
- [`feature-matrix.md`](feature-matrix.md) — historical feasibility snapshot;
- [`ui-reference.md`](ui-reference.md) and [`ui-measurements.json`](ui-measurements.json) — UI reference material;
- [`history.md`](history.md) — classification for large older remediation/design documents.

## Accepted ADR

- [`0001-rust-and-slint.md`](adr/0001-rust-and-slint.md)
- [`0002-daemon-boundaries.md`](adr/0002-daemon-boundaries.md)
- [`0003-gpu-provider-strategy.md`](adr/0003-gpu-provider-strategy.md)
- [`0004-authoritative-read-only-session.md`](adr/0004-authoritative-read-only-session.md)
- [`0005-split-gpu-provider-capabilities.md`](adr/0005-split-gpu-provider-capabilities.md)
- [`0006-privileged-performance-write.md`](adr/0006-privileged-performance-write.md)
- [`0007-battery-mutation-backend.md`](adr/0007-battery-mutation-backend.md)
- [`0008-supergfxd-staged-gpu-mutation.md`](adr/0008-supergfxd-staged-gpu-mutation.md)
- [`0009-native-asus-eco-backend.md`](adr/0009-native-asus-eco-backend.md)
- [`0010-architecture-evolution.md`](adr/0010-architecture-evolution.md)
- [`0011-fan-curve-write-ownership.md`](adr/0011-fan-curve-write-ownership.md)
- [`0012-apply-result-accepted-semantics.md`](adr/0012-apply-result-accepted-semantics.md)

## Update rules

- architecture contract changed → update `architecture.md`, and ADR when a stable decision changed;
- production source/status or blocker changed → update `current-state.md` + `backend-completion-status.md` where relevant;
- ordering changed → update `roadmap.md`;
- release acceptance changed → update `beta-acceptance-checklist.md`;
- hardware probe added → add new dated evidence, never retouch old live evidence;
- historical audit became stale → keep body historical and record supersession in `history.md`;
- do not mark feasibility as `IMPLEMENTED` without production code;
- do not mark source as `TESTED` without executable evidence;
- do not commit chat/session transcripts, secrets, private runtime dumps, generated ADR bundles or one-off validation trigger files.
