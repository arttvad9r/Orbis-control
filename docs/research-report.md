# Research Report — Orbis Control

> Дата исследования: **2026-08-06**
> Автор: главный агент (Этап 0)
> Статус: зафиксирован, готов к ревью. Никаких аппаратных записей не выполнялось.

## 1. Цель

Определить, какие функции G-Helper (Windows) достижимы в Linux нативно, через какие
интерфейсы, и с какими ограничениями. Зафиксировать источники истины: установленное
ядро и его sysfs ABI, D-Bus introspection установленных сервисов, исходный код
соответствующих версий, документацию.

Иерархия источников истины (по убыванию доверия), как предписано заданием:

1. установленное ядро и его sysfs ABI;
2. D-Bus introspection установленного сервиса;
3. исходный код соответствующей версии;
4. документация этой версии;
5. README и сторонние статьи.

---

## 2. Репозитории, версии, лицензии

Исследование проводилось по `--depth 1` клонам в `/tmp/opencode/research/`.
Полные хэши зафиксированы на момент исследования.

| # | Репозиторий | Commit (2026-08-06) | Лицензия | Роль в проекте |
|---|---|---|---|---|
| 1 | `seerge/g-helper` | `e439349fc6d99720861128b55421d05fc0f36ec7` | GPL-3.0 | Референт UI/UX и функций (визуальная компоновка; код не переносится) |
| 2 | `OpenGamingCollective/asusctl` | `dfe548a7bef36378323466828369d6301063a775` | MPL-2.0 | Основной D-Bus backend для ASUS-функций |
| 3 | `OpenGamingCollective/cardwire` | `1de887f2fb9d5dbd98ba73433c3abf04ee4a33b6` | GPL-3.0 | Экспериментальный GPU-block (eBPF, Wayland-only) |
| 4 | `UdayaSri0/g-helper-linux` | `5c1459bb49bf828d862ab684a6cb757f6268afec` | MIT / Apache-2.0 | Архитектурный референс (Rust, UI/daemon/providers) |
| 5 | `utajum/g-helper-linux` | `4cc1bb7dcc2e88f4e4579dbff25f49e95ac49efc` | **не обнаружен LICENSE** | Avalonia-порт; не переиспользовать без уточнения лицензии |
| 6 | `Traciges/Ayuz` | `3b094780e9650dbe956f2498d675ab02664d6649` | GPL-3.0 | GTK4/Rust референс |
| 7 | `SuperGify/supergfxd` | (clone failed; API снят через установленный сервис) | GPL-3.0 | Legacy GPU-mode fallback |

### 2.1 Полная воспроизводимость исследования

| Репозиторий | Canonical URL | Ветка | Tag | git describe | Дата/время получения (UTC) | Файл лицензии | Перенос кода | Справочный материал |
|---|---|---|---|---|---|---|---|---|
| g-helper | https://github.com/seerge/g-helper | main | — | `e439349` | 2026-08-06 ~15:15 | `LICENSE` (GPL-3.0) | Нет (только компоновка как референс) | Да |
| asusctl | https://github.com/OpenGamingCollective/asusctl | main | — | `dfe548a` | 2026-08-06 ~15:15 | `LICENSE` (MPL-2.0) | Нет (только D-Bus взаимодействие; при переносе — MPL требования) | Да |
| cardwire | https://github.com/OpenGamingCollective/cardwire | main | — | `1de887f` | 2026-08-06 ~15:15 | `LICENSE` (GPL-3.0) | Нет (взаимодействие по D-Bus) | Да |
| g-helper-linux (UdayaSri0) | https://github.com/UdayaSri0/g-helper-linux | main | — | `5c1459b` | 2026-08-06 ~15:15 | `LICENSE-APACHE`, `LICENSE-MIT` | Нет | Да |
| g-helper-linux (utajum) | https://github.com/utajum/g-helper-linux | master | — | `4cc1bb7` | 2026-08-06 ~15:15 | отсутствует | **Нет (лицензия не подтверждена)** | Ограниченно |
| Ayuz | https://github.com/Traciges/Ayuz | main | v1.1.9 | `v1.1.9` | 2026-08-06 ~15:15 | `LICENSE` (GPL-3.0) | Нет | Да |
| supergfxd | https://github.com/SuperGify/supergfxd | — | — | **не подтверждён** (clone не удался) | — | не проверена | **Нет, до проверки лицензии и исходной версии** | Ограниченно (только D-Bus API установленного пакета) |

Использованные файлы/интерфейсы: для g-helper — `app/Settings.Designer.cs`,
`app/Fans.Designer.cs`, `app/Extra.Designer.cs`, `app/UI/RForm.cs`, `app/Mode/Modes.cs`,
`app/Settings.cs`; для asusctl — `rog-dbus/src/*.rs`, `rog-platform/src/platform.rs`,
`rog-control-center/src/shortcuts.rs`, `asusd-user/src/`; для cardwire —
`crates/cardwire-daemon/src/interface/*.rs`, `models.rs`.

### 2.2 supergfxd: зафиксировано по установленному пакету

- **Версия установленного пакета**: `supergfxctl-5.2.7` (Nix store path:
  `/nix/store/n4mwd9yk3fqh6rb4p7yh8rjq8v2zmgpm-supergfxctl-5.2.7/bin/supergfxd`).
- **Источник пакета Nix**: nixpkgs (пакет `supergfxctl`), подключён в системной
  конфигурации `/home/artt/.nixos/nixos/modules/hardware/asus.nix`
  (`services.supergfxd.enable = true;`).
- **D-Bus**: service `org.supergfxctl.Daemon` (system bus), объект
  `/org/supergfxctl/Gfx`, интерфейс `org.supergfxctl.Daemon`:
  - методы: `Config() -> (ubbbbtu)`, `Mode() -> u`, `PendingMode() -> u`,
    `PendingUserAction() -> u`, `Power() -> u`, `SetConfig(ubbbbtu)`,
    `SetMode(u) -> u`, `Supported() -> au`, `Vendor() -> s`, `Version() -> s`;
  - сигналы: `NotifyAction(u)`, `NotifyGfx(u)`, `NotifyGfxStatus(u)`.
- **busctl introspect**: см. фикстуру
  `tests/fixtures/hardware/fa707nv/supergfxd-introspection-00-org_supergfxctl_Gfx.xml`
  (снято `busctl --system introspect --xml-interface ...`).
- **Исходный commit не подтверждён** — clone `git clone https://github.com/SuperGify/supergfxd`
  не удался в момент исследования (сетевой сбой). Реализация supergfxd НЕ копируется
  до проверки лицензии и исходной версии; в проекте используется только D-Bus API.

Примечания:

- **asusctl** использует MPL-2.0: при переносе его кода (не планируется в стабильной
  версии; только D-Bus взаимодействие) потребуется соблюдать MPL для изменённых файлов.
- У проекта **utajum/g-helper-linux отсутствует LICENSE** в корне — код из него не
  переиспользуется до выяснения лицензии.
- Логотипы ASUS/ROG/TUF и значок G-Helper не копируются; компоновка G-Helper
  используется только как визуальный референс (см. `docs/ui-reference.md`).

---

## 3. Установленная система (источник истины №1)

> **NixOS — официальная целевая платформа** (наравне с Fedora, Arch, Ubuntu,
> Debian, openSUSE; см. `docs/architecture.md` §Совместимость). Разработка и
> аппаратная проверка выполняются на NixOS; окружение воспроизводимо через
> `flake.nix` (dev shell, `nix build`, `nix run`, `nix flake check`).

Все данные ниже сняты с реального ноутбука, на котором ведётся разработка:

| Параметр | Значение |
|---|---|
| Вендор / модель | ASUSTeK COMPUTER INC. / ASUS TUF Gaming A17 FA707NV_FA707NV |
| Board name | FA707NV |
| BIOS | FA707NV.316 (2024-11-04) |
| Дистрибутив | NixOS 26.11 (Zokor) |
| Ядро | 7.1.6 (x86_64) |
| CPU | AMD Ryzen 5 7535HS |
| dGPU | NVIDIA RTX 4060 Max-Q / Mobile (AD107M, rev a1) |
| iGPU | AMD Radeon 680M (Rembrandt) |
| DE / сессия | KDE Plasma 6, Wayland |
| asusd | 6.3.8 (установлен, активен) |
| supergfxd | активен |
| cardwire | **не установлен** |
| power-profiles-daemon | **не установлен** (владелец platform_profile — asusd/asus-wmi) |

Загруженные модули ядра, релевантные задаче: `asus_nb_wmi`, `asus_armoury`,
`firmware_attributes_class`, `asus_wmi`, `sparse_keymap`, `nvidia_wmi_ec_backlight`,
`video`, `battery`.

> Значение: на целевой машине уже работают asusd + asus_armoury на свежем ядре.
> Это даёт полный набор ASUS-интерфейсов и позволяет валидировать capability-probe
> на реальном железе без записей.

---

## 4. D-Bus introspection установленных сервисов (источник истины №2)

### 4.1 asusd — `xyz.ljones.Asusd` (system bus)

Дерево объектов (реальное):

```
/xyz/ljones
├── /xyz/ljones/asus_armoury
│   ├── /xyz/ljones/asus_armoury/charge_mode
│   ├── /xyz/ljones/asus_armoury/dgpu_disable
│   ├── /xyz/ljones/asus_armoury/gpu_mux_mode
│   ├── /xyz/ljones/asus_armoury/nv_dynamic_boost
│   ├── /xyz/ljones/asus_armoury/nv_temp_target
│   ├── /xyz/ljones/asus_armoury/panel_overdrive
│   ├── /xyz/ljones/asus_armoury/ppt_pl1_spl
│   ├── /xyz/ljones/asus_armoury/ppt_pl2_sppt
│   └── /xyz/ljones/asus_armoury/ppt_pl3_fppt
└── /xyz/ljones/aura
    └── /xyz/ljones/aura/tuf
```

Интерфейс `xyz.ljones.Platform` (на `/xyz/ljones`) — фактические значения 2026-08-06:

| Член | Тип | Значение | Комментарий |
|---|---|---|---|
| `PlatformProfile` | u | 0 | Balanced |
| `PlatformProfileChoices` | au | [3, 2, 0, 1] | LowPower, Quiet, Balanced, Performance |
| `PlatformProfileOnAc` | u | 0 | Balanced |
| `PlatformProfileOnBattery` | u | 2 | Quiet |
| `ChangePlatformProfileOnAc` | b | true | |
| `ChangePlatformProfileOnBattery` | b | true | |
| `ChargeControlEndThreshold` | y | 80 | |
| `ProfileBalancedEpp` | u | 3 | |
| `ProfilePerformanceEpp` | u | 1 | |
| `ProfileQuietEpp` | u | 4 | |
| `PlatformProfileLinkedEpp` | b | true | |
| `EnablePptGroup` | b | false | |
| `Version` | s | "6.3.8" | (в исходниках 6.3.11 — метод/свойство версии различаются между версиями!) |

Методы `xyz.ljones.Platform`: `NextPlatformProfile`, `OneShotFullCharge`,
`SupportedProperties`.

Интерфейс `xyz.ljones.FanCurves`: `FanCurveData(u) -> a(s(yyyyyyyy)(yyyyyyyy)b)`,
`SetCurvesToDefaults(u)`, `SetFanCurve(u, s(yyyyyyyy)(yyyyyyyy)b)`,
`SetFanCurvesEnabled(u, b)`, `SetProfileFanCurveEnabled(u, s, b)`.

Интерфейс `xyz.ljones.AsusArmoury` (для каждого armoury-объекта):
свойства `AvailableAttrs` (as), `CurrentValue` (i, writable), `DefaultValue` (i),
`MaxValue` (i), `MinValue` (i), `Name` (s), `PossibleValues` (ai), `QueuedGpuValue` (i),
`ScalarIncrement` (i); методы `RestoreDefault`, `ApplyQueuedGpuValue() -> b`.

Фактические значения armoury-объектов на FA707NV:

| Объект | Name | PossibleValues | CurrentValue | Default | ScalarInc | Примечание |
|---|---|---|---|---|---|---|
| charge_mode | ChargeMode | [0,1,2] | 1 | -1 | -1 | |
| dgpu_disable | DgpuDisable | [0,1] | 0 | -1 | -1 | |
| gpu_mux_mode | GpuMuxMode | [0,1] | 0 | -1 | -1 | reboot-latched |
| nv_dynamic_boost | NvDynamicBoost | — | **ошибка чтения** | -1 | 1 | AvailableAttrs=[scalar_increment] |
| nv_temp_target | NvTempTarget | — | **ошибка чтения** | -1 | 1 | AvailableAttrs=[scalar_increment] |
| panel_overdrive | PanelOverdrive | [0,1] | 1 | -1 | -1 | |
| ppt_pl1_spl | PptPl1Spl | — | **ошибка чтения** | -1 | 1 | AvailableAttrs=[scalar_increment] |
| ppt_pl2_sppt | PptPl2Sppt | — | **ошибка чтения** | -1 | 1 | |
| ppt_pl3_fppt | PptPl3Fppt | — | **ошибка чтения** | -1 | 1 | |

**Критический вывод (эмпирика):** наличие объекта/файла ≠ поддержка. У `ppt_*`,
`nv_dynamic_boost`, `nv_temp_target` на этой модели `CurrentValue` читается с ошибкой,
`AvailableAttrs` содержит только `scalar_increment`. Провайдер обязан при пробе
пытаться прочитать и помечать `Unsupported`/`TemporarilyUnavailable`, а не
предполагать поддержку по имени объекта.

### 4.1.1 Точные ошибки и семантика для PPT / Dynamic Boost / Temperature Target (FA707NV)

Зафиксировано 2026-08-06 двумя путями:

| Путь | Результат | Точная ошибка | Классификация |
|---|---|---|---|
| asusd D-Bus `CurrentValue` (`xyz.ljones.AsusArmoury`) | ошибка | `Failed to get property CurrentValue ... Could not read current value`; журнал asusd: `[ERROR asusd::asus_armoury] Failed to read: Io(Os { code: 19, ... message: "No such device" })` | **Unsupported (asusd backend)**: errno 19 = ENODEV, устойчиво; `SupportedProperties` платформы не включает PPT-группу, `EnablePptGroup=false` |
| Прямой sysfs (`/sys/devices/platform/asus-nb-wmi/ppt_pl1_spl` и т.д.) | чтение успешно | нет ошибки | read: **Supported (kernel)**; write: **PermissionDenied** (файлы `-rw-r--r-- root:root`, запись требует root) |
| Значения прямого чтения | `ppt_pl1_spl=5`, `ppt_pl2_sppt=5`, `ppt_fppt=5`, `nv_dynamic_boost=5`, `nv_temp_target=75` | — | **семантика значений требует верификации** по документации драйвера `asus_armoury` перед отображением в UI; для `nv_temp_target` 75 читается правдоподобно (°C), для `ppt_*`=5 единицы не подтверждены |
| `cpufv` (CPU boost) | чтение запрещено | `Отказано в доступе` (EACCES) | **PermissionDenied**: файл `-w------- root:root` (0200), root-only |

Итог для capability-модели FA707NV (не фиксировать окончательный статус только по
наличию файлов!):

- **PPT / Dynamic Boost / Temp Target**: via asusd — `Unsupported` (ENODEV);
  via kernel — read `Supported`, write `PermissionDenied` (нужен hardwared/root).
  Эффективный статус для UI без hardwared: **ReadOnly** с пояснением.
  Семантика значений — открытый вопрос до Этапа 3.
- **cpufv**: `PermissionDenied` (root-only).
- Детальные права/ошибки сохранены в фикстуре
  `tests/fixtures/hardware/fa707nv/sysfs-tree.json` и
  `tests/fixtures/hardware/fa707nv/expected-capabilities.json`.

Прочие интерфейсы (из исходников asusctl 6.3.11, `rog-dbus/src/`):

- `xyz.ljones.Backlight`: `primary_brightness`, `screenpad_*`.
- `xyz.ljones.Aura`: `all_mode_data`, `brightness`, `led_mode`, `led_mode_data`,
  `led_power`, `supported_basic_modes`, `supported_basic_zones`, `supported_power_zones`,
  `direct_addressing_raw` (последний — только экспериментально).
- `xyz.ljones.Anime`: `device_state`, `run_main_loop`, `write(AnimeDataBuffer)`,
  `brightness`, `builtin_animations`, `builtins_enabled`, `enable_display`,
  `off_when_lid_closed`, `off_when_suspended`, `off_when_unplugged`.
- `xyz.ljones.Slash`: режимы/яркость/скорость/интервал.
- `xyz.ljones.XgmLed`, `xyz.ljones.ScsiAura` — периферия XG Mobile / SCSI-подсветка.

### 4.2 supergfxd — `org.supergfxctl.Daemon` (system bus)

Объект `/org/supergfxctl/Gfx`, интерфейс `org.supergfxctl.Daemon`:

- Методы: `Config() -> (u b b b b t u)`, `Mode() -> u`, `PendingMode() -> u`,
  `PendingUserAction() -> u`, `Power() -> u`, `SetConfig((u b b b b t u))`,
  `SetMode(u) -> u`, `Supported() -> au`, `Vendor() -> s`, `Version() -> s`.
- Сигналы: `NotifyAction(u)`, `NotifyGfx(u)`, `NotifyGfxStatus(u)`.

Роль в Orbis Control: legacy fallback для GPU-режимов на системах без asusd/
asus-armoury. Не должен быть первым выбором там, где доступен asusd.

### 4.3 Cardwire — `org.opengamingcollective.cardwire.*` (system bus)

Не установлен; интерфейсы зафиксированы по исходникам `cardwire-daemon`:

- `org.opengamingcollective.cardwire.Mode` (на `/org/opengamingcollective/cardwire`):
  `set_mode(u32)`, `mode() -> u32`, `requested_mode() -> u32`.
- `org.opengamingcollective.cardwire.Gpu`: `set_block(bool)`, `block() -> bool`,
  `lsof() -> a{sv}`, `get_device() -> GpuDevice`, `power_state() -> s`,
  сигнал `power_state_changed(s)`.
- `net.hadess.SwitcherooControl` (реализован самим cardwire): `has_dual_gpu()`,
  `num_gpus()`, `gpus() -> a{sv}`.
- Механизм: eBPF LSM-хуки возвращают `-ENOENT` для `/dev/dri/*`, `nvidia*`,
  sysfs `config` и т.п. **Только Wayland**, X11 не поддерживается.
- Отказоустойчивость: `apply_mode_at_startup` при ошибке переходит в manual mode;
  есть `monitor_display_future` и `battery_switch_future` (авто-режимы).

### 4.4 UPower — `org.freedesktop.UPower` (system bus)

Установлен (upowerd 1.7-1.9). Устройство `battery_BAT1` (A32-K55, 90.0 Wh design):

| Параметр | Значение |
|---|---|
| state | fully-charged |
| percentage | 95 % |
| energy-full | 80.14 Wh |
| energy-full-design | 90.01 Wh |
| capacity | 89.0 % |
| charge-cycles | N/A (ASUS не отдаёт через SMBus на этой модели) |
| energy-rate | 0 W |
| voltage | 16.71 V |

Источник истины для состояния AC/батареи: `org.freedesktop.UPower` +
`org.freedesktop.UPower.Device` (свойства `State`, `Percentage`, `EnergyRate`,
`TimeToEmpty/Full`, `IsPresent`, `PowerSupply`; сигнал `PropertiesChanged`).
Тип источника (AC / battery / USB-C low-power): `org.freedesktop.UPower` свойства
устройства + `Type`, плюс возможность отличать слабое USB-C зарядное по
`energy-rate`/`voltage` (см. `docs/feature-matrix.md`, раздел про USB-C).

### 4.5 power-profiles-daemon

На целевой машине отсутствует. API (для совместимости с системами без asusd):
интерфейс `org.freedesktop.UPower.PowerProfiles` (он же `net.hadess.PowerProfiles`),
методы `GetProfiles/GetActiveProfile/SetActiveProfile/HoldProfile/ReleaseProfile`,
свойства `ActiveProfile`, `Profiles`, `Actions`, `PerformanceInhibited`,
`PerformanceDegraded`, `ActiveProfileHolds`; сигнал `ProfileChanged`.
PPD поддерживает `power-saver / balanced / performance` и метаданные
`Driver: "power-profiles-daemon"`.

Политика: Orbis Control **не отключает** ppd автоматически; при обнаружении
конфликта владения platform_profile показывается предупреждение и выбирается
единственный владелец политики (см. ADR 0002 и `docs/architecture.md`).

---

## 5. Kernel ABI (источник истины №1, sysfs на ядре 7.1.6)

### 5.1 platform_profile

```
/sys/firmware/acpi/platform_profile         -> balanced
/sys/firmware/acpi/platform_profile_choices -> quiet balanced performance
```

Ядро 7.1.6 отдаёт три профиля (quiet/balanced/performance). Сопоставление:
Silent→quiet, Balanced→balanced, Turbo→performance. Владелец на этой машине —
asus-wmi (`throttle_thermal_policy` на платформе asus-nb-wmi). Интерфейс
стандартизирован (Documentation/ABI/testing/sysfs-platform_profile.txt).

### 5.2 charge_control_end_threshold

```
/sys/class/power_supply/BAT1/charge_control_end_threshold = 80
```

Доступен и совпадает со значением, отдаваемым asusd (`ChargeControlEndThreshold=80`).
Стандартный fallback, когда asusd недоступен.

### 5.3 asus-wmi (платформа asus-nb-wmi)

Реальные sysfs-атрибуты на FA707NV (ядро 7.1.6):

```
charge_mode  cpufv  dgpu_disable  gpu_mux_mode  nv_dynamic_boost
nv_temp_target  panel_od  platform-profile  ppt_fppt  ppt_pl1_spl
ppt_pl2_sppt  throttle_thermal_policy
```

Атрибуты `ppt_*`, `nv_*`, `panel_od`, `gpu_mux_mode`, `dgpu_disable`, `charge_mode`
относятся к драйверу `asus_armoury` (класс `firmware_attributes_class`).

### 5.4 asus_armoury

Модуль `asus_armoury` загружен (ядро 7.1.6). Предоставляет ограниченный allowlist
атрибутов с min/max/step/possible_values и поддержкой queued values (для MUX и
атрибутов, требующих перезагрузки). Это предпочтительный интерфейс для:
MUX (gpu_mux_mode), dgpu_disable, charge_mode, panel_overdrive, PPT PL1/PL2/PL3,
NVIDIA Dynamic Boost, NVIDIA temp target.

Минимальное ядро для asus_armoury — оценка по документации и истории драйвера:
потребуется явная проверка на целевой системе (в доке `hardware-support.md`
зафиксировать: UI-запуск / основные ASUS-функции / asus-armoury-функции).

### 5.5 hwmon

Реальные устройства:

| hwmon | name | Содержимое |
|---|---|---|
| hwmon5 | k10temp | CPU-температура |
| hwmon13 | amdgpu | iGPU |
| hwmon9 | asus | `fan1_input=2700 (cpu_fan)`, `fan2_input=2800 (gpu_fan)`, `pwm1_enable=2`, `pwm2_enable=0` |
| hwmon10 | asus_custom_fan_curve | `pwm{1,2}_auto_point{1..8}_pwm/_temp` |
| hwmon0/1 | nvme | SSD-температуры |
| hwmon2 | acpitz_0 | ACPI thermal |
| hwmon12 | iwlwifi_1_1 | WiFi |
| hwmon3/4 | ACAD/BAT1 | power supply |

Кривые вентиляторов: **8 точек × 2 вентилятора**, temp 45–85 °C, pwm 0–102
(шкала 0–255 hwmon, но asus использует 0–102).

### 5.6 backlight / DRM

- `/sys/class/backlight/nvidia_0` — nvidia_wmi_ec_backlight (единственный backlight).
- DRM: amdgpu (iGPU) + nvidia (dGPU). Внутренний дисплей — на amdgpu; поиск
  внутреннего дисплея выполняется через DRM connector properties и фактическое
  состояние (не хардкодить `eDP-1`).
- Внешние утилиты для смены частоты: `wlr-randr` (wlroots), KScreen D-Bus (Plasma),
  Mutter интерфейсы (GNOME), RandR (X11). Прямой DRM atomic mode setting из GUI — нет.

### 5.7 keyboard LED

`/sys/class/leds/asus::kbd_backlight` — системный LED-интерфейс (яркость).
Полноценная Aura-RGB — через asusd `xyz.ljones.Aura`.

---

## 6. G-Helper: анализ (источник: исходники e439349f)

### 6.1 Технологии

- C# / .NET, WinForms, `AutoScaleDimensions = 192×192` (DPI-aware, все размеры в
  «192-DPI логических пикселях» → нормализация ÷2 для 96-DPI).
- Собственные R-контролы: `RForm`, `RButton`, `RComboBox`, `RTrackBar`,
  `RChart`, `RBadgeButton`, `RCheckBox`, `RNumericUpDown`, `Slider`, `RColorButton`.
- `AsusACPI.cs` — низкоуровневый доступ к ACPI/WMI (Windows-специфично).
- `HardwareControl.cs`, `ModeControl` — высокоуровневая логика.

### 6.2 Цвета (тёмная тема, из RForm.cs)

| Токен | Значение |
|---|---|
| formBack | `#1C1C1C` |
| buttonMain | `#2E2E2E` |
| buttonSecond | `#242424` |
| foreMain | `#F0F0F0` |
| chartMain | `#232323` |
| hover shift | +4 % к целевому, pressed +8 % (RButton) |

Шрифт: Segoe UI 9 pt; заголовки секций 9 pt Bold. Границы секций: panels с
`Padding(20,…)`, ширина контента 787 px при ширине панели 827 px.

### 6.3 Режимы производительности

`Modes.cs`: `{2: Silent, 0: Balanced, 1: Turbo}`, до 20 пользовательских режимов.
Отображение текущего профиля и переключение — три кнопки + кнопка Fans.

### 6.4 Инвентарь функций (структура app/)

- **AsusACPI** — ACPI/WMI (вентиляторы, профили, GPU, backlight, charge limit, ...).
- **Battery/** — заряд, лимит, статистика.
- **Display/** — `ScreenControl` (частота, brightness), `VisualControl` (visual mode,
  gamut, цветовые профили), `AmdDisplay`, `ScreenCCD`, `ScreenBrightness`.
- **Fan/** — `FanSensorControl` (кривые, RPM, режимы).
- **Gpu/** — `GPUModeControl`, `IGpuControl`, NVidia/AMD реализации (Eco/Standard/
  Ultimate/Optimized, MUX).
- **Keyboard** — режимы подсветки, яркость, цвета.
- **Matrix.cs / Slash.cs** — AniMe Matrix и Slash Lighting.
- **Overlay/** — игровой оверлей (FPS, температуры).
- **Updates / AutoUpdate** — обновления приложения.
- **Input/** — `KeyboardHook`, `MKeyControl` (M-клавиши), `NumberPad`.
- **Peripherals/** — `Mouse` (настройки мышей ASUS).
- **USB/** — XG Mobile и USB-устройства.
- **Ally/** — ROG Ally (TDP, контроллеры).
- **Extra.cs** — расширенные настройки (hotkeys, backlight timeout, CPU cores,
  ACPI DEVS test, сервисы).

### 6.5 Что переиспользуем

Только **визуальную компоновку и поведение** (компактное вертикальное окно без
боковой панели, три кнопки режимов, четыре кнопки GPU, отдельные окна Fans+Power
и Extra). Код GPL-3.0 из G-Helper в производственный код не переносится; при
любом заимствовании — раздел `THIRD_PARTY_NOTICES.md` и таблица происхождения.

---

## 7. Анализ Linux-проектов-аналогов

### 7.1 asusctl (6.3.11) — основной backend

- Workspace: `asusd` (root daemon, system bus), `asusd-user` (user daemon,
  anime-контроль), `rog-control-center` (GTK GUI с user D-Bus `xyz.ljones.rogcc`),
  `asusctl` (CLI), `rog-*` crates.
- `rog-control-center` уже использует **XDG Global Shortcuts portal** для горячих
  клавиш (в исходниках отмечено: «KDE may persist denied shortcuts with an empty
  trigger» — важный кейс для нашего hotkey-провайдера).
- Fan curves через D-Bus `FanCurveData(u) -> a(s(yyyyyyyy)(yyyyyyyy)b)`.

### 7.2 cardwire (1de887f) — экспериментальный GPU-block

См. §4.3. Вывод: блокировка доступа приложений ≠ физический MUX; Wayland-only;
необходим чёткий экспериментальный статус в UI.

### 7.3 g-helper-linux (UdayaSri0, 5c1459b) — архитектурный референс

Rust-проект с разделением UI / демона / провайдеров. Используется как референс
для структуры workspace и trait-дизайна (не код).

### 7.4 Ayuz (3b09478) — GTK4/Rust

Свежий GTK4-проект; референс для провайдеров и пакетирования. Не используется как
основа (выбран Slint).

### 7.5 Slint

Официальная документация (slint.dev): declarative UI, Rust API, backends для
Wayland (winit/femtovg+skia) и X11, поддержка fractional scaling, `@tooling` для
preview, `slint-viewer` для screenshot-тестов (эталонный рендер с `--auto-scale`).
Проверка версии и фич — на этапе 2 через context7/официальные доки.

---

## 8. Таблица достижимости функций

| Функция G-Helper | Реализация в Linux | Статус |
|---|---|---|
| Режимы Silent/Balanced/Turbo | asusd `Platform` → `platform_profile` | полноценная |
| AC/battery-профили | asusd `PlatformProfileOnAc/OnBattery` | полноценная |
| GPU Eco/Standard/Ultimate/Optimized | asusd/asus-armoury + Cardwire + supergfxd | backend-dependent |
| MUX Ultimate | asusd `AsusArmoury/gpu_mux_mode` (reboot) | полноценная (с требованием) |
| Battery charge limit | asusd / `charge_control_end_threshold` | полноценная |
| Статистика батареи (health, циклы) | UPower | частичная (циклы не у всех) |
| Экран: частота, overdrive, HDR | KScreen / Mutter / wlr-randr / RandR + asusd `panel_overdrive` | backend-dependent |
| Flicker-free dimming / visual mode | asusd/asus-armoury (по моделям) + AMD/экранные контролы | backend-dependent |
| Подсветка клавиатуры (Aura) | asusd `Aura` | полноценная |
| AniMe Matrix | asusd `Anime` (модели с матрицей) | полноценная (модель-специфичная) |
| Slash Lighting | asusd `Slash` | полноценная (модель-специфичная) |
| Fans + Power (кривые, RPM) | asusd `FanCurves` + hwmon | полноценная |
| Power limits (SPL/SPPT/FPPT/PL1/PL2...) | asusd `AsusArmoury/ppt_*` | backend-dependent |
| CPU boost / EPP | asusd EPP-свойства + sysfs | полноценная |
| Undervolting | нет стабильного mainline-интерфейса | экспериментальная |
| Горячие клавиши | XDG Global Shortcuts portal | частичная |
| Трей | StatusNotifierItem (ksni) | полноценная |
| Автоматизация AC/батарея/resume | sessiond + UPower/logind сигналы | полноценная |
| Overlay (FPS) | отдельное окно + MangoHud | частичная (MangoHud) |
| Обновления приложения | пакетный менеджер / fwupd | полноценная |
| Обновления BIOS | fwupd / ссылки на сайт ASUS | backend-dependent |
| M-клавиши / NumberPad / мыши ASUS | нет стабильного Linux-интерфейса | пока недостижимая |
| ROG Ally / XG Mobile | asusd XgmLed/ScsiAura + отдельные проекты | частичная |
| Armoury Crate «режимы игр» | аналога нет | недостижимая |

---

## 9. Выбранные API и риски

### 9.1 Выбранные API

1. **asusd (D-Bus, system bus)** — основной ASUS-провайдер:
   `xyz.ljones.Platform`, `xyz.ljones.FanCurves`, `xyz.ljones.Aura`,
   `xyz.ljones.Anime`, `xyz.ljones.Slash`, `xyz.ljones.AsusArmoury`.
2. **Kernel ABI** — fallback и чтение:
   `platform_profile`, `charge_control_end_threshold`, hwmon, backlight, leds.
3. **UPower (D-Bus)** — батарея/AC/заряд.
4. **supergfxd (D-Bus)** — legacy GPU-mode fallback.
5. **Cardwire (D-Bus)** — экспериментальный «блок доступа к dGPU» (Wayland).
6. **Дисплей**: KScreen D-Bus (Plasma) → Mutter → wlr-randr → RandR (X11).
7. **Горячие клавиши**: XDG Global Shortcuts portal (ashpd).
8. **Обновления**: fwupd (D-Bus `org.freedesktop.fwupd`) + пакетный менеджер.
9. **Трей**: ksni (StatusNotifierItem).

### 9.2 Риски

| Риск | Оценка | Митигация |
|---|---|---|
| asus-armoury требует свежих ядер | высокий для дистрибутивных ядер LTS | документация минимальных ядер; fallback на asusd-версии API |
| Скалярные armoury-атрибуты (PPT/boost/temp) нечитаемы на части моделей | средний | capability-probe читает CurrentValue; UI показывает Unsupported |
| Cardwire ранняя разработка, Wayland-only | средний | экспериментальный флаг; чёткая маркировка |
| Конфликт владельцев platform_profile (asusd vs ppd vs tuned) | средний | политика единственного владельца + предупреждение |
| Нет стабильного интерфейса undervolting | средний | раздел скрыт/Experimental; никаких MSR/debugfs в обычной сборке |
| MUX-переключение защёлкивается firmware (reboot) | высокий | pending-state модель; диалоги с требованием reboot |
| utajum/g-helper-linux без LICENSE | низкий | не переиспользуем его код |
| Различия версий asusd (6.3.8 vs 6.3.11) в D-Bus | средний | runtime introspection + version-проверка |

---

## 10. Ссылки

- G-Helper: https://github.com/seerge/g-helper
- asusctl: https://github.com/OpenGamingCollective/asusctl
- cardwire: https://github.com/OpenGamingCollective/cardwire
- g-helper-linux (Rust): https://github.com/UdayaSri0/g-helper-linux
- g-helper-linux (Avalonia): https://github.com/utajum/g-helper-linux
- Ayuz: https://github.com/Traciges/Ayuz
- supergfxd: https://github.com/SuperGify/supergfxd
- Kernel ABI: Documentation/ABI/testing/sysfs-platform_profile.txt,
  Documentation/ABI/testing/sysfs-class-power (charge_control_end_threshold),
  Documentation/ABI/testing/sysfs-class-led, hwmon sysfs ABI,
  DRM connector properties.
- UPower: https://upower.freedesktop.org/
- power-profiles-daemon: https://gitlab.freedesktop.org/hadess/power-profiles-daemon
- Slint: https://slint.dev/
