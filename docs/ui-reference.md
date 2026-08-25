# UI Reference — Orbis Control

> Дата: 2026-08-06. Normative reconciliation: **2026-08-25** (sidebar redesign).
> Источник измерений: приложенный reference-макет (спокойная системная утилита,
> GNOME Settings / Windows 11 Settings class), утверждённый как главный
> визуальный источник истины. Разделы 1–9 ниже — исторический источник
> измерений G-Helper.
> Компилируемый UI: `ui/app-entry.slint` → `ui/audited/main-window.slint`
> (оболочка) + `ui/audited/sections/*` + `ui/components/*` + `ui/themes/*`.

## 0. Normative Orbis UI specification (2026-08-25, sidebar redesign)

Одно окно, боковая навигация, карточный контент. Не gaming dashboard: белые/
off-white поверхности, тонкие границы, один синий акцент, спокойная
типографика. Все элементы — переиспользуемые компоненты, связанные с реальными
данными (`UiState` + бэкенды); unsupported capabilities скрываются, а не
рисуются disabled-заглушками.

### Каркас

| Параметр | Значение |
|---|---|
| Окно | preferred 1200×800 logical px, минимум 980×680, `no-frame`, `background: transparent`, корневой radius 12 + clip |
| Титлбар | 44px, `TitleBar`: app-глиф + «Orbis Control» над сайдбаром, drag-область, свернуть/развернуть/закрыть (close hover — error) |
| Drag | slint `unstable-winit-030` → winit `drag_window()` по pointer-down; maximize — `Window::set_maximized` toggle |
| Close | общий close-action путь (`window_lifecycle_backend::handle_close_request`: tray/quit) |
| Сайдбар | 230px, фон `titlebar-background`, правая граница 1px `border-default`; `NavItem` 44px, radius 8, icon 18px + label 13px; selected = `accent-soft` + `accent-default` текст/иконка |
| Навигация | Главная · Производительность, Питание, Охлаждение, Графика, Подсветка, Экран, Система · Настройки, О программе (`Section`, 10 разделов) |
| Контент | фон `window-background`, padding 28, вертикальный шаг 16px, `ScrollView` на странице |
| Контракт окна | `check-ui-contract.py`: 1000≤w≤1400, 640≤h≤900 |

Важно: **software-renderer Slint не рендерит `Path` и повороты** — иконки
(`Icon`, 18×18) и кривая вентиляторов (`FanCurveChart`: piecewise-linear
area-силуэт из 44 колонок + точки) построены из Rectangle-примитивов.
Fluent ComboBox/SpinBox в light-схеме оффскрин-рендера ненадёжны — основные
потоки используют собственные контролы (`ChoiceButton`, `NumberStepper`,
`OptionStepper`); fluent остаётся стилем std-widgets (контракт build.rs).

### Разделы (страницы)

- **Главная** (`sections/dashboard.slint`): карточка устройства (нейтральный
  глиф, DMI-имя/плата, батарея % + статус + здоровье), режим производительности
  (3 доказанных профиля — «Ручного» режима в backend нет, карточка не
  рисуется), телеметрия (CPU/GPU температура, потребление AC/dGPU), вентиляторы
  (RPM + read-only кривая + «Настроить» → Охлаждение), ограничение заряда
  (слайдер 20–100) и подсветка клавиатуры (только при `keyboard-state-ready`).
- **Производительность**: только профили платформы; GPU-режимы живут в Графике.
- **Питание**: слайдер порога заряда, аккумулятор (заряд/состояние/здоровье/
  циклы/адаптер), потребление (AC/dGPU).
- **Охлаждение**: `FanCurveEditor` (CPU/GPU, профиль BIOS, кривая PWM/°C,
  температурные степперы) + write-safety статус; fan writes остаются
  hard-blocked.
- **Графика**: GPU-режимы Eco/Standard/Ultimate (+Optimized только при
  наличии в `available-gpu-mask`) с queued/reboot-семантикой; read-only
  состояние dGPU (питание/MUX/доступ).
- **Подсветка**: уровни клавиатуры (request-only, гейт `keyboard-control-ready`),
  Aura effect/speed как read-only наблюдение (запись Static RGB — отдельный
  непромоушен-гейт).
- **Экран**: частота обновления (Авто/60/120, request-surface, production
  read-only), Panel Overdrive (`RequestToggleRow`), честный статус отсутствия
  владельца мутации display.
- **Система**: устройство/BIOS (privacy-safe DMI: vendor/product/board/BIOS,
  без serial/UUID), звук включения (read-only), параметры ASUS (черновик:
  LED/clamshell/ASPM/standby, iGPU, гибернация, P/E-ядра, M1–M5 — те же
  request/Apply-семантики), диагностика (Refresh/Copy/Export).
- **Настройки**: тема, autostart, запуск свёрнутым, положение окна, кнопка
  закрытия, выход.
- **О программе**: имя, версия, честное описание scope.

### Палитра (норматив)

Светлая — базовая (reference): off-white chrome, белые карточки, синий акцент.
Тёмная — зеркальная спокойная. Токены семантические, оба набора в `ui/themes/`.

| Токен | Light | Dark |
|---|---|---|
| window-background | #F7F8FA | #17181C |
| titlebar-background | #FAFAFB | #1B1C21 |
| surface-default (карточки) | #FFFFFF | #232529 |
| surface-hover | #F3F4F7 | #2A2D33 |
| surface-pressed | #ECEDF1 | #1E2024 |
| surface-selected / accent-soft | #EAF2FF | #1A2840 |
| surface-disabled | #F2F3F5 | #202226 |
| border-default | #E6E8EC | #33363D |
| border-strong | #CFD3DA | #464A53 |
| text-primary | #171717 | #F1F2F4 |
| text-secondary | #6B7280 | #9BA1AC |
| text-disabled | #9CA3AF | #6A6F79 |
| text-on-accent | #FFFFFF | #FFFFFF |
| accent-default | #2563EB | #4D8DFF |
| warning | #D97706 | #E0A24E |
| error | #DC2626 | #E35D6A |
| success | #16A34A | #46BD93 |

Контракт контраста: `check-ui-contract.py` проверяет WCAG AA для
text-primary/text-secondary против window-background (обе темы ≥ 4.5:1).

### Типографика

Системный sans-serif (fontdb). Page title 22/650; device title 21/650; card
heading 14/600 (`SectionTitle`); primary metric 20–22/650; body 13; secondary
11.5–12.5. Без лишнего bold.

### Состояния и честность

- selected = `accent-soft` фон + `accent-default` рамка/текст (без заливки
  акцентом, без свечения); hover = `surface-hover` + `border-strong`;
  pressed = `surface-pressed`; disabled = `surface-disabled` +
  `text-disabled`; фокус не убран.
- Мутации — request-only: клик отправляет команду воркеру, UI подтверждается
  только авторитетным read-back (`Accepted != Applied`).
- Загрузка/отсутствие данных = «—» (никаких фиктивных 0).
- Ошибки — inline `LocalStatus`/статус-строки; модалки только для
  reboot/logout-операций (`PreviewDialogWindow`).

### Скриншоты

`cargo run -p orbis-ui --example ui_snapshot -- <section> [path] [dark|light]`
(sections: dashboard, performance, power, cooling, graphics, backlight,
display, system, settings, about, dialog) и
`cargo run -p orbis-ui --features ui-review -- --screenshot <path>
--ui-section <s>` (`--ui-state default|pending|disabled|error`).

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
