# ADR 0011: Fan Curve Write Ownership

- Статус: **Принято как архитектурный контракт** (2026-08-17)
- Дата: 2026-08-17

## Context

Read-only fan curve backend (`asus_custom_fan_curve` hwmon) реализован и
live-validated (Task 6.2/6.3): активная CPU/GPU кривая читается через
`FanProvider::active_curve`, raw `FanPwm` 0..255, profile-specific semantics
не фальсифицируются. Остаётся нерешённым вопрос **ownership** для записи
кривых: кто является единственным writer.

Два кандидата:

1. **asusd `xyz.ljones.FanCurves`** — typed D-Bus API, владеет persistence и
   restore;
2. **direct sysfs writer через `orbis-hardwared`** — прямой write в
   `pwm{1,2}_auto_point{1..8}_temp/_pwm`.

## Precedents в проекте

- **Battery (ADR 0007)**: `asusd` владеет configured threshold (persistence в
  `/etc/asusd/asusd.ron`, startup/resume/power-event restore). Прямой Orbis
  sysfs write при активном `asusd` создаёт competing-owner risk. Решение —
  `asusd` как единственный owner; Orbis вызывает через
  `Hardware1 → polkit → hardwared → typed asusd D-Bus setter`.
- **Performance (ADR 0006)**: `asusd` НЕ предоставлял PlatformProfile setter,
  поэтому выбран direct kernel write через `hardwared`. Это исключение, а не
  правило: direct write выбран только потому, что asusd API отсутствовал.
- **Original-caller polkit model (ADR 0006/0007)**: mutation идёт через
  `Hardware1` на system bus, polkit subject — `system-bus-name` original
  caller; `sessiond` не выполняет privileged writes и не является deputy.

## Сравнение backend

### asusd `xyz.ljones.FanCurves`

- **API покрывает CPU/GPU 8-point curves**: `(s(yyyyyyyy)(yyyyyyyy)b)` =
  профиль + 8×(temp,pwm) CPU + 8×(temp,pwm) GPU + enabled
  (provider-matrix.md, reference audit `fan_curve_set.rs`).
- **Persistence/restore**: asusd сохраняет `fan_curves.ron` и повторно
  применяет при startup, смене профиля, resume (reference audit
  `ctrl_fancurves.rs`). Это обязанности, которые Orbis не должен дублировать.
- **Единственный writer**: asusd — единственный writer; нет duplicate-writer
  risk.
- **`pwm*_enable`**: asusd управляет manual/auto mode; Orbis не нуждается в
  `pwm*_enable` experiments.
- **Read-back**: после asusd setter — fresh read через наш read-only
  `SysfsFanCurveSource` (authoritative read-back обязателен).
- **Authorization**: через `Hardware1 → polkit → hardwared → typed asusd
  D-Bus setter` (как Battery ADR 0007).

### direct sysfs writer через `orbis-hardwared`

- **Duplicate-writer risk**: asusd уже пишет те же `pwm*_auto_point*`
  атрибуты; два независимых writers на одном устройстве — конфликт
  (reference audit: "Whether two daemons can safely coexist").
- **`pwm*_enable`**: прямой write требует управления manual/auto mode,
  семантика которого не доказана для модели — blocker.
- **Persistence/restore**: Orbis не владеет `fan_curves.ron`; asusd
  перезапишет кривые при startup/profile change — состояние рассинхронизируется.
- **Дополнительный privileged surface**: ещё один прямой sysfs writer в
  hardwared.

## Decision

**Единственный owner для fan curve writes — `asusd`** (через typed
`xyz.ljones.FanCurves` D-Bus API), по паттерну Battery ADR 0007.

1. **Write path**:

   ```text
   original application caller
     → io.github.orbiscontrol.Hardware1
     → Orbis polkit authorization (system-bus-name original caller)
     → orbis-hardwared
     → typed xyz.ljones.FanCurves D-Bus setter
     → asusd
     → ASUS kernel backend + asusd persistence/restore
   ```

   Прямой Application → asusd отклонён (обходит единый Orbis typed
   authorization model). Делегация через `orbis-sessiond` запрещена (второй
   D-Bus hop теряет original caller identity). Прямой sysfs write через
   hardwared отклонён (duplicate-writer risk, `pwm*_enable` blocker,
   persistence/restore mismatch).

2. **Hardware1 method** — узкий semantic method, не generic D-Bus relay:

   ```text
   SetFanCurve(profile: y, cpu_curve: (8×(temp,pwm)), gpu_curve: (8×(temp,pwm))) → typed mutation result
   ```

   Contract: ровно 8 точек на вентилятор, temp в °C, raw PWM 0..255
   (`FanPwm`), монотонность (temp и pwm не убывают). Значения вне contract
   отвергаются до вызова asusd. Никаких path/interface/member, shell strings
   или arbitrary sysfs arguments.

3. **Raw `FanPwm` 0..255 не трактуется как percent.** Mapping 0..255 → % не
   доказан (на эталоне GPU curve достигает raw 112 > 100). asusd API принимает
   raw PWM; Orbis передаёт raw значения без преобразования.

4. **Profile semantics не выдумываются.** asusd FanCurves API принимает
   profile + curves. Orbis передаёт только доказанные profile→curve mapping;
   если asusd требует активный профиль для чтения defaults (reference audit:
   "Fan defaults may only be readable while their profile is active"), это
   фиксируется отдельно и не фальсифицируется.

5. **Read-back обязателен.** После asusd setter — fresh read через
   `SysfsFanCurveSource::active_curve`; success только при совпадении
   read-back с requested (как Battery ADR 0007: configured обязан совпасть с
   requested).

6. **`pwm*_enable` и defaults/reset** — НЕ в первом slice. asusd управляет
   manual/auto; Orbis не экспериментирует с `pwm*_enable`. `SetCurvesToDefaults`
   — отдельная операция после отдельного доказательства semantics.

## Unresolved blockers

1. **Raw PWM semantics в asusd**: reference audit отмечает, что Fan PWM может
   быть raw 0..255 ИЛИ percent→raw (`fan_curve_set.rs:68-114`). Требуется
   live-валидация, какой формат принимает asusd setter, до реализации.
2. **Profile→curve mapping**: как asusd связывает profile с кривой (активная
   кривая vs per-profile storage) требует live-валидации.
3. **Authorization внутри asusd**: как Battery (ADR 0007), нет отдельной
   polkit-проверки внутри asusd; authorization на system-bus/service policy
   boundary. Требуется E2E в VM до live write.

## Consequences

- `asusd` — единственный owner fan curve writes; Orbis не выполняет direct
  sysfs write при активном asusd.
- `orbis-hardwared` НЕ становится generic sysfs writer; добавляется только
  узкая fan curve capability (как Performance/Battery).
- `sessiond` остаётся unprivileged и не выполняет privileged writes.
- Read-only fan curve provider (Task 6.2/6.3) остаётся источником authoritative
  read-back и capability metadata.
- Реализация mutation backend — отдельные шаги после live-валидации blockers.

## Status

**Принято как архитектурный контракт.** Фиксирует `asusd` как единственного
owner fan curve writes. Реализация — отдельные шаги; каждый blocker требует
live-валидации до live hardware write.
