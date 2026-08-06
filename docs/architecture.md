# Architecture — Orbis Control

> Дата: 2026-08-06. Статус: черновик Этапа 1 (спецификация), подлежит согласованию
> перед Этапом 2. Связанные решения: `docs/adr/0001-rust-and-slint.md`,
> `docs/adr/0002-daemon-boundaries.md`, `docs/adr/0003-gpu-provider-strategy.md`.

## 1. Обзор

Трёхуровневая архитектура:

```
┌─────────────────────────────┐
│  orbis-ui (Slint GUI)       │  user session, НЕ root, нет HW I/O
└──────────────┬──────────────┘
               │ session bus D-Bus (io.github.orbiscontrol.Session)
┌──────────────▼──────────────┐
│  orbis-sessiond (user daemon)│  владеет состоянием, провайдеры, автоматизация
└──────┬───────────────┬──────┘
       │ system D-Bus  │ sysfs/hwmon (read)
┌──────▼───────┐ ┌─────▼──────────────────────────┐
│ asusd, UPower │ │ kernel ABI, hwmon, leds,      │
│ supergfxd,    │ │ backlight, DRM, powercap      │
│ cardwire, fwupd│ │ (read; write — только через   │
│ ppd, logind   │ │  узкий orbis-hardwared, если  │
└──────────────┘ │  нужен root)                   │
                 └────────────────────────────────┘
```

Правила:

- GUI общается **только** с `orbis-sessiond` (session bus), никогда напрямую с
  аппаратными сервисами.
- `orbis-sessiond` — непривилегированный пользовательский демон; работает без GUI.
- `orbis-hardwared` — опциональный root-помощник, добавляется только когда функция
  не покрыта asusd/системными сервисами и требует узкой привилегированной операции.
- CLI (`orbisctl`) использует те же модели данных и D-Bus API, что GUI.

## 2. Workspace и crates

```
crates/
├── orbis-core/           платформенно-независимая модель (без D-Bus/Slint/sysfs)
├── orbis-config/         TOML-конфиг, миграции, атомарная запись, импорт/экспорт JSON
├── orbis-capabilities/   capability-модель, probe-движок, статусы, матрица
├── orbis-providers/      trait-ы провайдеров + реализации (asusd, kernel, UPower, ...)
├── orbis-sessiond/       пользовательский демон: состояние, политики, D-Bus API
├── orbis-ui/             Slint GUI (только представление)
├── orbis-cli/            orbisctl (те же модели/API)
├── orbis-hardwared/      опциональный root-helper (allowlist, polkit, sandbox)
└── orbis-test-support/   mock-провайдеры, фикстуры, snapshot-инструменты
```

Зависимости направления: `ui → sessiond(API) → providers → core/config/capabilities`.
`hardwared` — отдельно, общается с sessiond по системе D-Bus с polkit.

## 3. Модель данных (orbis-core)

Типы (все `Serialize + Deserialize` для D-Bus/JSON, `Clone + Debug + PartialEq`):

- `PerformanceProfile { Silent, Balanced, Turbo, Custom(String) }`
- `GpuMode { Eco, Standard, Ultimate, Optimized }`
- `GpuMuxState { Integrated, Discrete, Unknown }`
- `GpuAccessPolicy { Unblocked, Blocked, Pending, Unknown }` (Cardwire)
- `PowerSource { Ac, Battery, UsbCPdLowPower, Unknown }`
- `ChargeLimit { Enabled(u8), Disabled, Unsupported }` (min/max/step отдельно)
- `FanId { Cpu, Gpu, Mid, System, Other(String) }`
- `FanCurvePoint { temp: f32, pwm: u16 }` (или процент)
- `FanCurve { fan: FanId, points: Vec<FanCurvePoint> }`
- `PowerLimits { spl, sppt, fppt, cpu_temp_limit, gpu_dynamic_boost, gpu_temp_target, ... }`
  — каждое поле с `PowerLimitValue { value, min, max, step, default, unit }`
- `DisplayMode { current_refresh_hz, modes: Vec<RefreshMode>, overdrive: Option<bool>, hdr: Option<HdrState> }`
- `LightingMode { Off, Static, Breathing, Strobing, ColorCycle, Rainbow, Custom(String) }`
- `DeviceCapabilities` — матрица функций → `CapabilityStatus`
- `CapabilityStatus` — enum из §7 задания + метаданные (причина, сообщение, действие,
  backend, endpoint, требование reboot/logout, риск, время проверки)
- `ProviderStatus { Healthy, Degraded, Unavailable }`
- `ActionRequirement { None, Logout, Reboot, Confirmation, Experimental }`
- `ApplyResult { Applied, Pending(ActionRequirement), Failed { reason } , RolledBack { reason } }`
- `PendingAction { id, target, requirement, created_at, cancelable }`
- `Warning { severity, code, message, details }`
- `DiagnosticEntry { key, value, severity, source }`
- `AutomationRule { id, trigger, action, priority, cooldown }`
- `HardwareSnapshot { cpu_temp, gpu_temp, fan_rpms, battery, power, gpu_power_state, ts }`

## 4. Capability-модель (orbis-capabilities)

Статусы: `Supported, SupportedWithRequirement, ReadOnly, TemporarilyUnavailable,
Unsupported, BackendMissing, PermissionDenied, Experimental, Conflicted, Unknown`.

Для каждого статуса хранится: техническая причина; короткое сообщение для UI;
рекомендуемое действие; backend; путь/D-Bus endpoint; требование reboot/logout;
уровень риска; время последней успешной проверки.

Пример (из реального introspection FA707NV):

```text
GPU Ultimate:
status: SupportedWithRequirement
provider: asusd / xyz.ljones.AsusArmoury (/xyz/ljones/asus_armoury/gpu_mux_mode)
requirement: Reboot
reason: Hardware MUX value is latched by firmware
```

Правила:

- Неизвестная функция → `Unknown`, не `Unsupported`.
- `Unsupported` только после достаточной проверки (probe + попытка чтения).
- Причина недоступности не прячется в лог — видна в tooltip/подзаголовке/диагностике.
- Полностью неприменимые секции скрываются (главное окно остаётся компактным).

## 5. Провайдеры (orbis-providers)

Базовый контракт (все провайдеры):

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn backend_id(&self) -> &'static str;
    fn backend_version(&self) -> Option<String>;
    fn risk(&self) -> RiskLevel; // Safe | Confirmation | Dangerous | Experimental
    fn timeout(&self) -> Duration;

    async fn probe(&self) -> Result<CapabilityReport, ProviderError>;
    fn capabilities(&self) -> &CapabilityMatrix;
    async fn read_state(&self) -> Result<ProviderState, ProviderError>;
    async fn health(&self) -> ProviderHealth;
    fn diagnostics(&self) -> Vec<DiagnosticEntry>;
    fn explain_unsupported(&self, feature: FeatureId) -> String; // человекочитаемое
}
```

Отдельные trait-ы: `PerformanceProvider`, `FanProvider`, `PowerLimitProvider`,
`BatteryProvider`, `GpuProvider` (декомпозирован на `MuxProvider`,
`GpuAccessProvider`, `GpuPowerStateProvider`), `DisplayProvider`,
`LightingProvider`, `AnimeProvider`, `SlashProvider`, `HotkeyProvider`,
`TelemetryProvider`, `FirmwareUpdateProvider`.

Проверка поддержки записи (не верить существованию файла):

1. существование; 2. тип; 3. доступность чтения; 4. доступность записи;
5. допустимый диапазон; 6. read-back; 7. стабильность значения;
8. соответствие DMI/устройству; 9. отсутствие конфликтующего владельца.

Ранжирование провайдеров: registry `Vec<Box<dyn Provider>>` с приоритетом; при
probe выбирается первый Healthy. При его падении — следующий (fallback с пометкой
technical debt, если это CLI-обёртка).

## 6. orbis-sessiond

### 6.1 Обязанности

- владение текущим состоянием приложения;
- агрегация данных провайдеров;
- применение автоматических политик (AutomationEngine);
- подписка на UPower и D-Bus сигналы;
- отслеживание подключения/отключения дисплеев;
- pending actions (MUX и т.п.);
- таймауты и повторы;
- единый session D-Bus API для GUI/CLI;
- предотвращение конфликта двух экземпляров (single-instance через name ownership);
- работа без открытого GUI;
- корректное завершение при logout (systemd user unit, `KillMode=control-group`,
  ловит `SIGTERM`, снимает свой D-Bus name, сохраняет состояние).

### 6.2 D-Bus API (session bus)

Имя: `io.github.orbiscontrol.Session`
Объект: `/io/github/orbiscontrol/Session`
Интерфейс: `io.github.orbiscontrol.Session1` (версионирование API в имени; при
ломающих изменениях — `Session2` и т.д.; XML-описания в `data/dbus-1/`).

Методы (типизированные, через zbus):

```
GetCapabilities()         -> a{sv}           // capability matrix
GetStatus()               -> a{sv}           // профиль, GPU, лимит, дисплей, освещение
GetTelemetry()            -> a{sv}           // HardwareSnapshot
GetDiagnostics()          -> a{sv}           // для окна Diagnostics
SetPerformanceProfile(u)  -> (u, s)          // ApplyResult + human message
SetGpuMode(u, b)          -> (u, s)          // Optimized-флаг подтверждения
SetChargeLimit(u)         -> (u, s)
GetFanCurves(u)           -> a(s(yyyyyyyy)(yyyyyyyy)b)
SetFanCurve(u, v)         -> (u, s)
SetFanCurvesEnabled(u, b) -> (u, s)
SetPowerLimit(s, i)       -> (u, s)
SetDisplayMode(u)         -> (u, s)
SetLightingMode(v)        -> (u, s)
SetAnimeMode(v)           -> (u, s)
SetSlashMode(v)           -> (u, s)
GetPendingActions()       -> a{sv}
CancelPendingAction(s)    -> (u, s)
ApplyConfig(v)            -> (u, s)          // применение конфига
ReProbe()                 -> (u, s)
Quit(b)                   -> ()              // b = завершить и демон
```

Сигналы:

```
StateChanged(a{sv})
TelemetryUpdated(a{sv})          // coalesced, не чаще 1 Гц
CapabilityChanged(a{sv})
PendingActionChanged(a{sv})
Warning(a{sv})
```

Свойства времени: все операции с таймаутами; повторные команды отменяют
устаревшие (cancellation tokens); ползунки — debounce; события питания/дисплея —
coalescing.

### 6.3 Внутренняя структура

```
SessionDaemon
├── State (Arc<RwLock<AppState>>)         // пользовательское намерение + подтверждённое HW
├── ProviderRegistry
├── AutomationEngine                       // правила, приоритеты, cooldown
├── PendingActionStore
├── UPowerSubscription
├── DisplayMonitor
├── LogindSubscription (PrepareForSleep)
└── DbusApi
```

Политика владения: если обнаружены и asusd (или ppd) и наша автоматизация — выбор
владельца `platform_profile` подтверждается пользователем (диалог), конфликт
отображается в Diagnostics.

## 7. orbis-ui

- Никакого аппаратного I/O, никакого `sudo`, никакого парсинга shell-вывода.
- Отображает состояние от sessiond; шлёт типизированные команды.
- Показывает pending/success/warning/failure.
- Оптимистичное обновление — только для безопасных обратимых настроек (например,
  яркость), при этом после подтверждения backend состояние синхронизируется.
- Для GPU MUX, power limits, undervolting и fan curves — ожидание подтверждения backend.
- Стек: Slint, отдельные `.slint`-компоненты (см. `docs/ui-reference.md`),
  дизайн-токены в `ui/themes/`, иконки — единый свободный набор (Lucide/Tabler).

## 8. orbis-hardwared (опциональный)

Добавляется только при доказанной необходимости (см. ADR 0002). Требования:

- минимальный API; никакой передачи произвольных путей/команд; allowlist атрибутов;
- диапазоны на стороне демона; read-back; Polkit-проверка;
- журналирование аппаратных изменений; защита от symlink/path traversal;
- `ProtectSystem=strict`, `PrivateTmp=true`, `NoNewPrivileges=true`,
  минимальные capabilities, seccomp/systemd sandboxing;
- отдельный threat model (`docs/threat-model.md`).

Пример (когда понадобится): запись `charge_control_end_threshold` на системах без
asusd, где sysfs-файл доступен только root. Не создаётся «на всякий случай».

## 9. orbis-cli

```
orbisctl status | capabilities | diagnostics [--json] | profile get|set <...>
orbisctl gpu get|set <eco|standard|ultimate|optimized>
orbisctl battery get-limit|set-limit <percent>
orbisctl fan list | curve get
orbisctl config validate
orbisctl providers
orbisctl version
orbisctl hardware-test --allow-writes   // отдельный интерактивный режим, НЕ в CI
orbisctl --safe-mode / orbis-sessiond --read-only  // safe mode
```

CLI использует тот же D-Bus API и модели данных, что GUI. `anyhow` — только на
верхнем уровне CLI/launcher; внутри — `thiserror`.

## 10. Конфигурация (orbis-config)

- TOML пользовательский конфиг; JSON только для экспорта/импорта/совместимости.
- `config_version`, миграции с автоматическим бэкапом перед миграцией.
- Атомарная запись: temp file + rename.
- Раздельное хранение: (а) пользовательское намерение; (б) последнее подтверждённое
  аппаратное состояние; (в) pending state; (г) backend-specific metadata.
- Импорт: валидация schema, preview, список изменений; неизвестные команды/hotkeys
  отключены; hardware-specific значения на другой модели не применяются;
  fan curves перепроверяются.
- Не сохранять unsupported значения как применённые.

## 11. Асинхронность и производительность

- Tokio runtime; все D-Bus/sysfs/внешние операции — async с таймаутами.
- Отмена устаревших запросов при повторном изменении; debounce ползунков;
  coalescing событий питания/дисплея.
- Polling: fast telemetry 1 s (только при открытом окне — подписка); slow
  capabilities 30–60 s или event-driven; static probe — при старте/resume/
  restart провайдера/ручном re-probe.
- Целевые метрики (после стабилизации): daemon CPU < 0.2 %, RAM < 20 MiB;
  GUI RAM < 80 MiB; cold start < 1.5 s; UI response < 100 ms.

## 12. Обработка ошибок и rollback

- Запрещены «Something went wrong» без деталей; всегда: что пошло не так, backend,
  состояние системы, что делать.
- Последовательность для обратимых операций: прочитать исходное состояние →
  валидировать → применить → прочитать новое → при частичной ошибке попытаться
  вернуть исходное → записать результат → success только после подтверждения.
- Rollback физического MUX — без самостоятельного выдумывания firmware-семантики
  (только RestoreDefault/через backend).

## 13. Безопасность (кратко)

Подробно — `docs/threat-model.md`. Ключевые границы:

- GUI/sessiond: user session, никаких привилегий.
- hardwared: root, polkit, allowlist, sandbox.
- D-Bus policy: sessiond виден только пользователю; hardwared методы — polkit.
- Диагностический экспорт обезличивается.
- Никакой телеметрии и рекламы.

## 14. Тестирование

- unit tests + `proptest` (кривые, диапазоны);
- mock D-Bus services (`orbis-test-support`);
- snapshot-тесты capability-модели;
- screenshot-тесты GUI (slint-viewer/рендер в CI);
- hardware-in-the-loop — только отдельно от CI, интерактивно
  (`orbisctl hardware-test --allow-writes`);
- матрица CI: latest stable, MSRV, Fedora/Ubuntu/Arch контейнеры,
  Wayland-headless где возможно.

## 15. Пакетирование (кратко)

PKGBUILD, Fedora spec, Debian packaging, AppImage; `.desktop`, AppStream, SVG icon;
systemd user unit; D-Bus activation; polkit policy (при наличии hardwared);
uninstall-документация. Установка не отключает чужие сервисы, не меняет grub,
не ставит модули ядра, не добавляет пользователя в широкие группы.
