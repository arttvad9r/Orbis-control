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

Daemon vertical slice протестирован и live-validated как Nix-installed systemd
user service. Production GUI **подключён** к этому пути через
`orbis-session-client` и **LIVE-VALIDATED** (Scenario A: daemon absent →
Unavailable без mock fallback; Scenario B: daemon present → Ready со значением,
совпадающим с authoritative D-Bus baseline). Production hardware mutations
отсутствуют.

## Major areas

| Area | Status | Фактическое состояние |
|---|---|---|
| Domain/core | IMPLEMENTED | Typed models/invariants для основных областей; optional charge bounds |
| Config | PARTIAL | TOML/XDG/versioned config и atomic store; нет production persistence workflow |
| Capabilities | PARTIAL | Domain model, report assembly, fixture loading/tests; runtime general probe отсутствует |
| Provider contracts | IMPLEMENTED | Traits и error model существуют |
| Broad provider implementation | MOCK-ONLY | `MockProvider` покрывает UI/application scenarios |
| Application layer | IMPLEMENTED | Performance/GPU/Battery commands + authoritative read-back |
| UI | PARTIAL | Slint main window + sequential worker; interactive Battery — real session client (LIVE-VALIDATED), Performance/GPU — mock |
| Session protocol | IMPLEMENTED | Getter-only `Session1.ChargeLimit`, `(bbybyyy)` |
| Session client | IMPLEMENTED | Fresh D-Bus Get, validation, read-only `BatteryProvider` |
| sessiond | LIVE-VALIDATED | Discovery, UPower read, server, runtime, signals, Nix user service |
| CLI | NOT IMPLEMENTED | `orbisctl` binary — stub |
| Production mutations | NOT IMPLEMENTED | Read-only providers return `Unsupported` |
| Privileged helper | NOT IMPLEMENTED | `orbis-hardwared` не входит в workspace |

## Battery Charge Limit

### IMPLEMENTED / LIVE-VALIDATED

Production GUI composition (split worker, один sequential loop):

- Performance/GPU → `MockProvider` (`main_service`);
- Battery → `SessionChargeLimitProvider` через
  `ZbusSessionChargeLimitSource` (`battery_service`);
- `run_worker<M, B, F>` принимает независимые `main_service` и
  `battery_service`; один provider не обязан реализовывать все три traits;
- FIFO/barriers/Battery adjacent coalescing сохранены.

UI Battery state semantics:

- `ChargeLimitState { Loading, Ready, Unavailable }` — честное
  availability/readiness состояние, отдельное от `charge_limit_enabled`;
- initial interactive state = Loading; ровно один authoritative
  `RefreshChargeLimit` при startup через `battery_service.charge_limit()`;
- daemon absent → session-client error → Unavailable **без mock fallback**;
- daemon present → Ready с фактическим значением из sessiond;
- `charge_limit_writable=false` для production session backend; mutation
  control disabled; `charge_limit_enabled` остаётся hardware state и не
  используется как writability.

Domain model:

```text
ChargeLimit { enabled, percent: Option<Percent>, bounds: Option<ChargeLimitBounds> }
```

- `bounds=None` поддержан end-to-end и означает UNKNOWN hardware constraints;
- Session wire signature: `(bbybyyy)` с canonical absent values;
- Session client валидирует untrusted wire payload и не использует property cache;
- sessiond UPower provider, battery discovery, composition и D-Bus server
  реализованы;
- UPower provider читает support/enabled/end-threshold через
  `CacheProperties::No` и возвращает `bounds=None`;
- P2P tests покрывают protocol, service/server, client и composed path без real
  system/session bus.

### LIVE-VALIDATED (2026-08-09)

Packaged GUI + packaged sessiond (direct binaries, без systemd):

- Scenario A (daemon absent): user D-Bus name отсутствовал, daemon process
  отсутствовал; GUI остался жив; Vision MCP подтвердил `Battery backend
  unavailable`, slider отсутствует, `80%` не отображается; mock Battery
  fallback отсутствует; Performance/GPU карточки продолжают отображаться.
- Scenario B (packaged sessiond работает): daemon занял
  `io.github.orbiscontrol.Session`; RAW authoritative property:
  `(bbybyyy) false true 80 false 0 0 0` (enabled=false, percent_present=true,
  observed percent=80, bounds_present=false, min/max/step=0 canonical absent
  bounds); Vision MCP подтвердил Battery section `80%`, labels `40`/`100`,
  отсутствие `Battery backend unavailable` → Ready; GUI `80%` совпал с
  authoritative D-Bus baseline `80`.
- Read-only/disabled slider доказан **pixel analysis** Palette tokens: active
  accent `#2DA8F2` отсутствует в Battery slider; track = surface-disabled
  `#292929`; thumb/labels = text-disabled `#777777`. (Vision-модель ошибочно
  интерпретировала disabled style как active — disabled/read-only state
  доказан pixel analysis + production `charge_limit_writable=false`, не
  интерпретацией Vision.)
- Loading transition визуально не пойман из-за скорости; startup refresh
  доказан code/tests и конечными Ready/Unavailable сценариями.
- Hardware writes отсутствовали.

Семантика observed value:

- `80` — observed live value на момент validation, **НЕ hardware constant**;
- `bounds_present=false`, canonical `min/max/step=0`; hardware bounds остаются
  UNKNOWN;
- `40`/`100` — presentation policy slider labels, не доказанные hardware bounds.

Ранее (контролируемый `nixos-rebuild test`) также подтверждены: systemd user
unit с `Type=dbus`, ownership bus name, clean SIGTERM → exit status 0, bus name
released, процесс отсутствует.

### Gaps

- Performance/GPU production backends остаются mock (см. ниже).
- Production `set_charge_limit` и one-shot full charge — **NOT IMPLEMENTED**.
- asusd write provider — **NOT IMPLEMENTED**.
- sysfs fallback/write provider — **NOT IMPLEMENTED**.
- Hardware charge min/max/step на FA707NV — **UNKNOWN**. UI/mock policy 40/100/5
  не является hardware evidence.

## Performance

### Performance production provider

**IMPLEMENTED / LIVE-VALIDATED**

- `KernelPerformanceProvider` + `SysfsKernelPlatformProfileSource`;
- backend: symbolic Linux kernel ABI
  `/sys/firmware/acpi/platform_profile` + `/sys/firmware/acpi/platform_profile_choices`;
- authoritative fresh reads: каждый вызов делает новый source read (кэш
  отсутствует); no-cache доказан deterministic scripted unit test;
- `current` + `available` возвращаются через canonical domain mapping;
- неизвестные значения отклоняются (`ProviderError::Unsupported`), без
  fallback/clamp/подбора ближайшего;
- mutation `set_profile` → `Unsupported`; write path отсутствует;
- opt-in live ignored integration test PASS (обычный `cargo test --workspace`
  live sysfs не читает).

Live observation (FA707NV, 2026-08-09):

- kernel current = `quiet` → `Silent`;
- choices = `quiet balanced performance` → `{Silent, Balanced, Turbo}`;
- provider current/available совпали с raw sysfs через domain mapping.

Это dated/current observation, не универсальная ASUS specification. Domain
также поддерживает `low-power → Silent`, но `low-power` НЕ наблюдался в live
choices на этой машине во время validation и не выдаётся за live-supported
профиль FA707NV.

### Performance application/UI vertical slice

**PARTIAL / MOCK-ONLY в production GUI**

- Domain types, `PerformanceProvider`, application read/set/read-back path,
  sequential worker и UI cards реализованы.
- Mock scenarios и tests покрывают state transitions/errors.
- General capability fixture содержит dated evidence наличия platform profiles
  на FA707NV.
- Production GUI всё ещё использует `MockProvider` для Performance.
- Provider пока не exposed через `Session1`/session client.
- Real Performance mutation отсутствует.
- UI не читает real current profile и не применяет profile к hardware.

## GPU Mode

### GPU runtime power production provider

**IMPLEMENTED / LIVE-VALIDATED**

- Production contract: `SupergfxdGpuPowerProvider` → `GpuPowerProvider` →
  `AppService::gpu_power_state()`;
- provider реализует `Provider` + `GpuPowerProvider` и НЕ реализует legacy
  `GpuProvider` (нет fake/Unsupported методов requested/mux/access);
- backend: ready `zbus::Connection` → `org.supergfxctl.Daemon` →
  `/org/supergfxctl/Gfx` → read-only `Power()`;
- PROVEN enum mapping: 0→Active, 1→Suspended, 2→Off, 3=AsusDisabled→Unknown,
  4=Unknown→Unknown, future unknown raw→Unknown (без clamp/fallback);
- каждый `power_state()` — authoritative fresh read; собственного cache нет;
- live ignored test: raw `Power()=1` → provider `Suspended`; supporting
  read-only PCI `runtime_status=suspended` (не provider contract, только
  consistency evidence);
- никаких GPU writes/state changes.

### GPU capability architecture

**IMPLEMENTED / ACCEPTED** (ADR 0005)

- independent `GpuPowerProvider` trait существует;
- `AppService<P>::gpu_power_state()` independent getter существует (вызывает
  только `provider.power_state()`);
- power-only provider regression-tested (PowerOnlyProvider без legacy
  `GpuProvider`);
- legacy `GpuProvider` не изменён;
- worker/production GUI GPU mode path пока остаётся на legacy `GpuProvider` /
  `MockProvider`.

### GPU application/UI vertical slice

**PARTIAL / MOCK-ONLY в production GUI**

- Domain разделяет requested mode, physical MUX, access policy и power state.
- `GpuProvider`, application state/read-back, worker и UI states
  (pending/disabled/error) реализованы.
- Mock tests сохраняют applied state при pending Ultimate/Eco.
- Production GUI GPU всё ещё использует `MockProvider`.
- `AppService::gpu_state()` legacy aggregate остаётся fail-fast
  (requested→mux→access→power). Это limitation legacy aggregate; он больше не
  является единственным API для partially available concepts — real power
  доступен через независимый `gpu_power_state()`.
- Production GPU mutations отсутствуют.

### Session1 GPU transport (read-only)

**IMPLEMENTED / LIVE-VALIDATED**

- Session1 exposes независимые read-only properties:
  `GpuPower`, `GpuMux`, `GpuAccess` (wire signature `y`);
- production composition:
  - power → `SupergfxdGpuPowerProvider`;
  - mux/access → `ArmouryGpuProvider`;
  - никаких mega-GpuProvider / GpuMode mappings;
- session client providers: `SessionGpuPowerProvider`,
  `SessionGpuMuxProvider`, `SessionGpuAccessProvider` (НЕ legacy `GpuProvider`);
- semantics: domain `Unknown` передаётся как semantic wire value; missing
  capability → D-Bus `NotSupported`; provider/read error → D-Bus error; unknown
  wire value на client → `Internal`;
- live: `Power wire=1` → Suspended (supergfxd raw=1); `Mux wire=0` → Integrated
  (backend raw=1); `Access wire=0` → Unblocked (backend raw=0);
  `ChargeLimit` regression sanity PASS;
- никаких writes.

### MUX / access production providers

**IMPLEMENTED / LIVE-VALIDATED**

- `ArmouryGpuProvider` + `SysfsArmouryGpuSource`;
- backend: read-only kernel ASUS Armoury firmware-attributes
  `/sys/class/firmware-attributes/asus-armoury/attributes/{gpu_mux_mode,dgpu_disable}/current_value`;
- implements `GpuMuxProvider` + `GpuAccessProvider` (+ `Provider`), НЕ legacy
  `GpuProvider`;
- independent AppService getters: `gpu_mux_state()`, `gpu_access_policy()`;
- PROVEN mapping из kernel 7.1.7 `asus-armoury.c`:
  `gpu_mux_mode`: 0→`Discrete`, 1→`Integrated`;
  `dgpu_disable`: 0→`Unblocked`, 1→`Blocked`;
- live: `mux raw=1` → `Integrated`; `dgpu_disable raw=0` → `Unblocked`;
- каждый вызов — authoritative fresh read; собственного cache нет;
- semantics: present future raw → domain `Unknown`; attribute NotFound →
  `ProviderError::Unsupported`; malformed/empty → `Internal`; прочие I/O → `Io`;
- никаких writes/queued/pending reads.

### Product GpuMode

Eco / Standard / Ultimate / Optimized backend mapping — **NOT PROVEN**.
Не превращать supergfxd Hybrid/Integrated/AsusMuxDgpu в Orbis product
GpuMode автоматически. Optimized остаётся product/session policy, не raw
backend state.

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

## Diagnostics

**IMPLEMENTED / LIVE-VALIDATED**

- production GUI инициализирует один global tracing subscriber в composition
  root (interactive path; offscreen rendering path subscriber не устанавливает);
- инициализация происходит до runtime/session connection/initial Battery
  refresh/Slint event loop;
- default filter без `RUST_LOG` = `warn` (видны WARN/ERROR);
- `RUST_LOG` обрабатывается стандартным EnvFilter; `RUST_LOG=debug`
  live-validated (реально исполняемые winit/sctk DEBUG события и Battery WARN
  видны);
- duplicate global initialization использует non-panicking `try_init()`;
- существующие `tracing::*` callsites не переписывались;
- отсутствие sessiond диагностируется через stderr: live packaged GUI показал
  существующий WARN
  `battery: refresh недоступен: Dbus("org.freedesktop.DBus.Error.ServiceUnknown:
  The name is not activatable")`;
- подтверждён stderr/fmt subscriber; file logging/journald integration
  отсутствует и не заявляется.

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

GUI runtime dependencies (dlopen) упакованы декларативно:

- `orbis-control` обёрнут стандартным Nix `makeWrapper`;
- wrapper добавляет минимальный declarative `LD_LIBRARY_PATH` для
  runtime/dlopen библиотек: wayland, libxkbcommon, fontconfig, libglvnd;
- эти библиотеки находятся в Nix closure пакета;
- EGL предоставляется через vendor-neutral `libglvnd` (не hard-coded Mesa);
- обёрнут только `orbis-control`; `orbis-sessiond` и `orbisctl` не обёрнуты;
- direct packaged startup `env -u LD_LIBRARY_PATH result/bin/orbis-control`
  PASS live: GUI отрисовался, loader errors отсутствуют, Battery без daemon
  корректно показал Unavailable, Performance/GPU UI сохранён.

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
