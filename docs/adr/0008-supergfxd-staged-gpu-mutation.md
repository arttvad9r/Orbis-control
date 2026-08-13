# ADR 0008: GPU mutation via supergfxd staged lifecycle

- Статус: **Принято** (2026-08-13)
- Дата: 2026-08-13

## Контекст

Для установленного `supergfxd 5.2.7` доказан source-level contract:

- source tag `5.2.7`;
- commit `a86383e1b2f32d4f87f8dd47f0d6b06690877c64`;
- `SetMode(GfxMode) -> UserActionRequired`;
- mode lifecycle выполняется staged worker-ом самого supergfxd.

ADR 0005 разделяет GPU capabilities по hardware concepts. Это решение
дополняет его mutation lifecycle и не превращает product `GpuMode` в raw
supergfxd mode.

## Решение

### Ownership

`supergfxd` является единственным compatibility owner полного GPU lifecycle.
Orbis делегирует ему mutation и не реализует самостоятельно display-manager
stop/start, NVIDIA process killing, driver unload/load, PCI unbind/remove/rescan,
runtime-PM sequencing, ASUS MUX sequencing, dGPU lifecycle или VFIO lifecycle.
Orbis может читать primitive capabilities отдельно. Generic sysfs writer не
вводится.

### Privilege boundary

Принятый mutation path:

```text
original application caller
  → Hardware1
  → Orbis polkit
  → orbis-hardwared
  → typed supergfxd system D-Bus
  → supergfxd lifecycle
```

Установленная policy supergfxd разрешает send access широким группам и не
разделяет read/mutation methods. Внутри `SetMode` нет caller credential check
или polkit. Поэтому `Hardware1` должен добавлять реально более узкую policy;
простой forwarding wrapper authorization boundary не является. `sessiond` не
используется как mutation deputy.

### SetMode и wire contract

`SetMode` принимает `GfxMode` и возвращает `UserActionRequired`. Return value
не означает applied state.

| wire | `GfxMode` |
|---:|---|
| 0 | `Hybrid` |
| 1 | `Integrated` |
| 5 | `AsusMuxDgpu` |
| 6 | `None` |

Полный enum также содержит `NvidiaNoModeset = 2`, `Vfio = 3`,
`AsusEgpu = 4`.

| wire | `UserActionRequired` |
|---:|---|
| 0 | `Logout` |
| 1 | `Reboot` |
| 2 | `SwitchToIntegrated` |
| 3 | `AsusEgpuDisable` |
| 4 | `Nothing` |

### Staged state и read-back

После request authoritative read-back включает `Mode()`, `PendingMode()`,
`PendingUserAction()`, `Power()` и `Supported()`. Для ASUS MUX transitions
дополнительно читаются MUX и access primitives.

- **APPLIED** — `Mode == requested`, `PendingMode == None`,
  `PendingUserAction == Nothing`;
- **PENDING / REQUIRES USER ACTION** — `PendingMode == requested`,
  `PendingUserAction != Nothing`;
- **FAILED** — D-Bus/backend error;
- **INCONSISTENT / PARTIAL** — contradictory mode/pending/action/read-back.

Один `Mode()` не является подтверждением mutation. Pending state у supergfxd
runtime-only и не persistent. supergfxd не транзакционен: forward failure может
оставить partial hardware state; reverse action list best-effort, rollback
failure возможен. Orbis не обещает atomic rollback.

При inconsistent/partial state Orbis останавливает дальнейшие mutations, делает
fresh reads relevant capabilities и показывает recovery-required state. Без
automatic retries.

### User-action policy

Orbis никогда автоматически не делает logout, reboot, не закрывает graphical
session и не перезапускает display manager. `Reboot` не является default
deployment/testing mechanism.

- `Logout` показывается пользователю и ждёт явного действия;
- `Reboot` показывается пользователю и ждёт явного действия;
- `SwitchToIntegrated` и `AsusEgpuDisable` показываются как explicit staged
  requirements и не выполняются скрыто.

### Первый технический slice

Первый implementation slice: staged supergfxd contract для
`Hybrid ↔ Integrated`.

Это не product mapping `Standard ↔ Eco`. Product presets остаются policy
layer; mapping `Eco/Standard/Ultimate/Optimized` принимается только после
primitive mutation capability validation. `Optimized` остаётся policy/preset,
а не supergfxd mode.

Для установленного source contract:

- `Hybrid → Integrated`: default `Logout`; `Reboot` при `always_reboot`;
  worker может wait logout, stop display manager, unload drivers, remove PCI
  GPU, disable dGPU/hotplug и start display manager;
- `Integrated → Hybrid`: default return `Nothing`; `Reboot` при
  `always_reboot`; worker всё ещё может содержать `WaitLogout`, display-manager
  lifecycle, PCI rescan, driver load и display-manager start.

`Nothing` не означает гарантированный immediate-live apply. `Hybrid ↔
AsusMuxDgpu` остаётся отдельным следующим slice: MUX write owned by supergfxd
и reboot required. Production GPU cards остаются disabled; live mutation этим
ADR не разрешается.

### Test-before-live gate

До любой live GPU mutation обязательны private P2P fake tests. `FakeSupergfxd`
моделирует `SetMode(mode) -> UserActionRequired`, `Mode()`, `PendingMode()`,
`PendingUserAction()`, `Power()` и `Supported()`.

Минимальные scenarios: immediate applied, logout required, reboot required,
pending requested mode, D-Bus failure, unsupported request, contradictory
read-back и partial/inconsistent state. Mutation через system bus в tests не
используется.

## Последствия

- lifecycle ownership остаётся у versioned supergfxd, без reimplementation в
  Orbis;
- mutation boundary будет уже текущего broad supergfxd D-Bus policy;
- staged result требует explicit read-back и отдельной user-action UX;
- partial state и non-atomic rollback являются частью contract;
- product presets и physical/backend modes остаются разными concepts (ADR 0005);
- следующий code milestone: **Implement READ/CONTRACT layer for staged
  supergfxd transition with private P2P fake service**;
- typed supergfxd staged DTO/source и fake tests выполняются до Hardware1 writes
  и UI writes.

## Out of scope

- live GPU mutation;
- `Hardware1` write implementation и UI mutation controls;
- `AsusMuxDgpu` slice;
- окончательный mapping product presets к backend modes;
- reimplementation supergfxd lifecycle в Orbis.
