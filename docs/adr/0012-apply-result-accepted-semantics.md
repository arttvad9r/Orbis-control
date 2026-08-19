# ADR 0012: `ApplyResult::Accepted` Is Not Applied

- Статус: **Принято как архитектурный контракт** (2026-08-19)
- Дата: 2026-08-19

## Context

Некоторые mutation backends могут подтвердить, что запрос принят и сохранён на config/backend level, но не могут authoritative read-back фактическое hardware state. Aura Static RGB через write-only/partially observable ABI — основной пример.

`orbis-core::ApplyResult` уже различает:

- `Applied` — hardware state подтверждён authoritative read-back;
- `Accepted` — backend/config-level acceptance подтверждён, hardware state не подтверждён;
- `Pending` — операция требует отдельного reboot/logout/другого requirement;
- `Failed` / `RolledBack` — неуспешные outcomes.

Без отдельного архитектурного правила application/UI consumers могут ошибочно превратить `Accepted` в optimistic success и показать непроверенное hardware state как применённое.

## Decision

`ApplyResult::Accepted` **никогда не эквивалентен `ApplyResult::Applied`**.

1. `Applied` разрешён только когда mutation path получил authoritative подтверждение фактического hardware state.
2. `Accepted` означает только подтверждённое принятие запроса на том уровне, который backend способен доказать. UI должен представлять это как accepted/unconfirmed, а не как applied/current hardware state.
3. `Accepted` не должен автоматически обновлять authoritative observed state, очищать pending/desired state или использоваться как доказательство live hardware application.
4. Consumers обязаны сохранять вариант `Accepted` losslessly через application/presentation boundaries. Нельзя сворачивать его в `bool success`, generic `OK` или selected-state optimistic update.
5. `Pending` остаётся отдельной семантикой: requirement известен и ожидается дальнейшее lifecycle action. Нельзя использовать `Pending` как замену `Accepted` только потому, что hardware read-back недоступен.
6. Capability `Supported`/writable evidence означает возможность вызвать operation согласно contract; это не доказательство, что конкретный вызов будет `Applied`.

## Consequences

- `ApplyResult::is_applied()` возвращает `true` только для `Applied`.
- UI/application tests должны отдельно покрывать `Accepted` и запрещать optimistic applied presentation.
- Export/diagnostics/release evidence не должны повышать `Accepted` до hardware-applied claim.
- Backend с write-only ABI может честно участвовать в production mutation path, если presentation явно сохраняет unconfirmed semantics.
- Если позже появляется authoritative read-back, backend может возвращать `Applied` только после успешной проверки фактического state.

## Rejected alternatives

### Treat `Accepted` as `Applied`

Отклонено: создаёт ложное authoritative state и нарушает no-optimistic-success policy.

### Treat `Accepted` as `Failed`

Отклонено: теряет реальный факт, что backend принял запрос и config-level contract выполнен.

### Treat every unconfirmed write as `Pending`

Отклонено: `Pending` требует известного lifecycle/action requirement; отсутствие hardware read-back само по себе таким requirement не является.

## Verification boundary

Unit/integration tests могут доказать корректное сохранение `Accepted` semantics, но не могут доказать фактическое hardware application. `LIVE-VALIDATED` hardware claim требует отдельного authoritative evidence для конкретной операции.
