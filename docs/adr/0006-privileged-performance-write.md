# ADR 0006: Privileged Performance Profile Write Path

- Статус: **Принято** (2026-08-09)
- Дата: 2026-08-09

## Context

Первый controlled mutation — Performance profile (Silent/Balanced/Turbo).
Read path live-validated: kernel `/sys/firmware/acpi/platform_profile` →
`KernelPerformanceProvider` → Session1 → session client → GUI. Write path
требует привилегий:

- `/sys/firmware/acpi/platform_profile` = `-rw-r--r-- root:root 644`; запись
  невозможна текущим пользователем/`orbis-sessiond` без root;
- asusd 6.3.8 на live-системе НЕ экспортирует `xyz.ljones.Platform`
  (PlatformProfile setter отсутствует); numeric setter/mapping не proven;
- power-profiles-daemon не установлен; UPower не пишет profile;
- D-Bus system.conf default policy (`<deny send_type="method_call"/>`) не
  позволяет user-процессам вызывать системные методы без явного allow.

По критериям ADR 0002 все условия введения `orbis-hardwared` выполнены
(не предоставляется asusd; не предоставляется стандартным D-Bus; не может
выполняться непривилегированным; стабильный kernel ABI; узкая операция).

## Decision

1. **GUI и `orbis-sessiond` остаются unprivileged.** `sessiond` — владелец
   состояния и оркестратор, но не выполняет sysfs write и не получает root.
2. **Реальный backend — kernel `/sys/firmware/acpi/platform_profile`**
   (standard symbolic ABI; read path уже live-validated). asusd НЕ
   используется как write backend: live API отсутствует, setter/mapping не
   proven.
3. **Запись требует узкого root-компонента `orbis-hardwared`.** Вводится
   только для доказанной операции; каждая новая privileged capability
   добавляется отдельно по evidence.
4. **`orbis-hardwared` НЕ является generic sysfs writer.** API принимает
   закрытый semantic enum/profile, НЕ arbitrary path/string; фиксированный
   allowlist атрибутов (id → путь в коде).
5. **Единственная разрешённая capability на этом этапе:
   `SetPerformanceProfile`.** Никаких других write-операций.
6. **Privilege API shape:** `SetPerformanceProfile(profile)` где `profile` —
   закрытый semantic enum `{Silent, Balanced, Turbo}`. Клиент не передаёт
   строки/пути/символы.
7. **Mapping внутри hardwared:**

   ```text
   Silent   → quiet
   Balanced → balanced
   Turbo    → performance
   ```

8. **Перед write:**
   - requested profile должен быть допустим (закрытый enum);
   - соответствующий symbol должен присутствовать в
     `platform_profile_choices` (иначе `Unsupported` — не угадывать).
9. **Операция:**
   1. validate;
   2. ровно один write в фиксированный `platform_profile`;
   3. fresh read-back (`CacheProperties::No` semantics, без кэша);
   4. success только при совпадении read-back с requested.
   Ошибки не скрывать и не подменять optimistic success; backend error
   propagates.
10. **Polkit:**
    - отдельная action только для Performance mutation;
    - ориентироваться на active local user;
    - не давать generic root access;
    - exact policy details (action id, allow_active/inactive) могут быть
      уточнены implementation-step.
11. **Performance confirmation пользователю НЕ требуется:** операция
    immediate/reversible, без reboot/logout. Существующая UI policy (клик без
    `confirmed` для Performance) не меняется.

## Amendment — authorization architecture (2026-08-11)

Статус ADR остаётся **Accepted**. Уточняется окончательная граница
авторизации первой controlled mutation.

### Read path

```text
GUI/application
  → Session1
  → orbis-sessiond
  → authoritative read-only Performance backend
```

`Session1` остаётся getter-only для Performance mutation architecture.

### Write path

```text
original application caller
  → Hardware1 на system bus
  → orbis-hardwared
  → polkit
  → fixed platform_profile writer
  → fresh read-back
```

Hardware1 contract:

```text
service:   io.github.orbiscontrol.Hardware
path:      /io/github/orbiscontrol/Hardware
interface: io.github.orbiscontrol.Hardware1
method:    SetPerformanceProfile(y) → y
```

Polkit subject — `system-bus-name` **original Hardware1 caller**. Не
используются caller-supplied UID/PID/session, user-unit name, executable-path
checks или UID-only authorization. `allow_any=yes` и `allow_inactive=yes`
не допускаются.

Mutation delegation через

```text
Session1 → orbis-sessiond → Hardware1
```

в production architecture не допускается.

Причины:

1. `systemd --user` `orbis-sessiond` не является надёжным представителем
   конкретной active login session;
2. второй D-Bus hop теряет original caller identity;
3. замена `UID(sessiond)` на «любую active session этого UID» создаёт
   confused-deputy risk;
4. same-UID SSH/background caller не должен наследовать privilege параллельной
   active KDE session;
5. direct Hardware1 сохраняет original system-bus sender, который hardwared
   может передать polkit.

### Empirical evidence

В read-only audit от 2026-08-11 на polkit 127 harmless action с defaults

```text
allow_any=no
allow_inactive=no
allow_active=yes
```

получил `AUTHORIZED` для `startplasma-wayland`, `plasmashell`, kitty и child
process из kitty, включая процессы под `systemd --user`. Поэтому KDE
user-manager process model сам по себе не является blocker для `allow_active`
semantics. Direct Hardware1 `system-bus-name` authorization должна быть
подтверждена E2E в изолированной VM до live hardware write.

### Post-mutation confirmation

После confirmed результата Hardware1:

```text
AppService::set_performance()
  → fresh Session1 performance_state()
  → authoritative post-mutation state
```

Optimistic update не используется.

### Implementation follow-up

Временный route, добавленный ранее для Session1 mutation, должен быть удалён
следующим implementation step:

- удалить `Session1.SetPerformance`;
- удалить `orbis-sessiond` `HardwarePerformanceClient` delegation;
- вернуть Session1 Performance к read-only semantics;
- application-side production Performance provider должен читать через
  Session1, а писать через Hardware1.

Конкретное имя Rust struct/provider фиксируется только после implementation
review.

## Consequences

- Появляется первый доказанный привилегированный компонент; его атакуемая
  поверхность минимальна (один типизированный метод, фиксированный файл,
  polkit на каждый вызов, sandbox по threat-model §3.3).
- `orbis-hardwared` НЕ объявляется универсальным владельцем всех hardware
  writes; другие операции (Battery, GPU product) требуют отдельных ADR и
  evidence.
- Session1 setter (`SetPerformance`) не является production authorization
  boundary и подлежит удалению из временной реализации; final write provider
  использует direct Hardware1 caller path.
- GUI включает write-capability для Performance (`perf_writable=true`) только
  после подтверждённого read-back path; UI остаётся read-only до этого.
- Реализация hardwared/Session1 setter/GUI mutation — вне данного ADR
  (фиксируется только архитектура).

## Status

**Accepted.** Фиксирует архитектуру первой controlled mutation (Performance
profile), включая final direct-caller authorization amendment от 2026-08-11.
Реализация компонентов — отдельные шаги; каждая будущая privileged capability
требует собственного ADR по evidence.
