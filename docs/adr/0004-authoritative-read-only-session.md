# ADR 0004: Authoritative Read-Only Session Boundary

- Статус: **Принято** (2026-08-09)
- Дата: 2026-08-09

## Context

Первый production backend slice должен передавать Battery Charge Limit из
UPower через user daemon в client без hardware mutations. UPower сообщает
current threshold и enabled/support state, но не достоверные hardware
min/max/step. D-Bus property caches могут скрывать изменения, а readiness
daemon должен быть связан с реальным ownership service name.

## Decision

1. Session1 на этом этапе getter-only и экспортирует только `ChargeLimit`.
2. Domain и wire contract хранят presence current percent и bounds отдельно.
   Неизвестные constraints кодируются как `bounds=None` /
   `bounds_present=false, min/max/step=0`; product defaults не подставляются.
3. UPower source и session client строят proxies с `CacheProperties::No`.
   Authoritative reads всегда выполняют новый property Get.
4. Wire DTO считается untrusted input; noncanonical absent values и invalid
   domain combinations отклоняются.
5. `orbis-sessiond` остаётся user daemon. systemd unit использует `Type=dbus` и
   `BusName=io.github.orbiscontrol.Session`; readiness наступает после name
   ownership. SIGINT/SIGTERM завершают runtime штатно.
6. Production mutation methods не симулируют успех и возвращают `Unsupported`.

## Consequences

- UI/client может показать current threshold даже при unknown bounds.
- Mock/UI policy 40/100/5 не является hardware contract.
- Дополнительные D-Bus Gets предпочтительнее скрытого stale cache для
  authoritative state.
- Getter-only protocol нельзя расширять setter-ом без отдельного capability,
  validation, error/read-back и safety design.
- Type=dbus делает startup failure наблюдаемым supervisor-ом; restart policy
  остаётся у systemd.
- Tests должны проверять fresh sequential values, canonical wire encoding,
  error classes и clean lifecycle.

## Status

**Accepted.** Решение закрепляет уже реализованный и live-validated read-only
vertical slice. Любое добавление mutation API требует отдельного решения и не
считается автоматическим продолжением этого ADR.
