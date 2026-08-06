# Feature Matrix — Orbis Control

> Дата: 2026-08-06 (обновлено после ревью Этапа 0). Источники:
> `docs/research-report.md`, живой D-Bus introspection на ASUS TUF Gaming A17 FA707NV
> (ядро 7.1.6, asusd 6.3.8), исходники asusctl 6.3.11, G-Helper e439349f,
> cardwire 1de887f.
> Целевые дистрибутивы: **NixOS** (официальная платформа разработки), Fedora,
> Arch, Ubuntu LTS, Debian, openSUSE.

## Статусы функций

| Статус | Значение |
|---|---|
| ✅ полноценная | Реализуема штатно, интерфейс стабилен |
| 🟡 частичная | Реализуема с ограничениями |
| 🔵 backend-dependent | Реализация зависит от установленного backend/модели |
| 🟣 экспериментальная | Только за экспериментальным флагом |
| ⛔ недостижимая | Нет стабильного Linux-интерфейса |

Сопоставление статусов с capability-моделью (§7 задания) приведено в
`docs/architecture.md`. Семантика статусов (что значит каждый статус, пример
FA707NV) — в `docs/provider-matrix.md` §Семантика capability-статусов.

---

## 1. Режимы производительности

| Функция G-Helper | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Silent / Balanced / Turbo | ✅ | `platform_profile` quiet/balanced/performance | asusd `xyz.ljones.Platform` → kernel ABI |
| Список профилей из системы (не хардкод) | ✅ | `PlatformProfileChoices` | asusd |
| Синхронизация внешних изменений | ✅ | сигналы PropertiesChanged | asusd / kernel |
| Профиль AC / батарея | ✅ | `PlatformProfileOnAc/OnBattery` | asusd |
| Профиль слабого USB-C зарядного | 🟡 | UPower (тип/мощность источника) + отдельная политика | sessiond (автоматизация) |
| Причина degraded performance | 🟡 | ppd `PerformanceDegraded` / свои проверки | ppd / asusd |
| Thermal throttling индикация | 🟡 | thermal zones (hwmon) | hwmon |
| Конфликт владельцев политики | ✅ | детект ppd/tuned/asusd; выбор владельца | sessiond |

Решение: трёхкнопочная модель сохраняется, но кнопки выводятся из фактического
списка профилей. Если `performance` отсутствует — Turbo отключается с объяснением,
не подделывается.

---

## 2. GPU-режимы

Три независимые сущности: **физический MUX**, **доступ приложений к dGPU**,
**фактический power state dGPU**. Не объединяются в одно булево значение.

| Функция G-Helper | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Eco (отключение dGPU) | 🔵 | Cardwire block ИЛИ `dgpu_disable` ИЛИ supergfxd integrated ИЛИ read-only статус | cardwire / asusd-asus_armoury / supergfxd |
| Standard (hybrid) | 🔵 | гибрид по умолчанию; MUX → iGPU при наличии | kernel/asusd |
| Ultimate (MUX → dGPU) | ✅ | `gpu_mux_mode` (queued, reboot) | asusd `AsusArmoury` |
| Optimized (политика AC/battery) | 🔵 | sessiond-автоматизация: battery→eco, AC→standard | sessiond |
| Проверка перед Eco | ✅ | lsof/процессы, внешние мониторы, CUDA/ROCm/Vulkan, владелец дисплея | sessiond + cardwire `Gpu.lsof` |
| Pending MUX state + отмена | ✅ | `QueuedGpuValue`, `ApplyQueuedGpuValue` | asusd |
| Индикация реального состояния после reboot | ✅ | read текущего значения после загрузки | asusd |
| Не отображать Ultimate активным без подтверждения | ✅ | capability + read-back | asusd |

Требования: Eco проверяет dGPU-процессы; при подключённом внешнем мониторе на
dGPU — отказ с понятным объяснением. Cardwire помечается как экспериментальный и
как «блокировка доступа», не «отключение» (≠ физический MUX).

---

## 3. Экран

| Функция G-Helper | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Авточастота | 🔵 | KScreen D-Bus (Plasma) → Mutter → wlr-randr → RandR | DE-specific |
| Min/max частота | 🔵 | список mode из DRM/KScreen | DE-specific |
| Overdrive | 🔵 | `panel_overdrive` | asusd `AsusArmoury` |
| MiniLED single/multi-zone | 🔵 | asusd (по моделям) | asusd |
| HDR-aware поведение | 🟡 | протоколы Wayland/DE; пометка «отключено из-за HDR» | DE-specific |
| Частота AC / батарея | 🔵 | автоматизация sessiond | sessiond |
| Восстановление после resume | ✅ | logind `PrepareForSleep` + re-apply | sessiond |
| Несколько внутренних дисплеев | 🟡 | DRM connector discovery | DRM |

Запрет: прямой DRM atomic mode setting из GUI. Внутренний дисплей ищется через
DRM connector properties и фактическое состояние (не `eDP-1`).

---

## 4. Flicker-free dimming / Visual modes

| Функция | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Яркость | ✅ | backlight sysfs + DE | kernel |
| OLED / flicker-free dimming | 🔵 | asusd/asus-armoury (по моделям) | asusd |
| Visual mode / gamut / профили | 🔵 | asusd (по моделям) + ColorProfileHelper-эквивалент | asusd / DE |
| Раздельные AC/батарея | 🔵 | автоматизация sessiond | sessiond |
| Пояснение при HDR-отключении | ✅ | UI-подсказка | — |
| Не подменять gamma-коррекцией | ✅ | явная маркировка «gamma» при её использовании | — |

Секция отображается **только при реальной поддержке**.

---

## 5. Подсветка клавиатуры

| Функция G-Helper | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Off / Static / Breathing / Strobing / Color Cycle / Rainbow | 🔵 | `Aura.led_mode` + `all_mode_data` | asusd `xyz.ljones.Aura` |
| Эффекты конкретной модели | 🔵 | `supported_basic_modes` | asusd |
| Яркость | ✅ | `Aura.brightness` / `asus::kbd_backlight` LED | asusd / kernel leds |
| RGB, скорость, направление | 🔵 | `Aura.led_mode_data` | asusd |
| Поведение при загрузке/sleep | 🔵 | asusd конфиг | asusd |
| Выключение после таймаута | 🔵 | автоматизация sessiond (таймеры) | sessiond |
| Таймаут AC / батарея | 🔵 | автоматизация | sessiond |
| FN-Lock | 🔵 | только стабильный backend (нет в стабильной версии — скрыто) | — |

Приоритет: asusd Aura → kernel LED → brightness-only → без hidraw-reverse
engineering в стабильной версии.

---

## 6. AniMe Matrix и Slash Lighting

| Функция | Статус | Linux-реализация | Backend |
|---|---|---|---|
| AniMe Off / анимации / изображение / GIF / часы / дата / audio visualizer | 🔵 | `Anime` D-Bus (`write(AnimeDataBuffer)`, `builtin_animations`) | asusd `xyz.ljones.Anime` |
| AniMe brightness | 🔵 | `Anime.brightness` | asusd |
| Startup/sleep/shutdown анимации | 🔵 | `off_when_lid_closed`, `off_when_suspended`, `off_when_unplugged`, конфиг | asusd |
| Ограничение частоты обновления | ✅ | sessiond rate-limit | sessiond |
| Slash: режим/яркость/скорость/интервал | 🔵 | `Slash` D-Bus | asusd |
| Slash: off на батарее / при закрытии крышки | 🔵 | asusd + sessiond | asusd/sessiond |

Модули — отдельные capability-модули; появляются только при наличии устройства.

---

## 7. Лимит зарядки и батарея

| Функция | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Ползунок лимита (40–100 или фактические min/max/step) | ✅ | asusd `ChargeControlEndThreshold` → `charge_control_end_threshold` | asusd / kernel |
| Текущее значение, состояние, мощность | ✅ | UPower `State/EnergyRate/Percentage` | UPower |
| Здоровье батареи | ✅ | UPower `Capacity` | UPower |
| Циклы (если доступны) | 🟡 | UPower `ChargeCycles` (N/A на части моделей) | UPower |
| Уведомление о перезаписи другим сервисом | ✅ | сигналы PropertiesChanged | sessiond |
| Повторное применение после resume только при необходимости | ✅ | logind + compare | sessiond |
| Нет постоянного секундного write | ✅ | сравнение перед записью | sessiond |

---

## 8. Вентиляторы (Fans + Power)

| Функция | Статус | Linux-реализация | Backend |
|---|---|---|---|
| CPU/GPU/Mid/System вентиляторы, динамический список | ✅ | hwmon `fan*_input`, `fan*_label` + D-Bus FanCurves | hwmon/asusd |
| RPM и процент | 🟡 | hwmon input; процент через max диапазон | hwmon |
| 8-точечные кривые | ✅ | `asus_custom_fan_curve` `pwm*_auto_point{1..8}` | kernel/asusd |
| Кривая на каждый профиль | ✅ | D-Bus `FanCurveData(profile)` | asusd |
| Factory defaults / Apply / Reset | ✅ | `SetCurvesToDefaults` / `SetFanCurve` | asusd |
| Import/export | ✅ | JSON (конфиг) | — |
| Режим RPM/проценты, grid, keyboard nav, numeric editor | ✅ | UI (Slint) | — |
| Предупреждение об опасной кривой | ✅ | валидация + диалог | — |

Валидация: ровно столько точек, сколько требует backend; температуры и значения не
убывают (если backend не поддерживает обратное); последняя точка безопасна; нельзя
нулевой вентилятор на критической температуре; read-back после записи.

> Приложение передаёт кривую firmware/EC и не является real-time fan controller.

---

## 9. Power limits

| Параметр | Статус | Linux-реализация | Backend |
|---|---|---|---|
| SPL (PPT PL1), SPPT (PL2), FPPT (PL3) | 🔵→🟡 | `ppt_pl1_spl`, `ppt_pl2_sppt`, `ppt_pl3_fppt` | asusd `AsusArmoury` / kernel |
| CPU temperature limit | 🔵 | asusd/asus-armoury (по моделям) | asusd |
| GPU Dynamic Boost | 🔵→🟡 | `nv_dynamic_boost` | asusd / kernel |
| GPU temp target | 🔵→🟡 | `nv_temp_target` | asusd / kernel |
| CPU boost policy / EPP | ✅ | EPP-свойства asusd + sysfs | asusd |
| Динамическое определение полей | ✅ | `AvailableAttrs`/`MinValue/MaxValue/ScalarIncrement` | asusd |

> **Уточнение для FA707NV (не фиксировать по наличию файлов!):** via asusd D-Bus
> `ppt_*`/`nv_*` устойчиво падают с ENODEV (errno 19) → статус через asusd
> `Unsupported` (`EnablePptGroup=false`, `SupportedProperties` без PPT-группы).
> Прямое чтение kernel sysfs работает (read `Supported`), запись требует root
> (write `PermissionDenied`). Эффективный статус UI без hardwared: **ReadOnly**.
> Семантика значений (`ppt_pl1_spl=5`) — верифицируется на Этапе 3.
> `cpufv` (CPU boost): `PermissionDenied` (файл 0200 root:root).
> Детали: `docs/research-report.md` §4.1.1, фикстура
> `tests/fixtures/hardware/fa707nv/expected-capabilities.json`.

Правила: min/max/step/current/default от backend; никаких универсальных значений
мощности; перед записью — проверка диапазона, подтверждение, read-back, журнал,
reset; без фонового бесконечного повторного применения.

---

## 10. CPU boost и undervolting

| Функция | Статус | Linux-реализация | Backend |
|---|---|---|---|
| CPU boost вкл/выкл | 🔵 | `cpufv`/`cpufreq` (AMD), Intel P-states | kernel/asusd |
| Undervolting | 🟣 | нет стабильного mainline-интерфейса; скрыт по умолчанию; Experimental | — |

Undervolting: скрыт; подтверждённая поддержка; предупреждение; не применяется при
первом запуске; reset; детект сбоя драйвера; никаких MSR/debugfs в обычной сборке.

---

## 11. Горячие клавиши

| Функция | Статус | Linux-реализация | Backend |
|---|---|---|---|
| Открыть/скрыть окно, цикл режимов, toggle lighting, Eco/Standard, overlay | 🟡 | XDG Global Shortcuts portal | ashpd |
| KDE/GNOME адаптеры | 🟡 | специфичные интеграции | DE |
| X11 fallback | 🟡 | X11 grabs | X11 |
| Пользовательская команда | 🟡 | список аргументов (без `/bin/sh -c`), подтверждение | sessiond |

Не читать `/dev/input/event*` от root без отдельной причины; не выдавать широкие
input group permissions автоматически.

---

## 12. Трей

| Функция | Статус | Реализация |
|---|---|---|
| Open, профили, GPU-режимы, battery limit, Diagnostics, About, Quit | ✅ | StatusNotifierItem (ksni) |
| Закрытие окна → tray (если tray есть) | ✅ | UI-логика |
| Первое закрытие — объяснение | ✅ | диалог |
| Quit → GUI (+ sessiond по настройке) | ✅ | D-Bus команда |
| Single-instance activation | ✅ | D-Bus name owner / activation |

---

## 13. Автоматизация

| Правило | Статус | Реализация |
|---|---|---|
| Профиль при AC/батарее | ✅ | sessiond + UPower signals |
| Профиль при USB-C low-power | 🟡 | UPower источник + мощность |
| GPU policy AC/батарея (Optimized) | 🔵 | sessiond |
| Refresh rate AC/батарея | 🔵 | sessiond + display provider |
| Keyboard backlight timeout | 🔵 | sessiond таймеры |
| Lighting on battery | 🔵 | sessiond |
| Действие при внешнем мониторе | 🔵 | DRM/KScreen сигналы |
| Закрытие крышки — через logind | ✅ | logind-интеграция |
| Reapply после resume | ✅ | logind `PrepareForSleep` |
| Configurable delay / cooldown / приоритеты / отмена при изменении условий | ✅ | sessiond-движок |

---

## 14. Диагностика

Полный набор из задания §8.14 реализуется sessiond + CLI (`orbisctl diagnostics`).
Экспорт обезличивается (username, hostname, serial, MAC, UUID дисков, домашние пути).

---

## 15. Обновления

| Функция | Статус | Реализация |
|---|---|---|
| Обновление приложения (пакет) | ✅ | пакетный менеджер; не заменять бинарник в /usr/bin |
| AppImage portable | ✅ | отдельный механизм обновления |
| Firmware/BIOS | 🔵 | fwupd (D-Bus `org.freedesktop.fwupd`); не ставить BIOS автоматически |
| Драйверы/kernel | 🟡 | ссылки/инструкции, не автоматика |
| Сетевые проверки отключаемы; без телеметрии | ✅ | конфиг |

---

## 16. Overlay

| Функция | Статус | Реализация |
|---|---|---|
| Прозрачное always-on-top окно | ✅ | layer-shell (Wayland), X11 fallback |
| CPU/GPU temp, power, load, fan RPM, battery | ✅ | sessiond телеметрия |
| FPS | 🟡 | MangoHud интеграция; без собственного Vulkan/OpenGL injection |
| Источник данных явно помечается | ✅ | UI |

---

## 17. Периферия и прочее

| Функция G-Helper | Статус | Комментарий |
|---|---|---|
| M-клавиши, NumberPad | ⛔ | нет стабильного Linux-интерфейса; возможно позже через input-адаптеры |
| Мыши ASUS | ⛔ | проприетарные протоколы; не в стабильной версии |
| ROG Ally TDP | 🟡 | отдельный проект/`asusd` (вне скоупа v1) |
| XG Mobile | 🟡 | asusd XgmLed/ScsiAura; ранняя поддержка |
| «Режимы игр» Armoury Crate | ⛔ | аналога нет |
| Updates/Donate/About в футере | ✅ | UI |

---

## Итоговый вывод

- **Полноценно** реализуются: режимы производительности, AC/battery-профили,
  MUX Ultimate (с reboot), лимит зарядки, статистика батареи, вентиляторы и кривые,
  Aura-подсветка, AniMe/Slash (при наличии), трей, автоматизация, диагностика,
  обновления приложения.
- **Backend-dependent**: GPU Eco/Optimized, экран, visual modes, power limits.
- **Экспериментальные**: undervolting, Cardwire block.
- **Недостижимые**: M-клавиши/NumberPad/мыши ASUS, «режимы игр», DirectX-оверлей.
