# UI Reference — Orbis Control

> Дата: 2026-08-06. Normative reconciliation: 2026-08-24 (#128).
> Источник измерений: исходники G-Helper `e439349f` (app/Settings.Designer.cs,
> app/Fans.Designer.cs, app/Extra.Designer.cs, app/UI/RForm.cs, app/Settings.cs).
> Методика: WinForms `AutoScaleDimensions = 192×192` → нормализованные 96-DPI
> логические пиксели (деление на 2), далее сверка со скриншотами (запланировано).

## 0. Normative Orbis UI specification (2026-08-24, redesign #128)

Orbis Control — **не клон G-Helper**: одно окно, четыре раздела, своя
навигация. Владелец утвердил редизайн (spec:
`docs/superpowers/specs/2026-08-24-ui-redesign-design.md`); разделы 1–9 ниже —
исторический источник измерений G-Helper. Компилируемый UI: `ui/app-entry.slint`
→ `ui/audited/main-window.slint` (шелл) + `ui/audited/sections/*` +
`ui/components/*` + `ui/themes/*`.

### Каркас

| Параметр | Значение |
|---|---|
| Окно | 760×600 logical px, фиксированное, `no-frame`, `background: transparent` |
| Скругление | корневой Rectangle radius 10 + clip (все углы) |
| Титлбар | 40px, `TitleBar`: имя раздела + drag-область + «свернуть»/«закрыть» (глифы – / ×; close hover — error) |
| Drag | slint `unstable-winit-030` → winit `drag_window()` по pointer-down |
| Close | общий close-action путь (`window_lifecycle_backend::handle_close_request`: tray/quit) |
| Сайдбар | `Sidebar` 168px: Dashboard / Fans / Hardware / Settings; Rectangle-глифы 16px (accent при активности) |
| Контракт окна | `check-ui-contract.py`: 400≤w≤800, 400≤h≤640 |

Важно: **software-renderer Slint не рендерит `Path`** — иконки и глифы строятся
из Rectangle/Text-примитивов (spec §8, фолбэк применён).

### Разделы

- **Dashboard** (`sections/dashboard.slint`): Performance и GPU Mode —
  `SegmentedTrack` (выбранный сегмент залит акцентом режима); Display и
  Keyboard — строки `DropdownRow` (ComboBox); Battery — одна строка
  (slider + «100%» + health/power).
- **Fans** (`sections/fans.slint`): `FanCurveEditor` + write-safety статус
  (fan writes остаются hard-blocked).
- **Hardware** (`sections/hardware.slint`): Hotkeys M1–M5, Aura effect/speed
  (read-only), Panel Overdrive (`RequestToggleRow`), Boot sound (read-only),
  Status LEDs / clamshell / ASPM / standby (draft-строки), Power/CPU
  (iGPU memory, hibernate, P/E-cores). Все честные disabled-статусы сохранены.
- **Settings** (`sections/settings.slint`): тема Dark/Light, autostart,
  start minimized, remember position, close action, Diagnostics
  (Refresh / Copy / Export; Copy — через toolkit clipboard из privacy-safe
  summary), About (версия + профиль).

### Удалённые поверхности

Updates (нужен сервер распространения) и Automation-редактор (execution
остаётся за safety-gate) убраны из UI. Rust-слой Automation shadow
(`automation_backend.rs`) сохранён как observation-only ядро. Отдельные окна
Preferences/Diagnostics/Fans/Extra/Updates/Automation удалены вместе с глеем.

### Палитра — Catppuccin (норматив)

Dark = Mocha, Light = Latte (https://catppuccin.com). Полный маппинг:

| Токен | Mocha | Latte |
|---|---|---|
| window-background | #1E1E2E | #EFF1F5 |
| titlebar-background | #181825 | #E6E9EF |
| surface-default | #313244 | #CCD0DA |
| surface-hover | #45475A | #BCC0CC |
| surface-pressed | #181825 | #DCE0E8 |
| surface-selected | #414356 | #C6CEE0 |
| surface-disabled | #262637 | #E0E3EA |
| border-default | #45475A | #9CA0B0 |
| border-strong | #585B70 | #7C7F93 |
| text-primary | #CDD6F4 | #4C4F69 |
| text-secondary | #A6ADC8 | #5C5F77 |
| text-disabled | #6C7086 | #9CA0B0 |
| text-on-accent | #11111B | #EFF1F5 |
| accent-default | #89B4FA | #1E66F5 |
| warning | #F9E2AF | #DF8E1D |
| error | #F38BA8 | #D20F39 |
| success | #A6E3A1 | #40A02B |
| silent / eco | #A6E3A1 | #40A02B |
| balanced / standard | #89B4FA | #1E66F5 |
| turbo | #F38BA8 | #D20F39 |
| ultimate | #FAB387 | #FE640B |
| optimized | #94E2D5 | #179299 |

Контракт контраста: `check-ui-contract.py` проверяет WCAG AA для текстовых
токенов (Latte text-secondary = subtext1 #5C5F77 именно поэтому).

### Скриншоты

`cargo run -p orbis-ui --example ui_snapshot -- <dashboard|fans|hardware|settings|dialog> [path] [dark|light]`
и `orbis-control --features ui-review --screenshot <path> --ui-section <s>`
(`--ui-state default|pending|disabled|error`). Базовые сценарии §10 актуальны.

## 1. Методология (историческая)

1. Из `*.Designer.cs` извлекаются `ClientSize`, `Location`, `Size`, `Padding`,
   `Margin`, `RowStyles`, `ColumnStyles`, шрифты, радиусы, цвета, visible/hidden.
2. G-Helper использует `AutoScaleDimensions = 192×192` (DPI-aware). Все размеры в
   «192-DPI пикселях»; для 96-DPI нормализация = ÷2.
3. Нормализация не является финальной без визуальной проверки: после появления
   эталонных скриншотов значения сверяются и корректируются.
4. Результаты сохраняются в `docs/ui-measurements.json` (машиночитаемо) и здесь
   (человекочитаемо).
5. После фиксации спецификации применяется visual regression (см. §10).

## 2. Размеры окон (измерено)

| Окно | ClientSize @192DPI | Нормализовано |
|---|---|---|
| Settings (главное) | 849 × 2075 | 424.5 × 1037.5 |
| Fans + Power | 1350 × 1100 | 675 × 550 |
| Extra | 1013 × 1759 | 506.5 × 879.5 |

Высота главного окна динамическая (G-Helper скрывает неподдерживаемые секции).

## 3. Главное окно — структура (сверху вниз)

1. Кастомный title bar (своя реализация в G-Helper: RForm)
2. Performance Mode (панель 827×208 → 413.5×104)
3. GPU Mode (панель 827×432 → 413.5×216)
4. Laptop Screen (827×187 → 413.5×93.5)
5. Flicker-free Dimming / Visual Mode
6. Slash Lighting / AniMe Matrix (827×183)
7. Laptop Keyboard (827×146)
8. Battery Charge Limit (827×104)
9. Run on Startup + служебный статус
10. Версия
11. Donate / Updates / Quit (footer 827×88)

Боковой панели навигации **нет**.

## 4. Ключевые измерения

### Performance panel (панель 827×208 @192)

| Элемент | @192DPI | Нормализовано |
|---|---|---|
| panelPerformance | 827×208 | 413.5×104 |
| tablePerf (inner) | 787 | 393.5 |
| buttonSilent | 188×120 @ (4,4) | 94×60 |
| buttonBalanced | 188×120 @ (200,4) | 94×60 |
| buttonTurbo | 188×120 @ (396,4) | 94×60 |
| buttonFans | 191×120 @ (592,4) | 95.5×60 |
| Row height | 128 | 64 |

Раскладка: 4 колонки (Silent, Balanced, Turbo, Fans), gap 8 @192 (4 норм.),
padding панели 20 @192 (10 норм.).

### GPU panel (панель 827×432 @192)

| Элемент | @192DPI | Нормализовано |
|---|---|---|
| panelGPU | 827×432 | 413.5×216 |
| tableGPU | 787×256 | 393.5×128 |
| buttonEco | 188×120 @ (4,4) | 94×60 |
| buttonStandard | 188×120 @ (396,4) | 94×60 |
| buttonUltimate | 191×120 @ (592,4) | 95.5×60 |
| buttonOptimized | 188×120 @ (4,132) | 94×60 |

2 строки: [Eco][пусто][Standard][Ultimate]; [Optimized][пусто][пусто][пусто].

### Screen panel (827×187 @192)

| Элемент | @192DPI | Нормализовано |
|---|---|---|
| panelScreen | 827×187 | 413.5×93.5 |
| button60Hz | 188×72 @ (200,4) | 94×36 |
| button120Hz | 188×72 @ (396,4) | 94×36 |
| panelScreenTitle | 787×40 @ (20,11) | 393.5×20 |

### Battery panel (827×104 @192)

| Элемент | @192DPI | Нормализовано |
|---|---|---|
| panelBattery | 827×104 @ (11,1683) | 413.5×52 |
| sliderBattery | 707×40 @ (20,60) | 353.5×20 |
| buttonBatteryFull | 73×36 @ (728,62) | 36.5×18 |

### Footer (827×88 @192)

| Элемент | @192DPI | Нормализовано |
|---|---|---|
| panelFooter | 827×88 | 413.5×44 |
| buttonDonate | 254×48 | 127×24 |
| buttonUpdates | 254×48 | 127×24 |
| buttonQuit | 255×48 | 127×24 |

### Extra (ширина панелей 949 @192 → 474.5 норм.)

Padding: 15 @192 (7.5 норм.). Секции: Bindings (header 949×51, тело 949×395),
Backlight (header + тело 949×444, extra 949×115), Other/Settings (949×472),
CPU Cores (949×59), ACPI DEVS (949×69), iGPU memory (949×57), Power (949×54),
Asus Services (949×75).

### Fans + Power

| Элемент | @192DPI | Нормализовано |
|---|---|---|
| panelFans | 820×1100 | 410×550 |
| tableFanCharts | 810×918, 4 rowstyles 25% | 405×459 |
| chartCPU/GPU/Mid | 786×208 | 393×104 |
| chartXGM | 786×209 | 393×104.5 |
| panelTitleFans | 810×66 | 405×33 |
| комбопрофилей | 480×66 | 240×33 |
| Hysteresis | 810×130 | 405×65 |

## 5. Цвета

### G-Helper dark (исходные, для референса)

| Токен | G-Helper |
|---|---|
| formBack | #1C1C1C |
| buttonMain | #2E2E2E |
| buttonSecond | #242424 |
| foreMain | #F0F0F0 |
| chartMain | #232323 |

### Начальные dark-токены Orbis Control (не финальные; будут сверены со скриншотами)

```json
{
  "window.background": "#1F1F1F",
  "titlebar.background": "#242529",
  "surface.default": "#303030",
  "surface.hover": "#383838",
  "surface.pressed": "#292929",
  "surface.disabled": "#292929",
  "border.default": "#444444",
  "text.primary": "#F2F2F2",
  "text.secondary": "#B7B7B7",
  "text.disabled": "#777777",
  "accent.default": "#2DA8F2",
  "warning": "#E5A94F",
  "error": "#E35D6A",
  "success": "#4BC39A"
}
```

### Mode accents (начальные, не финальные)

```json
{
  "silent": "#49C6A5",
  "balanced": "#2DA8F2",
  "turbo": "#EF6376",
  "eco": "#67C66A",
  "standard": "#2DA8F2",
  "ultimate": "#E6A04B",
  "optimized": "#2DA8F2"
}
```

## 6. Типографика

- Системный sans-serif: Segoe UI (если есть), иначе Inter/Noto Sans/системный fallback.
- Body: 13–14 px; заголовки секций: 13–14 px semibold; телеметрия: 12–13 px.
- Не поставлять шрифт в проект без необходимости; layout не привязан к ширине текста.

## 7. Состояния кнопок (mode cards)

| Состояние | Визуал |
|---|---|
| normal | surface.default + border.default |
| hover | surface.hover (+4 % сдвиг, как RButton) |
| pressed | surface.pressed (+8 %) |
| selected | цветная рамка (accent), слегка изменённый background, без свечения |
| pending | пунктирная/анимированная рамка, маленький индикатор, tooltip (reboot/logout) |
| disabled | не только opacity; tooltip с причиной; focus не приводит к действию |
| error | рамка error + tooltip с деталями |

## 8. Геометрия (исходные базовые значения)

```text
content width           425 logical px (±8 после измерений)
title bar height        34 px
horizontal padding      10 px
section top padding     10 px
section title height    20 px
section gap             8–12 px
button row height       64 px
mode card height        60 px
card gap                4 px
card corner radius      5–6 px
border width            1–2 px
icon size in cards      20–24 px
footer button height    27–30 px
```

## 9. Окна

### Fans + Power
- Отдельное окно. Позиция: слева от главного, верхние границы совпадают, gap 8 px.
  Если слева нет места: справа → внутри монитора со смещением → по центру.
  Не открывать за пределами рабочей области.
- Размер по умолчанию 675×550 logical px (увеличение допустимо при крупном шрифте).
- Компоновка: слева кривые, справа power-контролы; низ: Factory Defaults + Apply;
  переключатель CPU/GPU; режим Grid; выбранный профиль сверху.

### Extra
- Отдельное прокручиваемое окно, ширина ≈ 506 logical px, высота ограничена рабочей областью.

## 10. Масштабирование и visual regression

- Проверяются: 100/125/150/175/200 %, fractional scaling Wayland, несколько мониторов
  с разным scale, перенос окна между мониторами. Логические размеры сохраняются
  (без ручного двойного масштабирования поверх Slint).
- Эталоны: `main-minimal`, `main-full`, `main-selected-silent`, `main-selected-turbo`,
  `main-gpu-pending`, `main-disabled-features`, `fans-default`, `fans-edited`,
  `extra`, `diagnostics`, `dialogs-reboot`, `dialogs-error`.
- Критерий: одинаковый logical size и scale; маскирование системного AA;
  perceptual diff; SSIM или эквивалент; ≤3 % различия для геометрических областей;
  смещение ключевых элементов ≤2 logical px после фиксации спецификации.
- Скриншоты G-Helper — только для внутреннего анализа, в пакет не включаются.
