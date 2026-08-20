# UI Design Guidelines

> Роль: **CURRENT UI DESIGN CONTRACT**.
> Обновлено: 2026-08-20.
>
> G-Helper используется как reference для информационной плотности и понятной
> группировки laptop controls. Orbis не копирует его assets, branding или Windows
> widget styling; runtime truth и safety contracts Orbis имеют приоритет.

## Product rule

Главный экран показывает только то, что пользователь может осмысленно прочитать
или использовать в текущей production composition.

Не показывать на primary surface control только потому, что существует mock,
preview state, provider trait или будущий backend. Неподключённая функция либо
скрыта, либо явно помещена в dedicated development/preview surface.

`Unsupported`, `Unavailable`, `ReadOnly`, `Loading`, `Pending` и `Error` должны
выглядеть различимо. Disabled control не должен выглядеть selected/applied.

## Main window hierarchy

Порядок scan path:

1. Performance mode + CPU telemetry.
2. GPU mode + GPU telemetry.
3. Battery charge limit + battery/power telemetry.
4. Secondary navigation: Preferences and Quit.

Fans + Power остаётся рядом с Performance, потому что это прямое продолжение
thermal/performance task.

Не держать на main window:

- visual-only display/lighting previews;
- simulated updater actions;
- automation rules без reconciliation runtime;
- diagnostic/export actions без lifecycle wiring;
- generic advanced controls без production capability/backend.

## Geometry

Target main window: около `452 × 420` logical px.

Baseline tokens:

- outer padding: `12px`;
- section gap: `10px`;
- local gap: `4–8px`;
- primary mode card height: `54px`;
- standard button/control height: `30px`;
- card/button radius: `7–8px`;
- border: `1px`, selected emphasis `1.5px`;
- section title: `11px`, primary text generally `9–11px` in compact controls.

Compact does not mean tiny click targets: interactive rows/buttons keep roughly
30px height and use surrounding layout whitespace as part of the visual target.

## Color system

Color is semantic emphasis, not decoration. Large areas stay neutral; mode colors
appear mainly as dots, outlines and curve strokes.

### Dark

| Token | Value |
|---|---|
| Window | `#17191C` |
| Surface | `#202328` |
| Hover | `#272B31` |
| Selected surface | `#242A30` |
| Border | `#343940` |
| Strong border | `#4A515B` |
| Primary text | `#F4F6F8` |
| Secondary text | `#AAB2BD` |
| Accent | `#4DA3FF` |

### Light

| Token | Value |
|---|---|
| Window | `#F4F5F7` |
| Surface | `#FFFFFF` |
| Hover | `#F0F3F6` |
| Selected surface | `#EDF4FC` |
| Border | `#D5DBE2` |
| Strong border | `#AEB7C2` |
| Primary text | `#171A1E` |
| Secondary text | `#5F6873` |
| Accent | `#2678E8` |

Mode accents remain concept-specific but should not fill entire cards by default.
Selected state is normally neutral selected surface + accent border/dot.

## Interaction rules

- Hover may change neutral surface/border, not semantic state.
- Selected uses accent outline and modest background shift.
- Pending is not Applied; keep a visible pending label/state.
- Error should be local to the affected section where possible.
- Slider drag is preview; hardware mutation commits only at the established
  callback boundary on release.
- Fan write safety blocks remain visible and authoritative; redesign must never
  make a blocked mutation look enabled.

## Secondary windows

Preferences should expose backed settings, not a catalogue of future options.
Fans should prioritize the authoritative curve and write-safety state. Advanced,
automation, updater and diagnostics surfaces enter normal navigation only when
their runtime lifecycle is real enough to provide truthful behavior.

## Validation

UI review requires two separate checks:

1. static contract review: callbacks, disabled/read-only semantics and backend
   boundaries did not change;
2. executable visual validation: Slint compile plus dark/light screenshots at the
   exact revision.

While executable CI #106 is unavailable, static review may establish only the
first item. It must not be promoted to `TESTED` or packaged visual evidence.
