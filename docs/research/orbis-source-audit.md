# Orbis Control — Source-Level Audit

> Роль: **RESEARCH / SNAPSHOT**. Независимый source-level аудит репозитория
> `orbis-control` по состоянию на HEAD `f13b017` (2026-08-14). Никакой код не
> изменялся; только чтение, сборка и тесты.
>
> Дата аудита: 2026-08-17.
> Метод: чтение всех Rust-исходников, Slint UI, Nix packaging, docs, ADR,
> fixtures и git history; `cargo check/test/clippy/fmt` на свежем target dir.
>
>
> Исторический snapshot: последующая Task 3 (после этого audit) устранила
> production dependency на `MockProvider` для GPU product state. Claims ниже о
> `MockProvider` как legacy production path описывают состояние на дату audit и
> не являются текущим production architecture.

---

## 1. Executive summary

Orbis Control — это **ранний, но архитектурно зрелый** Rust + Slint проект для
управления ASUS-ноутбуками на Linux (аналог G-Helper). Проект находится в
стадии `0.1.0` и прошёл путь от «исследование + mock-first прототип» до
**первого read-only MVP и двух узких live-validated mutation-путей**
(Performance profile и Battery charge limit).

Ключевая особенность проекта — **необычно строгая инженерная дисциплина**:

- capability-driven архитектура: поддержка доказывается probe/read, а не
  предполагается по модели ноутбука или имени файла;
- честная модель состояния: `Unknown` ≠ `Unsupported` ≠ `Unavailable` ≠
  `ReadOnly`; никакого mock fallback в production;
- authoritative read-back после каждой мутации; никакого optimistic state;
- узкий привилегированный helper `orbis-hardwared` вместо универсального
  root-демона; polkit-авторизация по `system-bus-name` original caller;
- kernel-first подход: стандартные ABI (`platform_profile`, `power_supply`,
  `firmware-attributes`) в приоритете; ASUS-specific через asusd/supergfxd.

**Что реально работает (live-validated):** read-only отображение Battery Charge
Limit, Performance Mode, GPU Power/MUX/Access через session path; controlled
mutation Performance (`platform_profile` через hardwared) и Battery
(`asusd` через hardwared).

**Что НЕ реализовано:** fans (RPM/curves), power limits, lighting/RGB, AniMe/
Slash, display controls, telemetry (mock-only), automation engine, CLI (stub),
suspend/resume, persistence workflow, GPU product-mode mutation.

Проект **уже совпадает** с современным kernel-first/capability-driven подходом
по большинству архитектурных принципов. Основные пробелы — в объёме
реализованных hardware-функций, а не в архитектуре.

---

## 2. Текущее назначение проекта

Из `README.md`, `docs/product.md` и `docs/architecture.md`:

- нативное Rust + Slint приложение для наблюдения и управления возможностями
  ASUS-ноутбуков на Linux;
- Wayland-first, X11 — compatibility mode;
- GUI работает от обычного пользователя и не выполняет direct hardware I/O;
- системные детали скрыты за provider/service boundaries;
- capability-driven: поддержка доказывается, а не предполагается;
- read-only capability никогда не выдаётся за write capability;
- значения после команд подтверждаются authoritative read-back;
- `orbis-hardwared` — узкий production root-helper только для доказанных
  привилегированных операций, не generic sysfs writer.

Продукт нацелен на честное отображение состояния и безопасные действия, а не
на «всё работает как в Windows».

---

## 3. Фактическая архитектура

Восстановленная по коду схема (не из README, а из фактических вызовов):

```text
Slint UI (orbis-ui, app-window.slint)
        │  callbacks → typed WorkerCommand
        ▼
sequential async worker (orbis-ui/src/worker.rs, run_worker)
        │  FIFO, Battery coalescing, authoritative read-back
        ▼
orbis-application::AppService<P> (orbis-application/src/lib.rs)
        │  типизированные команды + read-back
        ▼
orbis-providers traits (orbis-providers/src/traits.rs)
        │
        ├── MockProvider (GPU product mode, tests, offscreen)
        ├── SessionHardwarePerformanceProvider / SessionHardwareBatteryProvider
        │     (orbis-session-client) — Session1 read + direct Hardware1 mutation
        └── SessionGpuPower/Mux/AccessProvider (read-only)
              │
              ▼
        orbis-sessiond (user daemon, session bus)
              │  Session1 interface (getter-only)
              │  providers: UPower/asusd/kernel platform_profile/supergfxd/Armoury
              ▼
        system services: UPower, asusd, supergfxd, kernel sysfs
              │
              ▼
        orbis-hardwared (root helper, system bus, Hardware1)
              │  polkit (system-bus-name original caller)
              ▼
        fixed kernel writer (platform_profile) / typed asusd D-Bus / supergfxd
```

### Ключевые границы

- **GUI → worker → AppService → provider traits**: GUI не знает о D-Bus/sysfs.
- **Read path**: GUI → Session1 (sessiond) → authoritative backend.
- **Write path (Performance/Battery)**: GUI/application caller → **direct
  Hardware1** (system bus) → hardwared → polkit → backend → read-back. Session1
  **не** делегирует mutation (confused-deputy защита, ADR 0006).
- **GPU product mode**: остаётся на MockProvider (legacy), production controls
  disabled; read-only hardware status (Power/MUX/Access) — через независимые
  capability providers (ADR 0005).

---

## 4. Cargo/workspace структура

`Cargo.toml` (workspace, resolver 3, edition 2024, MSRV 1.85):

| Crate | Роль | Завершённость |
|---|---|---|
| `orbis-core` | Domain types, newtypes, invariants | IMPLEMENTED |
| `orbis-config` | TOML/XDG config, atomic store | PARTIAL |
| `orbis-capabilities` | Capability model, fixtures, report | PARTIAL |
| `orbis-providers` | Provider traits, errors, MockProvider, supergfxd contract, native-eco preflight | IMPLEMENTED (traits) / MOCK-ONLY (broad) |
| `orbis-application` | Async use cases + authoritative read-back | IMPLEMENTED |
| `orbis-session-protocol` | Getter-only D-Bus wire contract | IMPLEMENTED |
| `orbis-session-client` | Session1 reads + direct Hardware1 mutation providers | IMPLEMENTED |
| `orbis-sessiond` | User daemon, UPower/asusd/kernel/supergfxd/Armoury adapters | LIVE-VALIDATED |
| `orbis-ui` | Slint presentation, worker, controller | PARTIAL |
| `orbis-cli` | CLI (`orbisctl`) | STUB |
| `orbis-test-support` | Deterministic mock device profiles | IMPLEMENTED |
| `orbis-hardwared` | Privileged root helper (Performance/Battery/GPU mutation) | LIVE-VALIDATED |

**Важное расхождение:** `AGENTS.md` (строки 35, 183) утверждает, что
`orbis-hardwared` «deliberately NOT in the workspace (ADR 0002)». Фактически
`Cargo.toml` (строки 15–19) **включает** `crates/orbis-hardwared` в workspace
members с комментарием «включён после ADR 0006». Это **документационный drift**:
AGENTS.md устарел относительно фактического состояния. ADR 0002 (статус-раздел)
тоже говорит «hardwared: не создаётся», но ADR 0006/0007/0008 его ввели.

---

## 5. GUI/Slint

- `ui/app-window.slint` (534 строки) — главное окно 425×~441px, секции
  Performance, GPU, Battery Charge Limit, Telemetry, footer.
- Компоненты: `ui/components/{mode-card,section-header,value-slider}.slint`,
  тема `ui/themes/dark.slint`.
- `orbis-ui/src/main.rs` (1830 строк) — composition root: создаёт session/
  system connections, worker, сервисы, callbacks, offscreen SoftwareRenderer
  для `--screenshot`.
- `orbis-ui/src/worker.rs` (2219 строк) — последовательный async worker с FIFO
  и Battery coalescing.
- `orbis-ui/src/controller.rs` (658 строк) — legacy UI-state model helper
  (`#[allow(dead_code)]` в main.rs: «Контроллер сохраняется как boundary/model
  helper для legacy unit tests»).

**Статус:** UI отображает реальные session-значения (Battery, Performance,
GPU Power/MUX/Access) — LIVE-VALIDATED. Product GPU mode — mock-only, controls
disabled. Telemetry-секция показывает mock-значения (не real).

---

## 6. Daemon и IPC

### orbis-sessiond (user daemon)

- `main.rs` → `runtime.rs::run_discovered_sessiond()`: bootstrap + ожидание
  SIGINT/SIGTERM.
- `bootstrap.rs`: открывает system bus (UPower) + session bus, discovery
  батареи, собирает GPU capability providers (supergfxd power, Armoury
  mux/access) и kernel performance provider.
- `service.rs`: `SessionService` + zbus interface `io.github.orbiscontrol.Session1`
  (getter-only properties: `ChargeLimit`, `GpuPower`, `GpuMux`, `GpuAccess`,
  `Performance`).
- `upower.rs`: `AsusdBatteryChargeLimitProvider` (enabled←UPower,
  configured←asusd, effective←kernel sysfs).
- `performance.rs`: `KernelPerformanceProvider` (kernel `platform_profile`).
- `armoury.rs`: `ArmouryGpuProvider` (kernel firmware-attributes).
- `supergfxd.rs`: `SupergfxdGpuPowerProvider` (supergfxd `Power()`).
- `discovery.rs`: UPower `EnumerateDevices` → ровно одна system battery.

### orbis-session-protocol

Getter-only wire contract: `ChargeLimitInfo` (signature `(bbybybyyy)`),
`PerformanceInfo` (`(yy)`), GPU wire constants. DTOs — untrusted input,
валидируются на client boundary.

### orbis-session-client

`SessionChargeLimitProvider`, `SessionGpuPower/Mux/AccessProvider`,
`SessionPerformanceProvider` (read-only через Session1), и composed
`SessionHardwarePerformanceProvider` / `SessionHardwareBatteryProvider`
(read через Session1, mutation через direct Hardware1).

### orbis-hardwared (root helper, system bus)

- `Hardware1` interface: `SetPerformanceProfile(y)->y`,
  `SetChargeLimit(u8)->u8`, `SetGpuMode(u32)->GpuMutationResult`.
- polkit authorizer по `system-bus-name` original caller.
- `PlatformProfileWriter` — фиксированный kernel writer с read-back.
- `AsusdBatteryMutationBackend` — typed asusd D-Bus setter + read-back.
- `SupergfxdMutationBackend` — staged supergfxd contract (GPU, не live).

---

## 7. Hardware abstraction

Provider traits (`orbis-providers/src/traits.rs`): `Provider` (base),
`PerformanceProvider`, `FanProvider`, `PowerLimitProvider`, `BatteryProvider`,
`GpuProvider` (legacy), `GpuPowerProvider`, `GpuMuxProvider`, `GpuAccessProvider`
(ADR 0005 split), `DisplayProvider`, `LightingProvider`, `AnimeProvider`,
`HotkeyProvider`, `TelemetryProvider`, `AutomationProvider`,
`FirmwareUpdateProvider`.

Каждый provider: `id()`, `backend()`, `timeout()`, `explain_unsupported()`,
`health()`, `diagnostics()`. Ошибки: `ProviderError` с классами
`BackendUnavailable/Unsupported/PermissionDenied/InvalidRequest/Timeout/Io/
Dbus/Internal`.

**Реальные production backends** (не mock):

| Backend | Интерфейс | Файл |
|---|---|---|
| UPower | D-Bus `org.freedesktop.UPower.Device` | `sessiond/upower.rs` |
| asusd | D-Bus `xyz.ljones.Platform` (configured threshold) | `sessiond/upower.rs`, `hardwared/battery.rs` |
| kernel `platform_profile` | sysfs `/sys/firmware/acpi/platform_profile(_choices)` | `sessiond/performance.rs`, `hardwared/lib.rs` |
| kernel `power_supply` | sysfs `charge_control_end_threshold` | `sessiond/upower.rs`, `hardwared/battery.rs` |
| supergfxd | D-Bus `org.supergfxctl.Daemon` `Power()` | `sessiond/supergfxd.rs` |
| kernel ASUS Armoury | sysfs `firmware-attributes/.../gpu_mux_mode`, `dgpu_disable` | `sessiond/armoury.rs` |

---

## 8. Подробный разбор поддерживаемых hardware capabilities

### 8.1 Performance Mode — **IMPLEMENTED / LIVE-VALIDATED**

- **Read:** kernel `platform_profile` / `platform_profile_choices` (standard
  symbolic ABI) → `KernelPerformanceProvider` → Session1 → GUI.
- **Write:** GUI caller → Hardware1 → hardwared → polkit → fixed
  `platform_profile` writer → read-back → fresh Session1 read-back.
- Mapping: `Silent→quiet`, `Balanced→balanced`, `Turbo→performance`.
- Root required для write (файл 0644 root:root). Capability discovery: проверка
  присутствия символа в `platform_profile_choices` перед write.
- Live: `Balanced → Silent → Balanced` подтверждён ровно двумя Hardware1 calls.

### 8.2 Battery Charge Limit — **IMPLEMENTED / LIVE-VALIDATED**

- **Read:** `enabled`←UPower `ChargeThresholdEnabled`; `configured`←asusd
  `ChargeControlEndThreshold`; `effective`←kernel
  `charge_control_end_threshold`.
- **Write:** GUI caller → Hardware1 → hardwared → polkit → typed asusd D-Bus
  setter → asusd (owner) → kernel + persistence. Direct sysfs write запрещён
  при активном asusd (competing-owner risk, ADR 0007).
- Contract: `20..=100`, step 1. `100` — обычный threshold, не disable.
- Live: `100 → 80 → 100` подтверждён.

### 8.3 GPU Power (runtime) — **IMPLEMENTED / LIVE-VALIDATED**

- supergfxd `Power()` → `GpuPowerState`. Mapping PROVEN: 0→Active, 1→Suspended,
  2→Off, 3=AsusDisabled→Unknown (консервативно), 4→Unknown, future→Unknown.
- Read-only; не будит dGPU.

### 8.4 GPU MUX — **IMPLEMENTED / LIVE-VALIDATED (read-only)**

- kernel ASUS Armoury `gpu_mux_mode/current_value` → `GpuMuxState`.
  Mapping PROVEN из kernel 7.1.7: 0→Discrete, 1→Integrated.
- Mutation (Ultimate) — **NOT IMPLEMENTED** (reboot-latched; ADR 0008 staged
  supergfxd contract принят, live mutation не разрешена).

### 8.5 GPU Access (dGPU disable) — **IMPLEMENTED / LIVE-VALIDATED (read-only)**

- kernel ASUS Armoury `dgpu_disable/current_value` → `GpuAccessPolicy`.
  Mapping PROVEN: 0→Unblocked, 1→Blocked.
- Mutation (Eco) — **NOT IMPLEMENTED**; native live Eco — только read-only
  preflight (`native_asus_eco.rs`, ADR 0009).

### 8.6 Fans (RPM/curves) — **NOT IMPLEMENTED**

- Domain (`fan.rs`), traits (`FanProvider`), mock curves есть.
- Production provider/UI editor отсутствуют. Dated fixtures доказывают
  hwmon `fan*_input` и asusd `FanCurves` на FA707NV, но production backend нет.

### 8.7 Power limits — **NOT IMPLEMENTED**

- Domain (`limits.rs`), traits есть. Units/ranges не доказаны (FA707NV:
  asusd `Unsupported` ENODEV, kernel read `Supported`, write `PermissionDenied`).

### 8.8 Lighting/RGB/Aura — **NOT IMPLEMENTED**

- Domain (`lighting.rs`), traits есть. Production backend нет.

### 8.9 AniMe Matrix / Slash — **NOT IMPLEMENTED**

- Trait/research only. FA707NV fixture отмечает устройства отсутствующими.

### 8.10 Display controls — **NOT IMPLEMENTED**

- Domain (`display.rs`), traits есть. Production backend нет.

### 8.11 Telemetry — **MOCK-ONLY**

- Domain (`telemetry.rs`), traits есть. UI показывает mock temperatures/fans/
  battery/power. Real telemetry provider отсутствует.

### 8.12 Native ASUS Eco preflight — **READ-ONLY FOUNDATION (ADR 0009)**

- `native_asus_eco.rs` (1042 строки): pure classifier + read-only host source,
  классифицирует `Ready/Blocked/Unsupported/Inconsistent` и строит typed
  release plan. Никаких writes/process kills/PCI ops.

---

## 9. Monitoring/telemetry

- Domain `Telemetry`/`HardwareSnapshot`/`BatteryTelemetry`/`FanTelemetry`/
  `PowerTelemetry` существуют.
- `TelemetryProvider` trait есть; `MockProvider` реализует.
- **Real telemetry provider отсутствует.** UI Telemetry-секция показывает
  mock-значения. Нет hwmon/powercap/thermal чтения в production.

---

## 10. Profiles/config/state

- `orbis-config`: TOML/XDG, atomic write (temp+rename), versioning (v1),
  backup-before-migration, validation. Секции: UI, Automation, Battery,
  Experimental.
- **Нет production persistence workflow**: конфиг не читается/пишется в
  production GUI/sessiond. `load_or_default()` не вызывается в production path.
- Automation domain (`automation.rs`) и config (`AutomationConfig`) есть, но
  automation engine в sessiond **не реализован**.
- `orbis-capabilities`: capability model, fixture loading, report assembly.
  **Runtime general capability probe отсутствует** — только dated fixtures.

---

## 11. NixOS/systemd/polkit integration

- `flake.nix`: `nixosModules.orbis-control`, `packages.orbis-control`,
  `devShells.default`, `checks.default` (fmt/clippy/test), VM checks
  (`hardwared-lifecycle`, `performance-mutation-vm`, `battery-mutation-vm`).
- `packaging/nix/module.nix`: user service `orbis-sessiond` (`Type=dbus`,
  `BusName=io.github.orbiscontrol.Session`, `graphical-session.target`); system
  service `orbis-hardwared` (`Type=dbus`, sandbox: `ProtectSystem=strict`,
  `PrivateDevices`, `NoNewPrivileges`, `CapabilityBoundingSet=""`,
  `ReadOnlyPaths=/sys`, `ReadWritePaths=platform_profile`).
- `packaging/nix/package.nix`: `buildRustPackage`, `makeWrapper` для GUI
  (dlopen libs), D-Bus system policy, polkit actions.
- Polkit actions: `set-performance-profile`, `set-charge-limit`, `set-gpu-mode`
  (все `allow_any=no, allow_inactive=no, allow_active=yes`).
- D-Bus policy: root owns `io.github.orbiscontrol.Hardware`; любой caller может
  send (авторизация — polkit внутри hardwared).
- **Gap:** module options `mockDevice`/`readOnlyEmpty` формируют CLI args,
  которые production `orbis-sessiond` не разбирает (dead options).
- Другие distro packages (appimage/arch/debian/fedora — пустые каталоги),
  desktop/AppStream, D-Bus activation — **NOT IMPLEMENTED**.

---

## 12. Suspend/resume

**NOT IMPLEMENTED.** Нет logind `PrepareForSleep` подписки, нет re-apply
логики, нет automation на resume. Domain `AutomationTrigger::OnResume` и
`LidClosed` существуют, но engine отсутствует. ADR 0007 упоминает asusd
restore-on-resume как внешний owner, но Orbis сам не обрабатывает resume.

---

## 13. Tests и validation

- **420 Rust tests PASS** (свежий target dir), 0 failed, 3 ignored (live
  hardware tests: `live_gpu_mux_access`, `live_gpu_power`, `live_performance`).
- `cargo fmt --check` PASS, `cargo clippy --workspace --all-targets -- -D warnings`
  PASS, `git diff --check` PASS.
- P2P D-Bus tests (private bus, без реального system/session bus): session
  protocol, server, client, upower discovery/composition, battery/performance/
  gpu hardware, supergfxd mutation.
- Privacy tests: fixtures не содержат персональных данных.
- VM checks (flake): hardwared lifecycle, performance mutation, battery mutation.
- **Примечание:** сборка в существующем `target/` падает из-за stale артефакта
  от прежнего checkout-пути (`/home/artt/Проекты/...`, кириллица). Это не дефект
  проекта; на свежем target всё собирается.

---

## 14. Git archaeology

132 коммитов, одна ветка `main`, без тегов/remotes. Хронология:

- **2026-08-06 (Этап 0/1):** `6a7c600` исследование+спецификация; `9ed28b3`
  domain models; `4accadf` config+capabilities; `9909a84` provider traits+mock;
  `b9558c6` placeholder binaries; `5c252ad` mock-first UI prototype.
- **2026-08-06/07:** UI worker (performance/gpu/charge), application service
  boundaries, session protocol/daemon.
- **2026-08-08:** sessiond lifecycle, discovery, Nix user service.
- **2026-08-09:** read-only MVP (battery/performance/gpu session paths),
  diagnostics, packaging runtime, GPU capability split (ADR 0005), hardwared
  security boundary + performance write (ADR 0006).
- **2026-08-10/11:** performance mutation E2E, hardwared lifecycle VM, direct
  Hardware1 path, polkit authorization refinement.
- **2026-08-12:** verification runner, battery configured/effective split.
- **2026-08-13:** battery mutation backend (ADR 0007), GUI battery control,
  staged GPU mutation contract (ADR 0008), supergfxd backend.
- **2026-08-14:** native ASUS Eco preflight (ADR 0009), compositor release
  gating.

**Архитектурно важные решения, видимые в истории:**
- mock-first → real backends (эволюция, а не переписывание);
- read-only MVP → controlled mutations (поэтапно, evidence-driven);
- split GPU capabilities (ADR 0005) — рефакторинг монолитного `GpuProvider`;
- direct Hardware1 caller path вместо Session1 delegation (ADR 0006 amendment)
  — осознанное решение против confused-deputy;
- hardwared введён только после доказанной привилегированной операции.

**Abandoned/изменённые подходы:** `Session1.SetPerformance` временный route
удалён (ADR 0006 follow-up); injected UPower charge bounds удалены
(`abaec34`, `bf340a0`); `orbis-supervisor` агент удалён (`c6589d3`).

---

## 15. Что полностью готово

1. **Domain model** (`orbis-core`) — типизированные newtypes, инварианты,
   capability/action/automation/telemetry модели.
2. **Provider trait contracts** (`orbis-providers`) — полный набор traits +
   error model.
3. **Application layer** (`orbis-application`) — типизированные команды +
   обязательный authoritative read-back + различение Command/ReadBack ошибок.
4. **Session protocol/client** — getter-only wire contract, untrusted-input
   validation, no-cache reads.
5. **sessiond** — read-only UPower/asusd/kernel/supergfxd/Armoury path,
   lifecycle, Nix user service (LIVE-VALIDATED).
6. **Performance read+write** — kernel `platform_profile` через hardwared
   (LIVE-VALIDATED).
7. **Battery read+write** — asusd через hardwared (LIVE-VALIDATED).
8. **GPU read-only** — Power/MUX/Access через session path (LIVE-VALIDATED).
9. **hardwared** — узкий, sandboxed, polkit-авторизованный helper.
10. **NixOS packaging/module** — package, module, polkit, D-Bus policy, VM checks.
11. **Test infrastructure** — 420 tests, P2P D-Bus, privacy, fixtures, VM.

---

## 16. Что частично готово

1. **Config** — TOML/XDG/atomic store есть, но нет production persistence
   workflow (не читается/пишется в production).
2. **Capabilities** — model/fixtures есть, runtime general probe отсутствует.
3. **UI** — real session sections работают; product GPU mode mock-only;
   telemetry mock-only; нет tray/hotkeys/overlay.
4. **GPU mutation** — staged supergfxd contract (ADR 0008) принят, READ/
   CONTRACT layer + P2P fake tests реализованы, но live mutation не разрешена.
5. **Native ASUS Eco** — read-only preflight готов, mutation executor нет.

---

## 17. Что является stub/dead/experimental

- **`orbis-cli`** — полный stub (`main.rs` = `fn main() {}`).
- **`orbis-ui/src/controller.rs`** — legacy UI-state helper, `#[allow(dead_code)]`
  в main.rs, только для unit tests.
- **`MockProvider`** — на дату этого historical audit production GPU product-mode
  backend (legacy), но production controls disabled; Task 3 устранил эту
  production dependency. Mock остаётся для tests/offscreen.
- **Module options `mockDevice`/`readOnlyEmpty`** — dead (production sessiond не
  разбирает CLI args).
- **`data/`** (dbus-1, icons, polkit-1, systemd) — пустые каталоги с `.gitkeep`.
- **`packaging/{appimage,arch,debian,fedora}`** — пустые каталоги.
- **`tests/{dbus,hardware,screenshots}`** — пустые каталоги.
- **`ExperimentalConfig.raw_wmi`** — флаг, но raw WMI не реализован (и запрещён
  в стабильной сборке).
- **`orbis-capabilities`** — в основном report assembly + fixture loading;
  runtime probe не реализован.

---

## 18. Технический долг и архитектурные проблемы

1. **Документационный drift:** `AGENTS.md` и ADR 0002 утверждают, что
   `orbis-hardwared` вне workspace; фактически он в workspace (после ADR 0006).
   AGENTS.md не обновлён.
2. **`run_worker` принимает всё больше сервисов** (main, battery, gpu_power,
   gpu_mux, gpu_access, performance) — сам код помечает это как technical debt
   (`#[allow(clippy::too_many_arguments)]`, комментарий в worker.rs:145-147).
3. **Legacy `GpuProvider` + `MockProvider`** сосуществуют с новыми split
   capability providers — двойная модель, требует миграции.
4. **`orbis-capabilities` runtime probe отсутствует** — capability-driven
   принцип декларирован, но runtime discovery не реализован (только fixtures).
5. **Config не подключён к production** — persistence/automation semantics
   (Milestone 6) не реализованы.
6. **Telemetry mock-only** — UI показывает нереальные значения.
7. **CLI stub** — `orbisctl` ничего не делает.
8. **Dead module options** в Nix module.
9. **Нет suspend/resume** обработки.
10. **`HardwareSnapshot::empty`** содержит mock bounds 40/100/1 — исторический
    mock, не hardware fact (документировано, но легко спутать).

---

## 19. Сильные стороны текущей реализации

1. **Честная capability/state модель** — `Unknown/Unsupported/Unavailable/
   ReadOnly` строго разделены; никакого mock fallback в production.
2. **Authoritative read-back** после каждой мутации; optimistic state запрещён.
3. **Узкий привилегированный helper** вместо универсального root-демона;
   polkit по `system-bus-name` original caller (защита от confused-deputy).
4. **Kernel-first подход** — стандартные ABI в приоритете; ASUS-specific через
   существующие сервисы (asusd/supergfxd).
5. **Capability-driven GPU** — физический MUX, access policy, power state,
   product mode разделены (ADR 0003/0005).
6. **Untrusted-input validation** на wire boundary (session protocol).
7. **No-cache authoritative reads** (`CacheProperties::No`), проверено тестами.
8. **Строгая тестовая дисциплина** — 420 тестов, P2P D-Bus, privacy, VM checks.
9. **Документированные ADR** — каждое архитектурное решение обосновано.
10. **Evidence-driven развитие** — hardware факты из dated fixtures, не из
    предположений.

---

## 20. Сравнение с современным kernel-first/capability-driven подходом

| Целевой принцип | Статус в Orbis |
|---|---|
| Rust | ✅ |
| Slint | ✅ |
| Отдельный непривилегированный GUI | ✅ |
| Системный Rust daemon для hardware access | ✅ (sessiond + hardwared) |
| Typed D-Bus IPC | ✅ (Session1, Hardware1) |
| Kernel-first (platform_profile, hwmon, power_supply, DRM, LED) | ✅ (platform_profile, power_supply, firmware-attributes) |
| ASUS-specific kernel ABI (asus-wmi) | ✅ (asus-armoury firmware-attributes) |
| HID backend | ⬜ (не реализован) |
| Raw EC только как крайний fallback | ✅ (не используется вообще) |
| Capability-driven hardware abstraction | ✅ (принцип) / ⬜ (runtime probe не реализован) |
| Никаких model checks в GUI/business logic | ✅ |
| Telemetry централизованно через daemon | ⬜ (mock-only) |
| Профили отдельно от raw hardware state | ✅ |
| Корректное восстановление после suspend/resume | ⬜ (не реализовано) |
| NixOS-native packaging/service integration | ✅ |

**Вывод:** Orbis уже следует современному подходу по большинству
архитектурных принципов. Отличия — в объёме реализованных функций
(telemetry, fans, lighting, display, suspend/resume, runtime probe), а не в
архитектуре. Проект не нуждается в переписывании; нужна реализация
недостающих функций в рамках существующей архитектуры.

---

## 21. Что стоит сохранить без изменений

1. **Domain model** (`orbis-core`) — newtypes, инварианты, capability/action
   модели.
2. **Provider trait contracts** и error model.
3. **Application layer** с authoritative read-back.
4. **Session protocol/client** — getter-only, untrusted-input validation.
5. **hardwared** — узкий, sandboxed, polkit-авторизованный helper.
6. **NixOS packaging/module/polkit/D-Bus policy**.
7. **Test infrastructure** — P2P D-Bus, privacy, fixtures, VM checks.
8. **ADR-документированные решения** (0001–0009).
9. **Capability-driven GPU split** (ADR 0003/0005).

---

## 22. Что стоит переработать

1. **`run_worker` composition** — вынести в отдельный composition layer
   (сейчас 6 сервисов в одном generic; сам код помечает как debt).
2. **Legacy `GpuProvider` + `MockProvider`** — постепенная миграция на split
   capability providers; удаление legacy после полного перехода.
3. **`orbis-capabilities`** — реализовать runtime probe (capability-driven
   discovery), а не только fixture loading.
4. **Config** — подключить к production (persistence workflow, Milestone 6).
5. **`orbis-cli`** — реализовать read-only status/diagnostics (Milestone 7).
6. **Dead module options** (`mockDevice`/`readOnlyEmpty`) — реализовать или
   удалить.
7. **`HardwareSnapshot::empty`** mock bounds — убрать из production path.

---

## 23. Что отсутствует

1. **Fans** — RPM/curves production provider + UI editor.
2. **Power limits** — production provider (после доказательства units/ranges).
3. **Lighting/RGB/Aura** — production provider.
4. **AniMe/Slash** — production provider (модель-специфично).
5. **Display controls** — refresh rate, overdrive, HDR.
6. **Telemetry** — real hwmon/powercap/thermal provider.
7. **Automation engine** — AC/battery/resume rules.
8. **Suspend/resume** — logind `PrepareForSleep` + re-apply.
9. **GPU mutation** — live Eco/Standard/Ultimate (staged supergfxd).
10. **Tray/hotkeys/overlay** — StatusNotifierItem, XDG Global Shortcuts.
11. **Firmware updates** — fwupd integration.
12. **Runtime capability probe** — general discovery.
13. **Persistence workflow** — config read/write в production.
14. **CLI** — полноценный `orbisctl`.
15. **Non-Nix packaging** — appimage/arch/debian/fedora (пустые).

---

## 24. Предлагаемая последовательность дальнейшей разработки

На основе roadmap и фактического состояния:

1. **Milestone 6 — Persistence and automation semantics** (config подключить к
   production; automation engine; suspend/resume). Это фундамент для
   долгоживущих настроек.
2. **Milestone 4 (завершение) — fan/telemetry** по доказанным источникам
   (hwmon `fan*_input`, `fan*_label`; powercap/thermal). Реальный telemetry
   provider + UI.
3. **Milestone 5 (завершение) — GPU mutation** через staged supergfxd
   (Hybrid↔Integrated), затем product mapping.
4. **Milestone 7 — Error/status UX и CLI diagnostics** (реализовать `orbisctl`).
5. **Milestone 8 — Packaging maturity** (dead options, desktop/AppStream,
   non-Nix packaging).
6. **Milestone 9 — Broader hardware support** (fans curves, lighting, display,
   power limits) по evidence-driven device profiles.
7. **Технический долг:** рефакторинг `run_worker` composition; миграция с
   legacy `GpuProvider`/`MockProvider`; реализация runtime capability probe;
   обновление AGENTS.md (hardwared в workspace).

---

## Приложение: функциональная матрица

| Функция | Статус | Backend | UI | Daemon/API | Примечание |
|---|---|---|---|---|---|
| Monitoring (battery/performance/gpu) | complete (read) | UPower/kernel/supergfxd/Armoury | ✅ real | Session1 | LIVE-VALIDATED |
| Platform profiles (read) | complete | kernel `platform_profile` | ✅ real | Session1 | LIVE-VALIDATED |
| Platform profiles (write) | complete | kernel via hardwared | ✅ | Hardware1 | LIVE-VALIDATED |
| Battery charge limit (read) | complete | UPower+asusd+kernel | ✅ real | Session1 | LIVE-VALIDATED |
| Battery charge limit (write) | complete | asusd via hardwared | ✅ | Hardware1 | LIVE-VALIDATED |
| GPU power (read) | complete | supergfxd | ✅ real | Session1 | LIVE-VALIDATED |
| GPU MUX (read) | complete | kernel Armoury | ✅ real | Session1 | LIVE-VALIDATED |
| GPU access (read) | complete | kernel Armoury | ✅ real | Session1 | LIVE-VALIDATED |
| GPU product mode (Eco/Std/Ult/Opt) | stub/mock | MockProvider (legacy) | disabled | — | NOT PROVEN mapping |
| GPU mutation | partial | supergfxd staged (ADR 0008) | disabled | Hardware1 (contract) | live NOT allowed |
| Native ASUS Eco | partial (preflight) | read-only classifier | — | — | ADR 0009 |
| Fan RPM | missing | — | mock | — | domain/traits only |
| Fan curves | missing | — | mock | — | domain/traits only |
| Power limits | missing | — | — | — | units/ranges unproven |
| Keyboard/RGB/Aura | missing | — | — | — | domain/traits only |
| AniMe/Slash | missing | — | — | — | trait/research only |
| Display refresh/overdrive | missing | — | — | — | domain/traits only |
| Telemetry | mock-only | — | mock | — | no real provider |
| Automation | missing | — | — | — | domain/config only |
| Suspend/resume | missing | — | — | — | no logind handling |
| Config persistence | partial | — | — | — | no production workflow |
| Runtime capability probe | missing | — | — | — | fixtures only |
| CLI (`orbisctl`) | stub | — | — | — | `fn main() {}` |
| Tray/hotkeys/overlay | missing | — | — | — | — |
| Firmware updates | missing | — | — | — | fwupd not integrated |
