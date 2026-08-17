# Architecture — Orbis Control

> Роль: **CURRENT DESIGN**. Документ описывает архитектурный контракт текущего
> репозитория. Фактическая готовность функций приведена в
> [`current-state.md`](current-state.md), а будущий порядок работ — в
> [`roadmap.md`](roadmap.md).

## 1. Архитектурные принципы

- GUI работает от обычного пользователя и не выполняет direct hardware I/O.
- Системные детали скрыты за provider/service boundaries.
- Архитектура capability-driven: поддержка доказывается probe/read, а не
  предполагается по модели ноутбука, имени файла или наличию D-Bus object.
- Read-only capability не выдаётся за write capability.
- Значения после команд считаются подтверждёнными только после authoritative
  read-back.
- Неизвестные hardware-ограничения остаются неизвестными.
- `orbis-hardwared` — узкий production root-helper только для доказанных
  привилегированных операций (Performance, Battery); он не является generic
  sysfs writer и не generic hardware daemon.

## 2. Фактические слои и зависимости

```text
Slint callbacks / UI state
        |
        v
orbis-ui sequential async worker
        |
        v
orbis-application::AppService<P>
        |
        v
orbis-providers traits
        |
          +------------------------------+------------------+
          |                              |                  |
          v                              v                  v
  ApplicationRuntime             orbis-session-client   Hardware1
          |                       (Performance + Battery  (system D-Bus)
          |                        provider)                    |
          v                              |                  v
  GpuPrimitiveServices                   |             orbis-hardwared
    |       |       |                    |                  |
    v       v       v                    v                  v
  Power   MUX    Access             Session1          fixed platform_profile
          |                              |
          +------------------------------+             system D-Bus / UPower

  Test/offscreen composition only:
  GpuServices → MockProvider / deterministic fixtures
```

Production interactive GUI: `ApplicationRuntime` содержит
`GpuPrimitiveServices` с независимыми Power/MUX/Access providers, Battery через
`orbis-session-client` → sessiond → UPower и Performance через Session1/direct
Hardware1. Product GPU mode не имеет доказанного backend и возвращает
`Unsupported`/`Unavailable`; production runtime не создаёт и не использует
`MockProvider`. Mock сохраняется для unit tests, deterministic fixtures и
offscreen rendering.

### Crate boundaries

| Crate | Ответственность |
|---|---|
| `orbis-core` | Domain types, invariants и backend-neutral semantics |
| `orbis-config` | TOML/XDG configuration, versioning и atomic storage |
| `orbis-capabilities` | Capability model, fixture loading и report assembly |
| `orbis-providers` | Provider traits, errors и broad `MockProvider` |
| `orbis-application` | Async use cases и обязательный authoritative read-back |
| `orbis-session-protocol` | Нейтральный getter-only D-Bus wire contract |
| `orbis-session-client` | GUI-side Session1 reads + direct Hardware1 Performance provider |
| `orbis-sessiond` | User daemon, UPower adapter, discovery, server и lifecycle |
| `orbis-ui` | Slint presentation, UI mapping и sequential worker |
| `orbis-cli` | Зарезервированный CLI crate; текущий binary — stub |
| `orbis-test-support` | Deterministic mock device states |

`orbis-session-protocol`, client и daemon разделены намеренно. Application layer
не зависит от daemon или D-Bus transport. `orbis-hardwared` входит в workspace
(добавлен после ADR 0006 — первой доказанной privileged capability); production
deployment запускает его как отдельный узкий root-helper для Performance и
Battery mutation.

## 3. UI и application boundary

Slint callbacks не блокируют event loop аппаратной или D-Bus работой. Они
отправляют typed commands в один Tokio worker. Worker:

- выполняет команды последовательно;
- сохраняет FIFO между Performance, GPU и Battery;
- coalesce-ит только соседние queued Battery commands (last-wins в группе);
- не clamp-ит значения и не подменяет provider validation;
- возвращает typed result в UI через `Weak<AppWindow>::upgrade_in_event_loop`.

`AppService<P>` не хранит backend state. После успешной mutation-команды он
перечитывает provider и возвращает `ApplyResult` вместе с authoritative state.
Ошибка команды и ошибка read-back различаются. UI обновляет backend-derived
состояние только из результата worker/application path; optimistic hardware
state не допускается.

## 4. Provider и capability model

Provider traits определяют backend-neutral контракты для Performance, Battery,
GPU, Fans, power limits, display, lighting, telemetry и других областей. Наличие
trait или domain type не означает существование production provider.

На текущем этапе:

- `MockProvider` реализует широкий набор traits для tests, deterministic fixtures
  и offscreen UI; production GPU product-mode backend отсутствует. Performance
  использует real session-client/Hardware1 provider;
- `UPowerChargeLimitProvider` — реальный read-only Battery provider внутри
  `orbis-sessiond`;
- `SessionChargeLimitProvider` — read-only Battery provider над session D-Bus,
  используется production GUI как `battery_service` (live-validated);
- Battery mutation backend, application routing и production GUI control —
  **COMPLETED / LIVE-VALIDATED** через caller-preserving
  `Hardware1 → orbis-hardwared → typed asusd` D-Bus API; shell `asusctl` и
  competing direct sysfs write не используются. См. [ADR 0007](adr/0007-battery-mutation-backend.md).
- runtime capability discovery общего назначения ещё не реализован;
  `orbis-capabilities` в основном собирает reports и читает dated fixtures.

Backend-specific enum values, ranges и object presence не должны протекать в UI
как доказанная semantics. Probe обязан различать `Unknown`, `ReadOnly`,
`Unsupported`, `PermissionDenied` и временную недоступность.

## 5. Battery Charge Limit

Domain model:

```rust
ChargeLimit {
    enabled: bool,
    configured_percent: Option<Percent>,
    effective_percent: Option<Percent>,
    bounds: Option<ChargeLimitBounds>,
}
```

`ChargeLimitBounds` содержит `min`, `max` и ненулевой `step`. Семантика:

- `configured_percent` и `effective_percent` разделены по источнику;
- `enabled` authoritative source — UPower `ChargeThresholdEnabled`;
- `configured_percent` authoritative source — asusd `ChargeControlEndThreshold`;
- `effective_percent` authoritative source — kernel `charge_control_end_threshold`;
- `bounds = Some(...)` — конкретный backend действительно сообщил constraints;
- `bounds = None` — hardware/backend constraints неизвестны;
- unknown bounds не являются ошибкой и не запрещают показать известный current;
- значения не clamp-ятся и не округляются в domain/application boundary.

Asusd compatibility setter принимает `u8` `20..=100` с шагом 1.
`100` — обычный threshold, не implicit disable. `SetChargeLimit` не смешивает
enable/disable semantics. Успех mutation требует fresh asusd configured,
kernel effective и Session1 read-back; direct sysfs write при активном asusd
запрещён как competing owner.

Production GUI Battery slider uses `20..=100`, step `1`; this UI contract and
mock bounds are distinct from authoritative hardware facts. Их нельзя
записывать в wire/domain state как constraints UPower или устройства.

Session wire DTO имеет D-Bus signature `(bbybyyy)`:

```text
enabled, percent_present, percent,
bounds_present, min, max, step
```

При отсутствующих данных payload canonical: отсутствующий percent кодируется
нулём; отсутствующие bounds — `bounds_present=false` и `min/max/step=0`.
Client валидирует D-Bus DTO как untrusted input и отклоняет noncanonical values.

## 6. Реализованный sessiond path

`orbis-sessiond` — непривилегированный user daemon. Реализованный путь:

1. открыть system-bus connection к UPower;
2. выполнить read-only `EnumerateDevices`;
3. выбрать ровно одну system battery (`Type=Battery`, `PowerSupply=true`);
4. собрать UPower source/provider и Session1 service;
5. открыть session-bus server, занять
   `io.github.orbiscontrol.Session` и экспортировать
   `/io/github/orbiscontrol/Session`;
6. обслуживать getter `ChargeLimit` до SIGINT/SIGTERM;
7. освободить connection/name при штатном завершении.

UPower и session-client proxies используют `CacheProperties::No`: каждый
authoritative read выполняет новый property Get. Fresh-read semantics проверены
последовательными отличающимися значениями. UPower сообщает current threshold и
enabled/support flags, но не hardware min/max/step, поэтому provider возвращает
`bounds=None`. Session1 Battery read model остаётся fresh read-back и не
делегирует mutation.

NixOS module создаёт systemd user service с `Type=dbus`,
`BusName=io.github.orbiscontrol.Session`, Nix-store `ExecStart`,
`Restart=on-failure` и `PartOf/WantedBy=graphical-session.target`. `Type=dbus`
считает daemon ready только после захвата имени. Session1 Performance остаётся
getter-only в final mutation architecture.

## 7. Privilege boundary

- `orbis-ui`: user process, без root и sysfs writes. Узкий typed D-Bus вызов
  `Hardware1` для Performance mutation не считается direct hardware I/O:
  authorization, fixed-path write и read-back остаются в `orbis-hardwared`.
- `orbis-sessiond`: user daemon; может читать system services через их публичные
  D-Bus APIs, но не получает root и не делегирует Performance mutation.
- System services (`UPower`, в будущем доказанные ASUS backends) сохраняют свою
  собственную privilege boundary.
- `orbis-hardwared`: реализован для доказанных privileged capabilities —
  Performance profile write (`/sys/firmware/acpi/platform_profile`) и Battery
  charge-limit mutation (typed asusd D-Bus setter) — и входит в workspace
  (ADR 0006). Его allowlist, validation, authorization и sandboxing
  live-validated; hardwared НЕ является generic sysfs writer, каждая новая
  privileged capability добавляется отдельно. См. [ADR 0006](adr/0006-privileged-performance-write.md)
  и [ADR 0007](adr/0007-battery-mutation-backend.md).
- Fan curve writes: единственный owner — `asusd` (typed `xyz.ljones.FanCurves`
  D-Bus setter), по паттерну Battery ADR 0007; прямой sysfs write при активном
  asusd запрещён (duplicate-writer risk). Read-only fan curve backend
  (`asus_custom_fan_curve`) остаётся источником authoritative read-back и
  capability metadata. См. [ADR 0011](adr/0011-fan-curve-write-ownership.md).
- GPU mutation через supergfxd следует staged lifecycle contract из [ADR 0008](adr/0008-supergfxd-staged-gpu-mutation.md):
  supergfxd остаётся single lifecycle owner, а будущий GPU `Hardware1` должен
  добавлять узкую polkit authorization и не переисполнять lifecycle sequencing.

### Performance mutation authorization

Final read path:

```text
GUI/application → Session1 → orbis-sessiond → authoritative read-only Performance backend
```

Final write path:

```text
original application caller
  → io.github.orbiscontrol.Hardware
  → /io/github/orbiscontrol/Hardware
  → io.github.orbiscontrol.Hardware1.SetPerformanceProfile(y) → y
  → orbis-hardwared → polkit → fixed platform_profile writer → fresh read-back
```

Polkit authorizes the `system-bus-name` of the original Hardware1 caller.
Session1 mutation delegation (`Session1 → sessiond → Hardware1`) запрещена в
production: user-service identity не доказывает конкретную active login
session, а второй D-Bus hop теряет original caller. Нельзя заменять caller
identity на UID sessiond или любую active session этого UID: это confused
deputy risk для same-UID SSH/background/linger callers. Caller-supplied
UID/PID/session, user-unit name, executable path и permissive polkit defaults
не являются trust anchors.

После confirmed Hardware1 result `AppService::set_performance()` выполняет
fresh `Session1` `performance_state()` read-back. Optimistic state update не
используется. Direct Hardware1 `system-bus-name` path требует E2E VM proof до
live hardware write; harmless polkit 127 audit 2026-08-11 авторизовал
`startplasma-wayland`, `plasmashell`, kitty и child process из kitty при
`allow_any=no`, `allow_inactive=no`, `allow_active=yes`.

## 8. GPU semantics

Нельзя объединять в один флаг или enum-derived hardware fact:

- physical MUX state;
- dGPU availability/access policy;
- фактический dGPU power state;
- requested mode и pending/reboot/logout requirement.

Четыре UI-кнопки — product abstraction. `Ultimate` не считается applied, пока
MUX state не подтверждён после необходимого reboot. `Eco`/block не равен
physical MUX. Raw numeric enum semantics конкретного ASUS backend остаётся
UNKNOWN без versioned evidence и mapping tests (ADR 0003).

### GPU capability boundary

GPU backend capabilities выражаются независимыми concept-specific provider
traits: provider реализует только те capabilities, которыми реально владеет,
и не обязан предоставлять полную GPU product model (ADR 0005).

- `GpuPowerProvider` — первый production example
  (`SupergfxdGpuPowerProvider` реализует только runtime power capability);
- legacy `GpuProvider` пока существует для test/full product-mode abstraction,
  но не является production hardware backend;
- production GPU product policy backend не доказан: product mode остаётся
  `Unsupported`/`Unavailable`, пока не появятся отдельный backend и доказанный
  policy mapping;
- application может выставлять independent reads (например,
  `AppService::gpu_power_state()`);
- hardware backend не обязан быть источником всех GPU concepts.

Сохраняется формула:

```text
physical MUX
!= dGPU access policy
!= runtime power
!= requested/product mode
!= pending/action requirement
```

## 9. Writes

Наличие read path, writable property в introspection или mode `0644` не доказывает
безопасную запись. Каждый production write path вводится отдельно и требует:

1. capability proof на конкретном backend/device class;
2. подтверждённой semantics, units, range и step;
3. validation до I/O;
4. безопасной privilege boundary;
5. точного error mapping;
6. authoritative read-back;
7. честного `Unsupported` при отсутствии доказательств;
8. tests для short-circuit, error classes и state transitions.

Текущий read-only session backend не является предварительным одобрением
asusd/sysfs writes.

## 10. Accepted decisions and evidence

- [ADR 0001](adr/0001-rust-and-slint.md) — Rust + Slint.
- [ADR 0002](adr/0002-daemon-boundaries.md) — GUI/sessiond privilege boundary.
- [ADR 0003](adr/0003-gpu-provider-strategy.md) — независимые GPU concepts.
- [ADR 0004](adr/0004-authoritative-read-only-session.md) — authoritative
  read-only session contract.
- [ADR 0005](adr/0005-split-gpu-provider-capabilities.md) — split GPU provider
  capabilities по hardware concepts.
- [ADR 0006](adr/0006-privileged-performance-write.md) — привилегированный
  write path Performance profile (первая controlled mutation).
- [ADR 0007](adr/0007-battery-mutation-backend.md) — Battery mutation backend
  через asusd (COMPLETED / LIVE-VALIDATED).
- [ADR 0008](adr/0008-supergfxd-staged-gpu-mutation.md) — staged GPU mutation
  contract и lifecycle ownership через supergfxd; implementation pending.
- [ADR 0009](adr/0009-native-asus-eco-backend.md) — native ASUS Eco read-only
  preflight foundation.
- [ADR 0010](adr/0010-architecture-evolution.md) — архитектурный verdict после
  source audit (KEEP/EVOLVE/REPLACE/DEFER).
- [ADR 0011](adr/0011-fan-curve-write-ownership.md) — fan curve write ownership:
  asusd как единственный writer (по паттерну Battery ADR 0007).
- [`research-report.md`](research-report.md) и hardware fixtures — dated evidence,
  не current implementation status.

## 11. Future direction (после source audit)

Следующие области являются запланированным направлением эволюции, а не текущей
реализацией. Они зафиксированы в [ADR 0010](adr/0010-architecture-evolution.md)
и не выполняются в рамках этой документационной задачи.

### EVOLVE

1. **Runtime Capability Registry** — существующий `orbis-capabilities` пока не
   является полноценным runtime discovery system. Capabilities должны строиться
   из фактических providers/probes; read/write/constraints/backend/requirements
   должны моделироваться отдельно.
2. **Application/worker composition** — flat service injection заменён
   `ApplicationRuntime`/composition module в Task 2.1; дальнейшее расширение
   runtime capabilities всё равно не должно превращать composition в неявный
   global registry.
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
   всего приложения; ownership/provider priority определяется **per capability**
   (например: Performance → kernel `platform_profile`; Battery configured →
   asusd; Battery effective → kernel `power_supply`; Battery general telemetry →
   UPower; GPU staged lifecycle → supergfxd compatibility backend). Одна product
   capability может использовать несколько authoritative sources для разных
   semantics.

### REPLACE / REMOVE LATER

- legacy `GpuProvider` должен быть завершённой миграцией заменён
  capability-specific interfaces;
- production composition dependency на `MockProvider` устранена в Task 3;
  `MockProvider` остаётся для tests/offscreen/deterministic scenarios;
- `ApplicationRuntime` должен эволюционировать вместе с новыми capability
  domains, сохраняя grouped composition и не возвращаясь к flat positional
  service injection;
- dead module options и устаревшие helpers должны быть удалены отдельными
  mechanical tasks после проверки usages.

### DEFER

Не объявлять готовыми и не проектировать write implementation без evidence:
power limits; окончательный product GPU Eco/Standard/Ultimate/Optimized mapping;
live GPU mutation; automation privilege semantics.
