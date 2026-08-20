# Historical Documentation

> Роль: **HISTORICAL INDEX**. Этот файл отделяет dated audits, промежуточные
> remediation plans и implementation snapshots от текущего source of truth.
>
> Для текущего состояния всегда начинайте с [`current-state.md`](current-state.md),
> затем сверяйтесь с production code, [`architecture.md`](architecture.md) и
> [`verification.md`](verification.md).

## Правило использования

Исторический документ фиксирует факты, предположения и решения на конкретный
момент времени. Он полезен для provenance и объяснения того, почему появился
определённый fix, но **не должен использоваться как описание текущего HEAD без
повторной проверки**.

Не обновляйте старые snapshots так, чтобы они выглядели как будто были написаны
про сегодняшнее состояние. Допустимо добавить в начало короткий supersession
banner, не переписывая исходный historical body. Если старое утверждение больше
не верно, фиксируйте актуальный факт в `current-state.md`.

## Dated audit snapshots

| Документ | Историческая роль | Известная supersession / оговорка |
|---|---|---|
| [`cloud-backlog-consolidation-2026-08-19.md`](cloud-backlog-consolidation-2026-08-19.md) | Снимок cloud-safe backlog и PR-состояния | PR topology и часть приоритетов уже изменились; используйте open Issues + `roadmap.md` |
| [`dependency-audit-2026-08-19.md`](dependency-audit-2026-08-19.md) | Dependency/toolchain audit | Упоминания Rust 1.85 устарели: workspace/toolchain сейчас закреплены на Rust 1.87 |
| [`privacy-audit-2026-08-19.md`](privacy-audit-2026-08-19.md) | Privacy/logging audit | Preferences warning payload redaction уже внедрена: production `Debug` выдаёт фиксированный `log_category()` |
| [`security-boundary-audit-2026-08-19.md`](security-boundary-audit-2026-08-19.md) | Privilege/polkit snapshot | Текущая production mutation surface уже: Performance и условно Battery; GPU/Fan/Panel/Keyboard/Aura fail-closed/default-deny |
| [`test-gap-audit-2026-08-19.md`](test-gap-audit-2026-08-19.md) | Test-gap snapshot | Toolchain blocker Rust 1.85 больше не актуален; executable GitHub Actions остаётся отдельным blocker #106 |

Для этих пяти snapshots добавлены supersession banners. Их original body
намеренно сохранён: historical claims внутри body следует читать в контексте
даты и banner, а не как current instructions.

## Historical implementation / remediation snapshots

Следующие документы сохраняются как engineering record. Они могут содержать
детали уже интегрированных или частично superseded этапов и не заменяют
`current-state.md`/`roadmap.md`:

- [`production-boundary-audit.md`](production-boundary-audit.md)
- [`production-hardening-checklist.md`](production-hardening-checklist.md)
- [`production-hardening-next-steps.md`](production-hardening-next-steps.md)
- [`production-hardening-status.md`](production-hardening-status.md)
- [`beta-readiness-plan.md`](beta-readiness-plan.md)
- [`nixos-production-deployment-audit.md`](nixos-production-deployment-audit.md)
- [`nixos-deployment-remediation-plan.md`](nixos-deployment-remediation-plan.md)
- [`fan-profile-read-remediation.md`](fan-profile-read-remediation.md)
- [`preferences-persistence-design.md`](preferences-persistence-design.md)
- [`diagnostics-backend-design.md`](diagnostics-backend-design.md)
- [`ui-backend-wiring-matrix.md`](ui-backend-wiring-matrix.md)

Если один из этих документов снова становится активным implementation plan,
его актуальные задачи должны быть отражены в GitHub Issues и `roadmap.md`, а не
только внутри длинного design-файла.

## Historical hardware / research evidence

Эти материалы не являются current implementation status, но сохраняют
revision/date-scoped evidence и research context:

- [`research-report.md`](research-report.md)
- [`feature-matrix.md`](feature-matrix.md)
- [`multi-model-discovery-research.md`](multi-model-discovery-research.md)
- [`power-limit-readiness-audit.md`](power-limit-readiness-audit.md)
- [`extended-asus-controls-readiness.md`](extended-asus-controls-readiness.md)
- [`research/`](research/)
- [`support-matrix.examples/`](support-matrix.examples/)
- hardware fixtures under [`../tests/fixtures/hardware/`](../tests/fixtures/hardware/)

Live hardware evidence остаётся revision-scoped. Старый успешный probe не
доказывает поддержку на другом устройстве или после изменения relevant code.

## Что не хранить в документации репозитория

- chat/session transcripts;
- локальные absolute paths и Nix store paths без необходимости для immutable
  release evidence;
- одноразовые generated archives рядом с ADR;
- секреты, токены, приватные runtime dumps;
- временные validation instructions, которые должны жить в PR/Issue discussion.

Accepted ADR хранятся только как reviewable Markdown в [`adr/`](adr/). Binary
archive с копиями ADR не является частью документационного контракта.
