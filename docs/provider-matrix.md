# Provider Matrix — Orbis Control

> Дата: 2026-08-06 (обновлено после ревью Этапа 0). Связывает функции
> (`docs/feature-matrix.md`) с провайдерами, backend-интерфейсами и статусами.
> Полный контракт trait-ов — в `docs/architecture.md`.
> Целевые дистрибутивы: **NixOS** (официальная платформа разработки), Fedora,
> Arch, Ubuntu LTS, Debian, openSUSE.

## Принципы

1. Бизнес-логика не привязывается к конкретному проекту — используется ранжируемый
   список провайдеров на каждую функцию.
2. Провайдер обязан: `probe()`, `capabilities()`, `read_state()`, write-методы,
   `validate_request()`, `health()`, `diagnostics()`, timeout, человекочитаемое
   объяснение отсутствия поддержки, идентификатор/версию backend, классификацию риска.
3. Наличие файла/объекта ≠ поддержка записи. Проверяется: существование, тип,
   чтение, запись, диапазон, read-back, стабильность, соответствие DMI,
   отсутствие конфликтующего владельца.
4. Приоритет backend: asusd(D-Bus) → asus-armoury(kernel) → стандартный kernel ABI →
   узкий helper → экспериментальный интерфейс (feature flag).

### Семантика capability-статусов (уточнена после ревью)

Для каждого неработающего атрибута провайдер обязан сохранять: путь; тип операции;
точный `errno`; текст ошибки; права; владельца; режим файла; `uevent` (если применимо);
драйвер; kernel version; результат повторной проверки; результат проверки через asusd;
результат D-Bus introspection; предположительную стабильность ошибки.

| Статус | Условие |
|---|---|
| `PermissionDenied` | ядро или D-Bus отклоняет операцию из-за прав (EACCES/EPERM/polkit) |
| `Unsupported` | драйвер/firmware устойчиво возвращает `ENODEV`/`ENOTSUP`/`EOPNOTSUPP`, реализация отсутствует для модели, поддержка достоверно опровергнута |
| `TemporarilyUnavailable` | функция может стать доступна без изменения программы/оборудования (смена профиля, подключение питания, загрузка драйвера, выход GPU из переходного состояния, перезапуск backend) |
| `ReadOnly` | значение достоверно читается, запись отсутствует или запрещена архитектурой backend |
| `Unknown` | информации недостаточно |

**Пример FA707NV (см. фикстуру `tests/fixtures/hardware/fa707nv/`):**

- `ppt_*`, `nv_dynamic_boost`, `nv_temp_target`: via asusd — `Unsupported` (ENODEV,
  errno 19, устойчиво; `SupportedProperties` без PPT-группы); via kernel — read
  `Supported`, write `PermissionDenied` (файлы `-rw-r--r-- root:root`).
  Эффективный статус UI без hardwared: `ReadOnly`. Семантика значений
  (`ppt_pl1_spl=5`) — верифицируется на Этапе 3.
- `cpufv`: `PermissionDenied` (файл `0200 root:root`, чтение EACCES).
- `gpu_mux_mode`: `SupportedWithRequirement (Reboot)` — MUX защёлкивается firmware;
  наблюдаемое расхождение «requested=0, DRM eDP активен на dGPU».
- `charge_mode`: `ReadOnly` (sysfs `0444`, режим задаётся через asusd/WMI).

---

## 1. PerformanceProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | asusd | `xyz.ljones.Platform.PlatformProfile` (+ Choices) | get/set |
| 2 | kernel ABI | `/sys/firmware/acpi/platform_profile`, `_choices` | get/set |
| 3 | power-profiles-daemon | `org.freedesktop.UPower.PowerProfiles` | get/set (только при отсутствии конфликта) |

Дополнительно (asusd): `PlatformProfileOnAc`, `PlatformProfileOnBattery`,
`Profile{Quiet,Balanced,Performance}Epp`, `PlatformProfileLinkedEpp`.

Статус на эталоне (FA707NV, asusd 6.3.8): **Supported**, профили `[LowPower, Quiet, Balanced, Performance]`.

---

## 2. FanProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | asusd | `xyz.ljones.FanCurves` (FanCurveData, SetFanCurve, SetFanCurvesEnabled, SetCurvesToDefaults) | кривые по профилям |
| 2 | kernel ABI | hwmon `asus_custom_fan_curve` (`pwm*_auto_point{1..8}`) | read + write (через hardwared, если нужно) |
| 3 | hwmon (read-only) | hwmon `fan*_input`, `fan*_label` | RPM/телеметрия |

Формат кривой D-Bus (asusd): `(s(yyyyyyyy)(yyyyyyyy)b)` = профиль, 8×(temp,pwm)
CPU, 8×(temp,pwm) GPU, enabled.

Статус на эталоне: **Supported** (hwmon9: cpu_fan/gpu_fan; hwmon10: 8 точек × 2 вентилятора).

---

## 3. PowerLimitProvider

| Приоритет | Backend | Интерфейс | Параметры |
|---|---|---|---|
| 1 | asusd | `xyz.ljones.AsusArmoury` объекты `/ppt_pl1_spl`, `/ppt_pl2_sppt`, `/ppt_pl3_fppt`, `/nv_dynamic_boost`, `/nv_temp_target` | SPL/SPPT/FPPT, Dynamic Boost, GPU temp target |
| 2 | kernel ABI | asus-nb-wmi `ppt_pl1_spl` и т.д. | то же |
| 3 | — | CPU temperature limit (по моделям) | asusd/asus-armoury |

Важно (FA707NV): via asusd объекты существуют, но `CurrentValue` падает с ENODEV
(errno 19) — статус через asusd **Unsupported**; прямое чтение kernel работает
(read **Supported**), запись требует root (write **PermissionDenied**). Эффективный
статус без hardwared: **ReadOnly**. UI обязан это отражать; семантика значений
`ppt_*=5` верифицируется на Этапе 3.

---

## 4. BatteryProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | UPower | `org.freedesktop.UPower` + `.Device` | state, %, energy-rate, capacity, cycles, source type |
| 2 | asusd | `Platform.ChargeControlEndThreshold`, `OneShotFullCharge` | лимит зарядки |
| 3 | kernel ABI | `/sys/class/power_supply/BAT*/charge_control_end_threshold` | лимит зарядки (fallback) |
| 4 | kernel ABI | power_supply sysfs | raw-показания (fallback) |

Статус на эталоне: **Supported** (UPower BAT1 A32-K55; заряд-лимит 80 через asusd и sysfs).

---

## 5. GpuProvider

Три независимых сущности (физический MUX / доступ приложений / power state).
Провайдер разбит на под-провайдеры:

### 5.1 MuxProvider (физический MUX)

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | asusd | `AsusArmoury/gpu_mux_mode` (CurrentValue, PossibleValues, QueuedGpuValue, ApplyQueuedGpuValue) | read/queue/apply |
| 2 | kernel ABI | asus-nb-wmi `gpu_mux_mode` | read/queue (hardwared) |
| 3 | supergfxd | `org.supergfxctl.Daemon` Mode/SetMode | legacy fallback |

Статус на эталоне: **SupportedWithRequirement (Reboot)** — `possible_values=[0,1]`, current=0.

### 5.2 GpuAccessProvider (доступ приложений)

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | Cardwire (экспериментальный) | `org.opengamingcollective.cardwire.Gpu` set_block/block, `Mode` | block/unblock; Wayland-only |
| 2 | asusd | `AsusArmoury/dgpu_disable` | аппаратное отключение (Eco) |
| 3 | supergfxd | `SetMode(integrated)` | legacy |

Статус на эталоне: Cardwire отсутствует (BackendMissing); `dgpu_disable` available `[0,1]`.

### 5.3 GpuPowerStateProvider (фактический power state)

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | sysfs/DRM | `/sys/bus/pci/devices/*/power/runtime_status`, hwmon | read-only |
| 2 | supergfxd | `Power()` | read |
| 3 | NVML/nvidia-smi (read-only, timeout) | температуры/мощность | read |
| 4 | Cardwire | `Gpu.power_state`, signal `power_state_changed` | read |

Не будить dGPU ради телеметрии; устаревшее значение помечать как `Sleeping`/stale.

---

## 6. DisplayProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | KScreen (Plasma) | KDE KScreen D-Bus | режимы/частота |
| 2 | Mutter (GNOME) | стабильные интерфейсы | режимы/частота |
| 3 | wlr-randr | внешняя команда через типизированный adapter + timeout + строгий парсинг | режимы/частота |
| 4 | RandR (X11) | RandR API | режимы/частота |
| 5 | DRM (read-only) | connector properties (поиск внутреннего дисплея) | discovery |

Отдельный под-провайдер PanelOverdrive: asusd `AsusArmoury/panel_overdrive`
(на эталоне Supported, current=1).

---

## 7. LightingProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | asusd | `xyz.ljones.Aura` (led_mode, led_mode_data, brightness, led_power, supported_*) | RGB/эффекты/яркость |
| 2 | kernel LED | `/sys/class/leds/asus::kbd_backlight` | brightness-only |
| 3 | — | без hidraw reverse engineering в стабильной версии | — |

Статус на эталоне: Aura доступна (`/xyz/ljones/aura/tuf`); kbd LED присутствует.

---

## 8. AnimeProvider / SlashProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | asusd | `xyz.ljones.Anime` (write, brightness, builtin_animations, off_*) | AniMe Matrix |
| 1 | asusd | `xyz.ljones.Slash` | Slash Lighting |
| 2 | kernel LED | (по моделям) | brightness fallback |

Модули появляются только при фактическом наличии устройства (probe).

---

## 9. HotkeyProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | XDG Global Shortcuts portal | ashpd `global_shortcuts` (session bus) | bind/list/trigger |
| 2 | KDE/GNOME адаптеры | специфичные D-Bus | fallback |
| 3 | X11 | X11 grabs | legacy |

Пользовательская команда: список аргументов, подтверждение, без `/bin/sh -c`.

---

## 10. TelemetryProvider

| Приоритет | Backend | Интерфейс | Данные |
|---|---|---|---|
| 1 | hwmon | sysfs | температуры, вентиляторы |
| 2 | powercap | `/sys/class/powercap/*` | мощности (CPU/GPU при наличии) |
| 3 | thermal zones | `/sys/class/thermal/thermal_zone*` | температуры |
| 4 | UPower | D-Bus | батарея |
| 5 | DRM/sysfs | PCI runtime status | GPU power state |
| 6 | NVML (nvidia) | библиотека | dGPU метрики (read-only) |
| 7 | nvidia-smi | внешняя команда, timeout | fallback (read-only) |

Правило: fast telemetry 1 s; не читать десятки файлов каждые 100–250 ms; не будить dGPU.

---

## 11. FirmwareUpdateProvider

| Приоритет | Backend | Интерфейс | Действия |
|---|---|---|---|
| 1 | fwupd | `org.freedesktop.fwupd` D-Bus | поиск/установка firmware (BIOS — только вручную) |
| 2 | пакетный менеджер | системный | обновление приложения |
| 3 | ссылки ASUS | https | страницы поддержки |

---

## 12. Сводная таблица по backend-ам

| Backend | Транспорт | Функции | Статус на эталоне |
|---|---|---|---|
| asusd | system D-Bus | профили, лимит, вентиляторы, Aura, Anime, Slash, armoury | активен 6.3.8 |
| asus-armoury (kernel) | sysfs + D-Bus | MUX, dgpu_disable, PPT, boost, temp, panel_od | активен (ядро 7.1.6) |
| kernel ABI | sysfs | platform_profile, charge limit, hwmon, leds, backlight | активен |
| UPower | system D-Bus | батарея/AC | активен |
| power-profiles-daemon | system D-Bus | профили (альтернатива) | не установлен |
| supergfxd | system D-Bus | GPU режимы (legacy) | активен |
| Cardwire | system D-Bus | GPU block (эксперимент) | не установлен |
| KScreen/Mutter/wlr-randr/RandR | D-Bus/процессы | дисплей | Plasma активен |
| fwupd | system D-Bus | firmware | не проверялся |
| logind | system D-Bus | sleep/lid | активен |

---

## 13. Требования к провайдерам (чеклист качества)

Для каждого провайдера в коде обязательны:

- [ ] `probe()` — безопасный read-only discovery
- [ ] `capabilities()` — полный capability-статус (§7 задания)
- [ ] `read_state()` — актуальное состояние
- [ ] write-методы с `validate_request()` до записи
- [ ] timeout на все внешние операции (D-Bus/sysfs/процессы)
- [ ] `health()` — жив ли backend
- [ ] `diagnostics()` — данные для раздела Diagnostics
- [ ] человекочитаемое объяснение отсутствия поддержки
- [ ] `backend_id` + `backend_version`
- [ ] классификация риска операции (safe / confirmation / dangerous / experimental)
- [ ] read-back после записи
- [ ] проверка конфликтующего владельца (кто ещё пишет в интерфейс)
