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

**Status: COMPLETED / LIVE-VALIDATED** (2026-08-09).

**Goal:** подключить интерактивный GUI к существующему
`orbis-session-client → sessiond → UPower` path, сохранив mock для tests и
offscreen screenshots.

**Why:** daemon/client vertical slice уже live-validated, но пользовательский UI
по-прежнему показывает mock state. Это ближайший подтверждённый integration gap.

**Entry conditions:**

- Session1 getter, client conversion и sessiond lifecycle green;
- Nix user service live-validated;
- authoritative UI worker/application invariants сохранены.

**Definition of done (выполнен):**

- running GUI открывает session connection вне session-client constructors;
- Battery current/enabled приходят из sessiond;
- отсутствующие bounds остаются unknown, без подстановки 40/100/5 как hardware;
- read-only backend не предлагает успешную mutation;
- mock остаётся для unit tests и `--screenshot`;
- backend unavailable/invalid payload отображаются как явный status/error;
- targeted UI/client tests и relevant workspace checks green;
- live-validated: daemon absent → Unavailable без mock fallback; daemon present →
  Ready со значением, совпадающим с authoritative D-Bus baseline; read-only
  slider behavior зафиксирован исторически, а production mutation control
  закрыт отдельным Battery GUI milestone.

**Result/notes:** split composition `run_worker<M, B, F>` реализован
(Performance/GPU → MockProvider; Battery → SessionChargeLimitProvider; один
sequential loop; FIFO/barriers/coalescing сохранены). Обнаружены два gap'а,
вынесены в отдельные milestones: packaging runtime correctness (LD_LIBRARY_PATH)
и GUI diagnostics (tracing initialization).

**Risks/dependencies (закрыты):** ранее worker generic требовал один provider с
Performance+GPU+Battery; split-дизайн устранил это требование.

## Milestone 2 — Packaging runtime correctness

**Status: COMPLETED / LIVE-VALIDATED** (2026-08-09).

**Goal:** packaged GUI должен запускаться напрямую без ручного
`LD_LIBRARY_PATH`.

**Why:** `nix build .#orbis-control` PASS, но при live запуске packaged GUI
требовал ручной LD_LIBRARY_PATH: runtime/dlopen библиотеки загружаются через
dlopen/libloading и не были обычными DT_NEEDED/RUNPATH dependencies.

**Entry conditions:**

- текущий GUI/session composition code green (Milestone 1);
- отсутствие несвязанных изменений.

**Definition of done (выполнен):**

- `nix build .#orbis-control` PASS;
- direct packaged `result/bin/orbis-control` startup;
- без ручного LD_LIBRARY_PATH;
- runtime/dlopen зависимости предоставляются декларативно (wrapper);
- live GUI startup повторно подтверждён (packaged GUI открывает окно без
  ручного окружения);
- `nix flake check` PASS, 273 Rust tests PASS;
- loader errors отсутствуют; Battery без daemon корректно показал Unavailable;
  Performance/GPU UI сохранён.

**Result/notes:** исправление — стандартный Nix `makeWrapper` с минимальным
declarative `LD_LIBRARY_PATH` для подтверждённого runtime set: wayland,
libxkbcommon, fontconfig, libglvnd. EGL предоставляется через vendor-neutral
`libglvnd` (Mesa driver не hard-coded). Обёрнут только `orbis-control`;
`orbis-sessiond` и `orbisctl` не обёрнуты. Runtime libraries находятся в Nix
closure.

## Milestone 3 — GUI diagnostics / tracing initialization

**Status: COMPLETED / LIVE-VALIDATED** (2026-08-09).

**Goal:** инициализировать tracing subscriber в GUI, чтобы существующие
`tracing::warn!`/`debug!` попадали в полезный runtime log.

**Why:** ранее GUI tracing не инициализирован; диагностические события
(включая `battery: refresh недоступен`) молча терялись — это затрудняло
операционную диагностику (например, отсутствие sessiond требовало visual-only
evidence).

**Entry conditions:** packaging runtime correctness закрыт (Milestone 2).

**Definition of done (выполнен):**

- production GUI инициализирует tracing subscriber;
- semantics `RUST_LOG`/EnvFilter определены;
- существующие `tracing::warn!` реально появляются в stderr/log;
- отсутствие sessiond можно диагностировать без visual-only evidence;
- duplicate/global subscriber initialization корректно обрабатывается;
- packaged GUI live validation подтверждает diagnostics.

**Result/notes:**

- default filter без `RUST_LOG` = `warn`;
- `RUST_LOG`/EnvFilter: `RUST_LOG=debug` live-validated;
- отсутствие sessiond видно в stderr: существующий Battery refresh WARN
  (ServiceUnknown) наблюдается в packaged GUI stderr;
- duplicate-safe initialization через non-panicking `try_init()`;
- packaged live validation passed (273 Rust tests, nix flake check, nix build).

## Milestone 4 — Real read-only ASUS providers

**Status: ACTIVE — read-only MVP COMPLETED / LIVE-VALIDATED; fan/telemetry pending.**

**Goal:** добавить доказанные read-only providers для приоритетных user-visible
areas: Performance, GPU concepts, fan/telemetry и доступные ASUS properties.

**Why:** расширить production visibility до write design и проверить provider
selection на реальном hardware.

**Entry conditions:** capability discovery умеет честно классифицировать
отсутствующие/ошибочные endpoints; имеются dated probes для target backend/version.

**Sequencing (внутри milestone, evidence-driven):**

1. Performance read-only provider — READ-ONLY AUDIT + минимальный production
   Performance provider — **COMPLETED / LIVE-VALIDATED**
   (`KernelPerformanceProvider`, symbolic kernel `platform_profile` ABI; live
   ignored integration test PASS); **Performance Session1 + GUI integration
   COMPLETED / LIVE-VALIDATED** (expose `KernelPerformanceProvider` через
   Session1/session-client → worker → GUI; current + available совпадают с raw
   kernel; без mock fallback; initial refresh only; controls read-only;
   Scenario A daemon absent → Unavailable; Scenario B packaged sessiond →
   Balanced/{Silent,Balanced,Turbo} — совпало с raw; никаких writes).
2. GPU concepts read-only providers — **COMPLETED / LIVE-VALIDATED** (GPU
   runtime power sub-concept, ADR 0005 split traits, MUX/access evidence +
   `ArmouryGpuProvider`, read-only Session1 GPU exposure, GUI read-only GPU
   integration: production GUI отображает Power/MUX/Access через
   session-client capability providers из одной session connection; без mock
   fallback; Scenario A daemon absent → три Unavailable; Scenario B packaged
   sessiond → Power=Active, MUX=Integrated, Access=Unblocked — совпало с raw
   supergfxd Power=0, sysfs mux=1, dgpu_disable=0; product
   Eco/Standard/Ultimate/Optimized path остаётся MOCK-ONLY в legacy, production
   controls disabled; никаких writes).
3. fan/other proven ASUS reads — pending.
4. telemetry только по доказанным источникам — pending.

Следующий ACTIVE substep (после read-only MVP):

**Performance controlled mutation — COMPLETED / LIVE-VALIDATED** (Milestone 5
foundations): узкий write path через Hardware1/hardwared, polkit, fixed kernel
writer и authoritative read-back доказан production GUI cycle. Незавершёнными
остаются Battery mutation, GPU product policy/mutation, fan/telemetry (substeps
3–4) и UX/polish (Milestone 7).

Milestone 4 целиком **НЕ закрывается**: fan/telemetry (substeps 3–4) остаются
pending; product GPU policy/mutation — pending.

Technical note: worker composition теперь имеет несколько independent services
(main, battery, gpu_power, gpu_mux, gpu_access, performance); это не blocker,
но дальнейшее бесконечное расширение `run_worker` может потребовать отдельного
composition refactor.

Pending остаётся: pending/action provider (отдельно: текущий `ActionRequirement`
относится к product requested mode и НЕ должен автоматически использоваться
для supergfxd `PendingUserAction`); mutation.

**Definition of done:**

- asusd/supergfxd/sysfs используются только там, где evidence подтверждает
  semantics;
- physical MUX, access, power и pending state не объединены;
- raw enum/units mappings покрыты fixtures/tests;
- reads не будят dGPU без необходимости;
- unsupported sections скрыты или объяснены, без crashes.

**Risks/dependencies:** backend version differences, raw enum ambiguity,
несогласованные system services и incomplete hardware evidence.

## Read-only MVP (достигнут)

**COMPLETED / LIVE-VALIDATED** (2026-08-09).

Production GUI реально показывает Battery Charge Limit, Performance
(current + available), GPU Power/MUX/Access через session path; все real
sections имеют честные Loading/Ready/Unavailable; без mock fallback; sessiond
absent → честный Unavailable. Все mutation controls в production
read-only/disabled. Это **не** означает завершённость Orbis в целом.

Следующие крупные направления остаются отдельными (привязка к milestones):

1. **Performance controlled mutation** (Milestone 5) — **COMPLETED /
   LIVE-VALIDATED**: [ADR 0006](adr/0006-privileged-performance-write.md),
   kernel `platform_profile` через узкий `orbis-hardwared`, polkit и
   authoritative read-back; production GUI `Balanced → Silent → Balanced`
   подтвердил ровно два valid calls/writes;
2. **Battery mutation** (Milestone 5) — **COMPLETED / LIVE-VALIDATED**:
   compatibility backend через единственного asusd owner, configured/effective/
   Session1 read-back и controlled GUI `100 → 80 → 100` подтверждены; exact
   Hardware1 sequence `[80, 100]`, total `2`, без retries. Архитектура зафиксирована
   в [ADR 0007](adr/0007-battery-mutation-backend.md);
3. **GPU product policy/mutation** (Milestone 5; product GpuMode backend
   mapping всё ещё NOT PROVEN);
4. **fan/telemetry** (Milestone 4, substeps 3–4; только по доказанным
   источникам);
5. **UX/polish** (Milestone 7 — error/status UX, CLI diagnostics).

## Milestone 5 — Controlled mutation foundations

**Status: ACTIVE — Performance и Battery substeps COMPLETED / LIVE-VALIDATED;
GPU mutation pending.**

**Goal:** реализовать первый узкий production write path только для операции с
доказанными capability, range/semantics и privilege boundary.

**Why:** read support не доказывает безопасность записи.

### Completed substep — Performance

- production deployment: generation 80, `orbis-hardwared` auto-start через
  `multi-user.target`, system D-Bus ownership и sandbox live-validated;
- GUI controls enabled только после Hardware1 owner probe;
- direct caller → Hardware1 → hardwared → polkit → fixed
  `platform_profile` write → hardwared read-back;
- `AppService` выполняет fresh Session1 post-write read-back;
- controlled live GUI validation PASS: ровно `wire 0` и `wire 1`, final state
  равен initial.

GPU mutation, fan control, real telemetry и broader UX polish не отмечаются как
completed и остаются отдельными следующими этапами. Battery GUI mutation
completed; отдельными pending scopes остаются GPU mutation, fan control,
telemetry и UX polish.

**Architecture:** принята в [ADR 0006](adr/0006-privileged-performance-write.md)
(Performance profile через kernel `platform_profile` + узкий `orbis-hardwared`;
GUI/sessiond unprivileged; polkit action; validate → один write → read-back →
success только при совпадении).

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

## Milestone 6 — Persistence and automation semantics

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

## Milestone 7 — Error/status UX and diagnostics

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

## Milestone 8 — Packaging and installation maturity

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

## Milestone 9 — Broader hardware support

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
