# ADR 0007: Battery Mutation Backend

- Статус: **Принято как архитектурный контракт; backend LIVE-VALIDATED**
- Дата: 2026-08-13

## Context

ASUS `asusd` владеет battery charge threshold: его typed D-Bus API пишет
kernel backend и сохраняет значение в `/etc/asusd/asusd.ron`. `asusd` также
повторно применяет значение при startup, resume, power events, config reload и
shutdown. Поэтому прямой Orbis write в
`/sys/class/power_supply/*/charge_control_end_threshold` при активном `asusd`
создаёт competing-owner risk.

Исследованный compatibility API:

```text
bus:       xyz.ljones.Asusd                 (system bus)
path:      /xyz/ljones
interface: xyz.ljones.Platform
property:  ChargeControlEndThreshold       (u8)
get:       ChargeControlEndThreshold() -> u8
set:       ChargeControlEndThreshold(u8) -> ()
```

Setter `asusd` принимает `20..=100`, пишет через ASUS power backend, затем
обновляет и сохраняет configured value. `100` в этом API является обычным
threshold value; отдельная `OneShotFullCharge` operation временно использует
100 и не доказывает, что 100 означает disabled.

`asusd` — root system-bus daemon. В просмотренном Battery path нет отдельной
polkit-проверки или caller-identity check внутри `asusd`; authorization
происходит на system-bus/service policy boundary. Без отдельного E2E
проверочного вызова нельзя считать, что обычный local user вправе вызвать
setter напрямую.

## Decision

1. Production Battery mutation backend и GUI Battery mutation control реализованы
   и live-validated.
2. Compatibility backend называется концептуально
   `AsusdBatteryMutationBackend` и вызывает только typed D-Bus API `asusd`.
3. Shell `asusctl` не является runtime dependency и не используется как
   backend.
4. Выбирается caller-preserving path:

   ```text
   original application caller
     → io.github.orbiscontrol.Hardware1
     → Orbis polkit authorization
     → orbis-hardwared
     → typed xyz.ljones.Platform D-Bus setter
     → asusd
     → ASUS kernel backend + asusd persistence/restore
   ```

   Прямой Application → asusd path отклонён: он обходит единый Orbis typed
   authorization model. Делегация через `orbis-sessiond` также запрещена:
   второй D-Bus hop теряет original caller identity.
5. Hardware1 method должен быть узким semantic method, не generic D-Bus relay:

   ```text
   SetChargeLimit(percent: u8) -> typed mutation result
   ```

   Contract: integer `20..=100`, step `1`; значения вне contract отвергаются
   до вызова `asusd`. Никаких path/interface/member, shell strings или
   arbitrary sysfs arguments.
6. `SetChargeLimit` означает только установку configured threshold через
   asusd. Он не означает disable. Отдельная enable/disable operation в
   Orbis появится только после отдельного доказательства semantics; нельзя
   кодировать disable через `100`.
7. Domain/UI остаются backend-neutral: `enabled`, `configured_percent`,
   `effective_percent` и `bounds` не меняются из-за выбора asusd backend.

## Authoritative success and states

`asusd` method returning OK — только подтверждение принятия backend operation,
не окончательный Applied result. После setter необходимы fresh reads:

1. asusd configured threshold;
2. kernel effective threshold через обнаруженный native power-supply path;
3. Session1 `ChargeLimit` read model.

Acceptance:

- **Applied** — configured read-back соответствует requested percent, effective
  read успешен, и Session1 отражает те же authoritative values;
- **Pending** — asusd принял операцию, но reads ещё не достигли согласованного
  состояния; optimistic UI state запрещён;
- **Failed** — transport/error/unsupported/range/kernel/persistence error либо
  окончательный read-back mismatch;
- configured/effective могут различаться легитимно, поэтому различие само по
  себе не означает failure; configured обязан совпасть с requested.

При любой ошибке Orbis не пытается компенсировать состояние direct sysfs
write. Единственным owner остаётся `asusd`.

## Failure and ownership semantics

- `asusd` unavailable — `Unavailable`/backend error, без fallback write;
- D-Bus transport failure — preserve error, no retry через другой writer;
- unsupported property — `Unsupported`;
- range rejection — `InvalidRequest` до mutation или typed asusd error;
- kernel write failure внутри asusd — backend failure;
- persistence failure — failure, даже если kernel value уже изменился;
- post-write mismatch — failure/diagnostic, не optimistic success;
- asusd restart during mutation — `Pending` только при bounded verifiable
  recovery; иначе `Failed`, затем fresh read; competing writes запрещены.

## Future native backend boundary

Позже `AsusdBatteryMutationBackend` может быть заменён на
`NativeAsusBatteryMutationBackend` без изменения domain, GUI, WorkerCommand,
AppService semantics или Session1 read model. Native backend обязан доказать и
реализовать отдельно:

- kernel write и exact validation;
- configured-value persistence;
- startup restore;
- resume/power-event restore;
- external-change reconciliation;
- single-owner policy при наличии asusd/UPower;
- fresh kernel/configured/effective read-back;
- failure and partial-state reporting.

До выполнения этих обязанностей direct native write при активном asusd
запрещён.

## Live validation

Controlled production cycle `100 → 80 → 100` через `Hardware1.SetChargeLimit`
успешно подтвердил asusd configured и kernel effective read-back; финальный
hardware state совпал с начальным. Session1 после rollback показал
`configured/effective=100/100`. UPower `ChargeEndThreshold=80` не является
authoritative configured value: `enabled` читается из UPower, configured — из
asusd, effective — из kernel.
Production GUI использует slider `20..=100`, step `1`; один drag даёт максимум
один commit на release. Hardware1 accounting подтвердил ровно sequence
`[80, 100]`, total `2`, без других Battery values и retries.

## Consequences

- `asusd` — compatibility backend, не permanent domain/UI dependency.
- Battery mutation использует отдельную узкую capability в hardwared и отдельную
  authorization action, как Performance в ADR 0006.
- UPower остаётся generic read/policy integration, но не смешивается с
  asusd-owned configured state без явной ownership detection.
- Battery backend и Battery GUI mutation объявляются **COMPLETED / LIVE-VALIDATED**.
