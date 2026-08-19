# Product Contract — Orbis Control

> Роль: **PRODUCT INTENT**. Этот документ описывает, что пользователь должен
> получить от Orbis Control и какие свойства результата нельзя потерять. Текущий
> implementation baseline находится в [`current-state.md`](current-state.md),
> development progression — в [`roadmap.md`](roadmap.md), а способ доказывать
> claims — в [`verification.md`](verification.md).

## 1. Purpose

Orbis Control даёт владельцу Linux-ноутбука ASUS единый пользовательский способ
наблюдать за состоянием и возможностями устройства и, только когда capability и
её semantics доказаны, выполнять разрешённые действия.

Продукт существует, чтобы hardware state и доступные действия были для
пользователя понятными, честными и пригодными для повседневного управления, а не
представлялись через неподтверждённые defaults, mock state или optimistic success.

## 2. Target user and context

- Пользователь — владелец или оператор ASUS-ноутбука, работающего под Linux.
- Основной interactive context — обычная пользовательская сессия; продукт
  ориентирован на Wayland, X11 поддерживается как compatibility mode.
- Повседневный контекст — просмотр battery, performance, GPU, telemetry и других
  доказанных capabilities, а также controlled actions без запуска GUI от root.

Orbis не предполагает, что наличие ASUS-модели, backend object или знакомого
названия capability доказывает поддержку конкретной операции.

## 3. Current product baseline

По текущему `main` пользовательский production baseline включает:

- Battery Charge Limit read path;
- controlled Battery threshold mutation через узкий privileged owner с
  authoritative confirmation;
- Performance current/available profiles;
- controlled Performance mutation с authoritative read-back;
- независимые GPU Power, physical MUX и Access observations;
- production telemetry polling;
- profile-specific fan reads;
- честные capability/read/write states вместо mock fallback.

Это не означает, что все видимые или domain-level функции готовы к записи.
Product GPU mode не имеет доказанного production policy backend и остаётся
недоступным. Fan mutation/reset implementation существует, но сейчас
fail-closed из-за открытых safety-contract issues. Power limits и extended ASUS
controls включаются только после concept-specific evidence.

Точный operational status не дублируется здесь и находится в
[`current-state.md`](current-state.md).

## 4. Intended product outcomes

В целевом развитии пользователь должен:

- понимать фактическое состояние устройства и границы доступных capabilities;
- отличать applied/confirmed state от requested, pending, unknown и unavailable;
- выполнять только те hardware/system actions, для которых есть доказанные
  semantics, validation и безопасная privilege boundary;
- видеть authoritative result или честную причину отказа после действия;
- получать полезный degraded/read-only experience, если backend или capability
  недоступны;
- не видеть локальный preview или fixture как реальное состояние устройства.

## 5. Core workflows

### Inspect machine state

```text
launch application
→ inspect user-visible capability sections
→ see current value and availability state
→ understand whether the value is real, unknown, read-only or unavailable
```

### Understand capability before action

```text
identify requested outcome
→ determine whether the relevant capability is supported and writable
→ keep unknown/unsupported/permission-denied states distinct
→ do not offer an action whose semantics are not established
```

### Perform and confirm a permitted action

```text
request a supported action
→ validate at the owning boundary
→ authorize the bounded operation when privilege is required
→ perform the operation
→ read authoritative state again when confirmation is possible
→ show confirmed result, pending requirement or explicit error
```

The product must not present a requested value as applied merely because a command
was accepted or sent successfully. For actions that are not ready, the user must
see an honest read-only, unavailable, blocked or unsupported state rather than
simulated success.

## 6. Product invariants

These are user-visible trust properties, not implementation structure.

- **Truthful state:** displayed hardware state must be grounded in an actual
  source or be explicitly marked unknown/unavailable.
- **No invented certainty:** unknown bounds, enum semantics and backend support
  remain unknown; product defaults and mock values are not hardware facts.
- **Capability honesty:** read-only, unsupported, permission-denied and
  temporarily unavailable states are not presented as writable or successful.
- **Confirmed mutations:** an action is shown as successful only after the
  confirmation required by that operation; failed writes and failed read-back
  remain visible as failures.
- **Accepted is not applied:** transport/backend acceptance alone cannot become
  authoritative observed state.
- **Safe degradation:** absence or failure of a backend must not silently produce
  a different fake state or make unrelated usable sections appear successful.
- **Bounded privilege:** user-visible actions must not turn Orbis into a generic
  root or unrestricted hardware writer. A privileged operation requires a narrow,
  proven use case and its own safety boundary.
- **User-session boundary:** the GUI is a normal-user application and does not
  perform direct privileged hardware writes.
- **Concept separation:** distinct user meanings such as GPU power, MUX, access
  policy and requested product mode must not be collapsed into one misleading
  state.
- **Fail-closed incomplete controls:** known unsafe or incompletely wired write
  paths stay disabled until the contract and evidence are complete.

## 7. Non-goals

The following are explicitly outside the intended product behaviour:

- presenting every ASUS feature as supported merely because a backend or device
  object exists;
- a universal root helper or generic sysfs writer;
- treating mock/test functionality as production hardware support;
- hiding uncertainty or replacing unsupported mutation with simulated success;
- making the GUI the owner of hardware-specific implementation details;
- claiming broad model/firmware support without device-specific evidence;
- automatically applying persisted/default values merely because they were
  loaded from configuration;
- enabling a write path while a known safety-contract defect is unresolved.

The product contract does not require every planned feature to exist in the
current release. Status and sequencing remain in `current-state.md` and
`roadmap.md`.

## 8. Product success criteria

Orbis is advancing the product when a user can reliably:

1. inspect relevant laptop state from a normal user session;
2. understand what is confirmed, unavailable, read-only, unsupported or unknown;
3. use a supported action without being exposed to guessed hardware semantics;
4. distinguish requested state from authoritative confirmed state;
5. recover from backend unavailability without false success or misleading
   fallback;
6. receive additional capabilities only when their user-visible semantics and
   safety boundaries are established.

These are product outcomes, not a verification checklist. Required evidence for
each claim is defined in [`verification.md`](verification.md).

## 9. Safety and trust expectations

The user should be able to trust that Orbis:

- does not claim that a change was applied before the required confirmation;
- does not turn missing evidence into a supported capability;
- does not hide meaningful uncertainty about device state or constraints;
- does not perform unsupported or unbounded hardware actions;
- remains understandable when a provider is absent, fails, or reports a partial
  capability;
- disables a known-unsafe write path rather than preserving functionality at the
  expense of correctness.

The exact provider boundaries, protocol contracts, validation rules and privilege
implementation belong in [`architecture.md`](architecture.md) and the accepted
ADRs, not in this product contract.

## 10. Product terminology

| Term | Product meaning |
|---|---|
| **Capability** | A device/system ability that Orbis may expose only when its support and semantics are established |
| **Authoritative state** | State read from the owning backend after the relevant operation; not merely requested or optimistic UI state |
| **Unknown** | Evidence is insufficient to claim a value or constraint; it is not the same as unsupported |
| **Unavailable** | The relevant backend or runtime path cannot currently provide the state |
| **Read-only** | The user may inspect the state, but Orbis does not claim that mutation is supported |
| **Mock-only** | Behaviour exists for tests, offscreen rendering or development composition, not as real device support |
| **Pending** | A requested outcome requires a later action or confirmation and must not be shown as already applied |
| **Blocked** | Implementation may exist, but a known contract/evidence/release blocker prevents safe product exposure |