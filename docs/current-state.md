# Current State

> Роль: **CURRENT STATUS**. Operational baseline для текущей реализации после
> завершения первого real read-only production vertical slice.

Статусы:

- **IMPLEMENTED** — существует в production code и покрыто tests;
- **LIVE-VALIDATED** — дополнительно проверено на живой системе;
- **MOCK-ONLY** — работает только через mock/test backend;
- **PARTIAL** — часть boundary реализована, end-to-end path неполон;
- **NOT IMPLEMENTED** — production implementation отсутствует;
- **BLOCKED** — есть конкретный внешний blocker;
- **UNKNOWN** — доказательств недостаточно.

## Summary

Первый production vertical slice реализован для read-only Battery Charge Limit:

```text
UPower → orbis-sessiond → Session1 D-Bus → orbis-session-client
```

Он протестирован и live-validated как Nix-installed systemd user service. GUI
пока использует `MockProvider`; production session client к нему не подключён.
Production hardware mutations отсутствуют.

## Major areas

| Area | Status | Фактическое состояние |
|---|---|---|
| Domain/core | IMPLEMENTED | Typed models/invariants для основных областей; optional charge bounds |
| Config | PARTIAL | TOML/XDG/versioned config и atomic store; нет production persistence workflow |
| Capabilities | PARTIAL | Domain model, report assembly, fixture loading/tests; runtime general probe отсутствует |
| Provider contracts | IMPLEMENTED | Traits и error model существуют |
| Broad provider implementation | MOCK-ONLY | `MockProvider` покрывает UI/application scenarios |
| Application layer | IMPLEMENTED | Performance/GPU/Battery commands + authoritative read-back |
| UI | MOCK-ONLY | Slint main window + sequential worker; interactive backend — mock |
| Session protocol | IMPLEMENTED | Getter-only `Session1.ChargeLimit`, `(bbybyyy)` |
| Session client | IMPLEMENTED | Fresh D-Bus Get, validation, read-only `BatteryProvider` |
| sessiond | LIVE-VALIDATED | Discovery, UPower read, server, runtime, signals, Nix user service |
| CLI | NOT IMPLEMENTED | `orbisctl` binary — stub |
| Production mutations | NOT IMPLEMENTED | Read-only providers return `Unsupported` |
| Privileged helper | NOT IMPLEMENTED | `orbis-hardwared` не входит в workspace |

## Battery Charge Limit

### IMPLEMENTED

- Slint Battery section и slider существуют.
- UI commands проходят через общий sequential async worker.
- Соседние queued `SetChargeLimit` coalesce-ятся last-wins; Performance/GPU
  команды разделяют группы.
- `AppService` выполняет provider command и обязательный authoritative read-back.
- Domain model:
  `ChargeLimit { enabled, percent: Option<Percent>, bounds: Option<ChargeLimitBounds> }`.
- `bounds=None` поддержан end-to-end и означает UNKNOWN hardware constraints.
- Session wire signature: `(bbybyyy)` с canonical absent values.
- Session client валидирует untrusted wire payload и не использует property cache.
- sessiond UPower provider, battery discovery, composition и D-Bus server
  реализованы.
- UPower provider читает support/enabled/end-threshold через
  `CacheProperties::No` и возвращает `bounds=None`.
- P2P tests покрывают protocol, service/server, client и composed path без real
  system/session bus.

### LIVE-VALIDATED

2026-08-09 controlled `nixos-rebuild test` подтвердил:

- Nix-installed `orbis-sessiond` запущен через generated systemd user unit;
- `Type=dbus` readiness и ownership
  `io.github.orbiscontrol.Session`;
- raw `ChargeLimit`:
  `(bbybyyy) false true 80 false 0 0 0`;
- 80 — observed current threshold на момент validation, не спецификация;
- `bounds_present=false`, canonical `min/max/step=0`;
- `systemctl --user stop` привёл к exit status 0, освобождению bus name и
  отсутствию процесса.

### Gaps

- GUI не использует `orbis-session-client`: интерактивный runtime всё ещё
  создаёт `MockProvider`.
- Worker type требует общий provider для Performance/GPU/Battery; безопасная
  composition/backend selection для mixed real/mock state ещё не выбрана.
- Production `set_charge_limit` и one-shot full charge — **NOT IMPLEMENTED**.
- asusd write provider — **NOT IMPLEMENTED**.
- sysfs fallback/write provider — **NOT IMPLEMENTED**.
- Hardware charge min/max/step на FA707NV — **UNKNOWN**. UI/mock policy 40/100/5
  не является hardware evidence.

## Performance

**PARTIAL / MOCK-ONLY**

- Domain types, `PerformanceProvider`, application read/set/read-back path,
  sequential worker и UI cards реализованы.
- Mock scenarios и tests покрывают state transitions/errors.
- General capability fixture содержит dated evidence наличия platform profiles
  на FA707NV.
- Production asusd/kernel/PPD provider отсутствует.
- UI не читает real current profile и не применяет profile к hardware.

## GPU Mode

**PARTIAL / MOCK-ONLY**

- Domain разделяет requested mode, physical MUX, access policy и power state.
- `GpuProvider`, application state/read-back, worker и UI states
  (pending/disabled/error) реализованы.
- Mock tests сохраняют applied state при pending Ultimate/Eco.
- Production asusd/supergfxd/Cardwire/sysfs provider отсутствует.
- Raw backend enum mapping, safe switch checks и real pending/reboot workflow не
  реализованы и без evidence считаются UNKNOWN.

## Fans, power limits, lighting and display

| Area | Status | Notes |
|---|---|---|
| Fan RPM/curves | NOT IMPLEMENTED | Domain/traits и dated fixtures существуют; production provider/UI editor отсутствуют |
| Power limits | NOT IMPLEMENTED | Domain/traits есть; units/ranges и safe write semantics не доказаны |
| Keyboard/Aura lighting | NOT IMPLEMENTED | Domain/traits и historical backend evidence есть |
| Display refresh/overdrive | NOT IMPLEMENTED | Domain/traits и historical DRM/asusd evidence есть |
| AniMe/Slash | NOT IMPLEMENTED | Trait/research only; FA707NV fixture отмечает устройства отсутствующими |
| Telemetry | MOCK-ONLY | UI отображает mock temperatures/fans/battery/power; real telemetry provider отсутствует |

Planned capability или hardware object presence не считается implementation.

## sessiond

**IMPLEMENTED / LIVE-VALIDATED** для read-only ChargeLimit:

- binary entry point вызывает production runtime;
- system-bus UPower connection;
- deterministic discovery ровно одной system battery;
- source/provider/service composition;
- Session1 name/path registration;
- SIGINT/SIGTERM lifecycle;
- supervisor restart policy делегирована systemd;
- mutation methods отсутствуют/Unsupported.

Не реализованы reconnect, multiple-battery selection, generic provider registry,
automation engine и остальные feature APIs из исторической Stage 0
спецификации.

## Packaging

**IMPLEMENTED / LIVE-VALIDATED** на NixOS:

- `nix flake check` PASS offline;
- `nix build .#orbis-control` PASS;
- package устанавливает `orbis-control`, `orbisctl`, `orbis-sessiond`;
- flake экспортирует `nixosModules.orbis-control`;
- module default package self-contained через consumer `pkgs.callPackage`;
- generated user service использует Nix-store binary, `Type=dbus`, BusName,
  `Restart=on-failure`, `RestartSec=2s`;
- controlled `nixos-rebuild test` PASS;
- persistent host enablement не выполнялось в рамках validation.

Module options `mockDevice` и `readOnlyEmpty` сейчас формируют CLI arguments,
которые production `orbis-sessiond` binary не разбирает. Они не должны
использоваться как доказательство отдельного runtime mode; cleanup/implementation
остаётся packaging gap.

Другие distro packages, desktop/AppStream integration, D-Bus activation и
release installation workflow — **NOT IMPLEMENTED**.

## Hardware evidence and UNKNOWN

Последний detailed in-repository hardware snapshot — read-only FA707NV probe от
2026-08-06 (`research-report.md` + fixtures). Он доказывает presence и observed
values того момента, включая `xyz.ljones.Platform`, `xyz.ljones.FanCurves`,
asus-armoury objects, UPower и sysfs. Он не доказывает безопасные writes.

Остаются UNKNOWN:

- hardware charge min/max/step;
- semantics/units/ranges части power-limit raw values;
- versioned raw GPU enum semantics;
- write permissions и safe behavior за пределами read-only evidence;
- behavior на других ASUS models и нескольких батареях.
