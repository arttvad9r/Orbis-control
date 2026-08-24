# UI Reference — Orbis Control

> Дата: 2026-08-06. Normative reconciliation: 2026-08-24 (#128).
> Источник измерений: исходники G-Helper `e439349f` (app/Settings.Designer.cs,
> app/Fans.Designer.cs, app/Extra.Designer.cs, app/UI/RForm.cs, app/Settings.cs).
> Методика: WinForms `AutoScaleDimensions = 192×192` → нормализованные 96-DPI
> логические пиксели (деление на 2), далее сверка со скриншотами (запланировано).

## 0. Normative Orbis UI specification (2026-08-24)

Orbis Control **не является клоном G-Helper**. Разделы 1–9 ниже — исторический
источник измерений, из которого вырос первоначальный дизайн; нормативом
является эта секция. Компилируемый UI живёт в `ui/audited/` (+ `ui/components/`,
`ui/themes/`); `ui/app-window.slint` — легаси и не компилируется.

### Дизайн-принципы

- Одно компактное окно быстрых управлений: Performance → GPU Mode →
  Quick Controls → Battery Charge Limit → футер; вторичные поверхности —
  отдельные окна (`ui/audited/*-window.slint`).
- Состояния честные: disabled/pending/unavailable выводятся из typed evidence,
  никогда не симулируются.
- Тёмная палитра — «cool charcoal» система Orbis (ниже); light-вариант в
  `ui/themes/light.slint`.

### Нормативная геометрия (реализация = спецификация)

| Параметр | Значение |
|---|---|
| Главное окно | 452×526 logical px, фиксированное |
| Padding окна / spacing секций | 12 / 10 |
| Mode-переключатель | `SegmentedTrack` 48px: трек radius 8, сегменты radius 6, gap 3 |
| Секционная карточка | `SectionCard` radius 8, border 1 |
| Заголовок секции | `SectionTitle` 22px, 11px semibold + detail справа |
| Кнопки действий | `ActionButton` высота 30, radius 8 |
| Статусная строка | `LocalStatus` высота 26, radius 8 |
| Quick Controls | `ChoiceChip` radius 8, ряды 32px |

### Нормативная палитра (dark; light — зеркально в `light.slint`)

```json
{
  "window.background": "#17191C",
  "titlebar.background": "#17191C",
  "surface.default": "#202328",
  "surface.hover": "#272B31",
  "surface.pressed": "#1B1E22",
  "surface.selected": "#242A30",
  "surface.disabled": "#1D2024",
  "border.default": "#343940",
  "border.strong": "#4A515B",
  "text.primary": "#F4F6F8",
  "text.secondary": "#AAB2BD",
  "text.disabled": "#68717D",
  "text.on-accent": "#FFFFFF",
  "accent.default": "#4DA3FF",
  "warning": "#E4A853",
  "error": "#F06B78",
  "success": "#55C79A",
  "mode.silent": "#5CC8A5",
  "mode.balanced": "#4DA3FF",
  "mode.turbo": "#FF6B78",
  "mode.eco": "#75C77A",
  "mode.standard": "#4DA3FF",
  "mode.ultimate": "#E7AC57",
  "mode.optimized": "#6AB6FF"
}
```

Выбранный режим подсвечивается заливкой акцента режима (единственное крупное
цветовое пятно в окне); акценты нигде больше не заливают поверхности.

### Структурные решения (отличия от G-Helper — сознательные)

- Нативный titlebar ОС (Wayland CSD), не кастомная RForm-панель.
- GPU Mode — один ряд из четырёх сегментов, а не 2 ряда с пустой колонкой.
- Высота окна фиксированная: все секции видны всегда, неподдерживаемые
  состояния показываются честными статусными строками, а не скрытием секций.
- Quick Controls (Display/Keyboard) — секция Orbis; контролы disabled без
  typed write evidence.
- «Fans + Power» — неотключаемый сегмент-действие (открывает FansWindow),
  никогда не отображает selected.

### Visual regression

Скриншот-проверки (`--features ui-review`, `--screenshot`, сценарии
`--ui-state default/pending/disabled`) остаются механизмом верификации
(§10); базовые сценарии актуальны, эталонные значения пересчитываются от
нормативной геометрии этой секции, а не от G-Helper-замеров.

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
