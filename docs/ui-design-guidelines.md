# UI Design Guidelines

> Роль: **CURRENT UI DESIGN CONTRACT**.
> Обновлено: 2026-08-20.
>
> G-Helper используется как reference для информационной плотности и понятной
> группировки laptop controls. Orbis не копирует его assets, branding или Windows
> widget styling; runtime truth и safety contracts Orbis имеют приоритет.

## Product rule

Frontend должен быть завершён независимо от готовности backend. Для плановой
функции допустимы только два production-состояния:

1. backend публикует authoritative value/capability и UI разрешает действие;
2. backend отсутствует/не доказал capability — control остаётся disabled/read-only.

Нельзя имитировать успех локальными строками вроде `applied locally`, `staged`,
`simulated`, менять hardware-looking state без backend read-back или показывать
preview как применённую настройку.

`Unsupported`, `Unavailable`, `ReadOnly`, `Loading`, `Pending` и `Error` должны
выглядеть различимо. Disabled control не должен выглядеть selected/applied.

## Main window hierarchy

Порядок scan path:

1. Performance mode + CPU telemetry.
2. GPU mode + GPU telemetry.
3. Quick Controls: Display + Keyboard Backlight.
4. Battery charge limit + battery/power telemetry.
5. Footer: More, Preferences, Quit.

Fans + Power остаётся рядом с Performance, потому что это прямое продолжение
thermal/performance task.

Quick Controls всегда используют authoritative input properties. До backend
wiring они видимы, но disabled; click emits request callback и не меняет
presented applied state сам по себе.

`More` открывает законченные frontend surfaces: Extra Controls, Automation,
Diagnostics и Updates. До подключения соответствующего backend их controls
явно disabled/read-only.

## Geometry

Target main window: `452 × 526` logical px.

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
Secondary-window height budgets are guarded by `scripts/check-ui-contract.py` so
fixed layouts are not silently compressed below their reviewed content budget.

## Color system

Color is semantic emphasis, not decoration. Large areas stay neutral; mode colors
appear mainly as dots, outlines and curve strokes. Unselected mode markers are
neutral; the concept color becomes prominent only for selected/pending state.

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

Standard Slint widgets are compiled with the cross-platform `fluent` style and
follow `ThemeState` through `ThemeBridge`. This avoids Linux `native` selecting a
Qt style whose palette may not follow runtime dark/light overrides.

## Interaction rules

- Hover may change neutral surface/border, not semantic state.
- Selected uses accent outline and modest background shift.
- Pending is not Applied; keep a visible pending label/state.
- Error should be local to the affected section where possible.
- Slider drag is preview of a draft value only; hardware mutation commits at an
  explicit callback boundary and authoritative read-back remains source of truth.
- Backend-owned toggles use `RequestToggleRow`: click emits the requested value
  but never flips `checked` locally. The owner publishes the confirmed value.
- Fan write safety blocks remain visible and authoritative; redesign must never
  make a blocked mutation look enabled.
- Settings that naturally form a configuration document may use a local draft,
  but must expose explicit `Reload`/`Apply` boundaries. `Apply` is a request, not
  proof of success.
- Draft-window action footers stay outside scrollable content so Apply/Reload/Save
  remain visible at normal window sizes.

## Secondary windows

### Preferences

Frontend contract includes Theme, Run on Startup, Start Minimized, Remember
Window Position and Close Action. Each lifecycle setting has its own enabled
state and request callback. Unsupported window-system behavior stays disabled.
Backend-owned toggles and close-action choices do not optimistically change their
presented state before the owner republishes it.

### Fans + Power

Prioritize CPU/GPU selection, BIOS profile, one authoritative curve, temperature
points and write-safety status. The frontend includes explicit presentation for
unknown/enabled/disabled custom-curve state; it must not infer active state from
curve points alone. The safety footer and mutation actions remain visible below
the editor.

### Extra Controls

This is an editable draft surface for bindings, keyboard/backlight, platform and
power/CPU settings. Backend loads the draft, `Reload` requests authoritative
state again, `Apply` is the single commit boundary. Hotkeys use a compact
multi-column layout and Apply/Reload remain fixed outside the scroll view.

### Automation

All rule values are backend-owned. Named policy rows distinguish Performance,
GPU, Display and Lighting in both AC/Battery columns. Toggle interactions are
request-only; Save/Reset are explicit backend actions. No local fake persistence.

### Diagnostics

Read-only surface with refresh/copy/log/export callbacks and independent enabled
states. No mutation API. Capability/service information remains the dominant
content area; subsystem cards are secondary.

### Updates

Backend owns channel, latest version, update availability, checking/installing
state and release notes. Check/Install only emit callbacks.

## Validation

Two layers are required:

1. `scripts/check-ui-contract.py` — standard-library static test for delimiter
   balance, relative imports, production callback markers, fake-success phrases,
   authoritative-toggle usage, reviewed window geometry budgets and theme
   token parity/contrast;
2. executable validation — Slint/Rust compile plus dark/light screenshots at the
   exact revision when a Rust/Slint toolchain is available.

The static test is intentionally runnable in minimal containers and is part of
the production UI contract. It does not replace compilation or live interaction
checks on Wayland/X11.
