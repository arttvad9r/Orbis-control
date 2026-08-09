# Roadmap

> Роль: **FUTURE PLAN**. Roadmap задаёт архитектурные milestones без сроков.
> Фактическая готовность — в [`current-state.md`](current-state.md).

## Ordering principles

- Сначала read-only integration и capability evidence, затем writes.
- Mock backend сохраняется для deterministic tests и offscreen rendering.
- Каждый milestone должен завершаться честным current-state update.
- Hardware-specific implementation не переносится в UI/domain.
- Новый privileged component не создаётся без отдельного доказанного use case.

## Milestone 1 — Real read-only Battery state in GUI

**Goal:** подключить интерактивный GUI к существующему
`orbis-session-client → sessiond → UPower` path, сохранив mock для tests и
offscreen screenshots.

**Why:** daemon/client vertical slice уже live-validated, но пользовательский UI
по-прежнему показывает mock state. Это ближайший подтверждённый integration gap.

**Entry conditions:**

- Session1 getter, client conversion и sessiond lifecycle green;
- Nix user service live-validated;
- authoritative UI worker/application invariants сохранены.

**Definition of done:**

- running GUI открывает session connection вне session-client constructors;
- Battery current/enabled приходят из sessiond;
- отсутствующие bounds остаются unknown, без подстановки 40/100/5 как hardware;
- read-only backend не предлагает успешную mutation;
- mock остаётся для unit tests и `--screenshot`;
- backend unavailable/invalid payload отображаются как явный status/error;
- targeted UI/client tests и relevant workspace checks green.

**Risks/dependencies:** текущий worker generic требует одного provider с
Performance+GPU+Battery; нужен минимальный composition/backend-selection design,
не превращающий real Battery и mock Performance/GPU в ложный production backend.

## Milestone 2 — Runtime capability discovery and read-only status

**Goal:** превратить fixture-oriented capability model в runtime discovery для
реально подключённых backends, начиная с уже используемых battery/session facts.

**Why:** UI не должен предполагать функции по профилю mock или модели устройства.

**Entry conditions:** Milestone 1 даёт рабочую error/status boundary.

**Definition of done:**

- runtime report различает Supported/ReadOnly/Unsupported/PermissionDenied/Unknown;
- backend identity, endpoint и reason доступны UI/diagnostics;
- stale/missing services не приводят к fabricated support;
- hardware fixtures остаются regression evidence, а не runtime truth;
- нет writes.

**Risks/dependencies:** version drift asusd, multiple batteries, conflict между
backend presence и фактической readable capability.

## Milestone 3 — Real read-only ASUS providers

**Goal:** добавить доказанные read-only providers для приоритетных user-visible
areas: Performance, GPU concepts, fan/telemetry и доступные ASUS properties.

**Why:** расширить production visibility до write design и проверить provider
selection на реальном hardware.

**Entry conditions:** capability discovery умеет честно классифицировать
отсутствующие/ошибочные endpoints; имеются dated probes для target backend/version.

**Definition of done:**

- asusd/supergfxd/sysfs используются только там, где evidence подтверждает
  semantics;
- physical MUX, access, power и pending state не объединены;
- raw enum/units mappings покрыты fixtures/tests;
- reads не будят dGPU без необходимости;
- unsupported sections скрыты или объяснены, без crashes.

**Risks/dependencies:** backend version differences, raw enum ambiguity,
несогласованные system services и incomplete hardware evidence.

## Milestone 4 — Controlled mutation foundations

**Goal:** реализовать первый узкий production write path только для операции с
доказанными capability, range/semantics и privilege boundary.

**Why:** read support не доказывает безопасность записи.

**Entry conditions:**

- read-only provider и capability proof stable;
- backend write API и permissions подтверждены dated evidence;
- error/status UX готов показывать partial/read-back failures;
- выбран один конкретный owner, отсутствует конфликтующий writer.

**Definition of done:**

- validation до write;
- typed error mapping;
- authoritative read-back;
- tests для unsupported, denied, invalid, backend failure и read-back failure;
- hardware operation opt-in и отдельно live-validated;
- никакого universal root helper.

**Risks/dependencies:** unsafe guessed ranges, firmware-latched behavior,
privilege escalation, conflict с asusd/system policy.

## Milestone 5 — Persistence and automation semantics

**Goal:** определить, что является user intent, confirmed hardware state и
pending state; подключить versioned config без ложного applied state.

**Why:** persistent settings до стабильных capability/write semantics могут
повторно применять неподдерживаемые или опасные значения.

**Entry conditions:** хотя бы один controlled mutation path и стабильная
authoritative state model.

**Definition of done:**

- миграции/backup/atomic write покрыты integration tests;
- unsupported values не сохраняются как applied;
- startup/resume automation сравнивает current state перед действием;
- ownership/conflict semantics явны;
- no polling/retry loops, маскирующих races.

**Risks/dependencies:** stale config после backend/hardware change, automation
conflicts, resume lifecycle.

## Milestone 6 — Error/status UX and diagnostics

**Goal:** сделать degraded/unknown/read-only state понятным без обращения к
логам.

**Entry conditions:** runtime capability/error data существует.

**Definition of done:**

- section-level loading/error/read-only states;
- actionable backend/reason details;
- pending/reboot/logout state не выглядит applied;
- anonymized diagnostics export;
- CLI получает read-only status/diagnostics вместо stub.

**Risks/dependencies:** утечка hardware identifiers, слишком общие errors,
расхождение GUI и CLI semantics.

## Milestone 7 — Packaging and installation maturity

**Goal:** перейти от validated Nix development package к устойчивой установке и
обновлению.

**Entry conditions:** real GUI/session integration и service lifecycle stable.

**Definition of done:**

- persistent NixOS enablement documented and tested;
- dead module options либо реализованы, либо удалены отдельным change;
- D-Bus/service activation policy определена;
- desktop/AppStream assets и uninstall behavior готовы;
- non-Nix packaging добавляется только с reproducible checks.

**Risks/dependencies:** user-session target lifecycle, package/backend version
compatibility, duplicated service ownership.

## Milestone 8 — Broader hardware support

**Goal:** расширять support по evidence-driven device profiles, не по общим ASUS
предположениям.

**Entry conditions:** probe format и privacy process stable; providers умеют
возвращать Unknown честно.

**Definition of done:**

- каждый новый device profile имеет dated read-only evidence;
- differences покрыты fixtures/mapping tests;
- unknown constraints не заменяются defaults;
- hardware writes для новой модели проходят отдельную validation.

**Risks/dependencies:** firmware/kernel/backend drift, privacy of probe exports,
малое количество доступных test devices.
