# ADR 0010: Architecture Evolution after Source Audit

- Статус: **Принято** (2026-08-17)
- Дата: 2026-08-17

## Context

После первого полного source-level аудита и архитектурного ревью (2026-08-14)
необходимо привести документацию в соответствие с фактически существующей
реализацией и зафиксировать принятое направление дальнейшей эволюции. Этот ADR
не переписывает существующие ADR и не меняет runtime-код; он фиксирует текущее
состояние архитектуры и future direction.

Source audit выявил ключевые факты:

- `orbis-hardwared` **входит** в workspace members (добавлен после ADR 0006),
  но `AGENTS.md` утверждал обратное;
- Battery mutation **реализован** (ADR 0007, LIVE-VALIDATED), но ряд документов
  содержал устаревшие stale claims;
- Architecture в целом соответствует современному kernel-first/capability-driven
  подходу; основные пробелы — в объёме реализованных hardware-функций.

## Decision

### KEEP — действующие invariants

Следующие решения остаются фундаментальными и не пересматриваются:

1. **Rust + Slint** (ADR 0001).
2. **normal-user GUI** — GUI работает от обычного пользователя, не выполняет
   direct hardware I/O (ADR 0001, 0002).
3. **`orbis-sessiond` как непривилегированный user daemon** (ADR 0002).
4. **Узкий `orbis-hardwared`** — в workspace (после ADR 0006), sandboxed, polkit-
   авторизованный; каждая privileged capability добавляется отдельно по evidence.
5. **typed D-Bus boundaries** — Session1 (getter-only), Hardware1 (mutation).
6. **direct original caller → Hardware1** для privileged interactive mutations;
   polkit authorization по original `system-bus-name`; `sessiond` не является
   mutation deputy (confused-deputy protection, ADR 0006 amendment).
7. ** отсутствие mutation delegation через Session1** в production.
8. **Authoritative read-back** после каждой mutation; отсутствие optimistic
   hardware state.
9. **Distinction Unknown / Unsupported / Unavailable / ReadOnly / PermissionDenied**
   — честные error classes без симуляции успеха.
10. **capability-driven architecture** — поддержка доказывается probe/read, а не
    предполагается.
11. **GPU concepts остаются независимыми**: product/requested policy, physical MUX,
    access policy, runtime power, pending/action requirement — не объединять.

### EVOLVE — требуют дальнейшего архитектурного развития

1. **Runtime Capability Registry** — существующий `orbis-capabilities` пока не
   является полноценным runtime discovery system. Capabilities должны строиться из
   фактических providers/probes; read/write/constraints/backend/requirements должны
   моделироваться отдельно.

2. **Application/worker composition** — на момент принятия ADR текущий
   `run_worker` принимал слишком много независимых services (main, battery,
   gpu_power, gpu_mux, gpu_access, performance). Этот historical composition
   debt устранён в Task 2.1; дальнейшее добавление telemetry/fans/display/etc.
   должно сохранять capability-domain grouping.

3. **GPU architecture** — concept-specific provider traits являются целевым
   направлением; legacy monolithic `GpuProvider` остаётся временным migration
   artifact; product `GpuMode` должен в будущем быть policy layer, а не raw
   hardware/backend enum.

4. **State architecture** — в дальнейшем требуется явное разделение
   `ObservedState` / `DesiredState` / `PendingState` / `CapabilityState`.

5. **Telemetry** — authoritative configuration state и telemetry имеют разные
   semantics; configuration reads остаются fresh/authoritative; telemetry в
   будущем может использовать timestamped samples/subscriptions; telemetry cache
   не должен трактоваться как authoritative configuration cache.

6. **Provider selection** — не фиксировать один глобальный порядок backend для
   всего приложения; ownership/provider priority определяется **per capability**:

   - Performance → kernel `platform_profile`
   - Battery configured threshold → asusd
   - Battery effective threshold → kernel `power_supply`
   - Battery general telemetry → UPower
   - GPU staged lifecycle → supergfxd compatibility backend

   Одна product capability может использовать несколько authoritative sources для
   разных semantics.

### REPLACE / REMOVE LATER

Следующие пункты остаются future direction:

- legacy `GpuProvider` должен быть завершённой миграцией заменён
  capability-specific interfaces;
- production composition dependency на `MockProvider` устранена в Task 3;
  `MockProvider` остаётся для tests/offscreen/deterministic scenarios;
- `ApplicationRuntime` должен эволюционировать вместе с новыми capability
  domains, не возвращаясь к flat positional service injection;
- dead module options и устаревшие helpers должны быть удалены отдельными
   mechanical tasks после проверки usages.

### Status amendment — application composition (Task 2.1)

Application Composition Refactor завершён: production worker теперь получает
один `ApplicationRuntime`; construction knowledge вынесено из `main.rs` в
composition module. `ApplicationRuntime` группирует capability-facing services,
включая `GpuPrimitiveServices`, Battery и Performance. Этот EVOLVE item имеет
статус **RESOLVED / COMPLETED** для текущих capabilities; это не означает, что
application architecture больше не будет расширяться.

### Status amendment — production Mock isolation (Task 3)

Task 3 завершил mechanical cleanup production composition: `ApplicationRuntime`
использует `GpuPrimitiveServices` с независимыми `GpuPowerProvider`,
`GpuMuxProvider` и `GpuAccessProvider`; `MockProvider` больше не создаётся в
production runtime. Product GPU mode без доказанного backend возвращает
`Unsupported`/`Unavailable`.

Это не реализует `GpuProductPolicy`, product mapping или GPU mutation. Legacy
`GpuProvider` и `MockProvider` сохраняются для tests, deterministic fixtures и
offscreen scenarios.

### DEFER

Не объявлять готовыми и не проектировать write implementation без evidence:

- power limits;
- окончательный product GPU Eco/Standard/Ultimate/Optimized mapping;
- live GPU mutation;
- automation privilege semantics.

## Consequences

- Документация (AGENTS.md, docs/README.md, docs/architecture.md,
  docs/roadmap.md, docs/threat-model.md, docs/provider-matrix.md) актуализирована
  в соответствии с фактической реализацией.
- ADR 0002 помечен как partially superseded (положение о «hardwared не
  создаётся» заменено ADR 0006/0007/0008).
- Future direction зафиксирован в architecture.md (§11) и roadmap.md
  (Architecture consolidation).
- Runtime-код, поведение приложения и D-Bus wire API **не изменяются**.

## Status

**Accepted.** Фиксирует результат source audit 2026-08-14 и текущее
архитектурное состояние. Не пересматривает существующие ADR и не вводит новых
требований к коду.
