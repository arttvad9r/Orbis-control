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

## Consequences

- Появляется первый доказанный привилегированный компонент; его атакуемая
  поверхность минимальна (один типизированный метод, фиксированный файл,
  polkit на каждый вызов, sandbox по threat-model §3.3).
- `orbis-hardwared` НЕ объявляется универсальным владельцем всех hardware
  writes; другие операции (Battery, GPU product) требуют отдельных ADR и
  evidence.
- Session1 setter (`SetPerformance`) добавляется отдельным шагом после этого
  ADR: validation, delegation в hardwared, read-back, честный `ApplyResult`.
- GUI включает write-capability для Performance (`perf_writable=true`) только
  после подтверждённого read-back path; UI остаётся read-only до этого.
- Реализация hardwared/Session1 setter/GUI mutation — вне данного ADR
  (фиксируется только архитектура).

## Status

**Accepted.** Фиксирует архитектуру первой controlled mutation (Performance
profile). Реализация компонентов — отдельные шаги; каждая будущая privileged
capability требует собственного ADR по evidence.
