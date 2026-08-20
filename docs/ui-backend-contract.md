# UI ↔ Backend Wiring Contract

> Статус: frontend contract для текущего production UI.
> Обновлено: 2026-08-20.
>
> Этот документ описывает оставшуюся интеграцию после UI redesign. UI не должен
> менять hardware-looking authoritative state до подтверждения backend/read-back.

## Общие правила

1. Backend сначала публикует наблюдаемое значение и availability/readiness.
2. UI включает control только при доказанной availability/policy capability.
3. Пользовательское действие вызывает request callback.
4. Backend выполняет mutation вне Slint callback thread, где это требуется.
5. После mutation backend делает authoritative read-back.
6. Только read-back обновляет выбранное/применённое состояние.
7. Pending/Error/Unavailable не маскируются optimistic local state.

Локальный draft разрешён только для конфигурационных окон с явным `Apply`.

## AppWindow

### Уже подключено

- Performance: `ui-state.perf-*` → `perf-clicked(int)`.
- GPU mode: `ui-state.gpu-*` → `gpu-clicked(int)`.
- Battery charge limit: `ui-state.charge-limit-*` → `charge-changed(float)`.
- Fans window/navigation and existing fan callbacks.
- Preferences/Extra/Automation/Diagnostics/Updates window opening callbacks.

### Display quick control

Backend publishes:

- `display-control-ready: bool`
- `display-mode: int`
  - `0` Auto
  - `1` 60 Hz
  - `2` 120 Hz
- `display-status: string`

Backend handles:

- `display-mode-requested(int)`

Do not set `display-mode` from the click itself. Update it only from observed
state after the display provider confirms the transition.

### Keyboard quick control

Backend publishes:

- `keyboard-control-ready: bool`
- `keyboard-brightness: int` (`0..3`)
- `keyboard-status: string`

Backend handles:

- `keyboard-brightness-requested(int)`

The current keyboard probe/read capability should drive readiness independently
from write capability. A read-only device may expose status without enabling the
buttons.

## PreferencesWindow

Theme remains wired through `theme-changed(bool)` and is persisted immediately.

Backend/lifecycle publishes:

- `startup`, `startup-enabled`, `startup-status`
- `start-minimized`, `start-minimized-enabled`
- `remember-position`, `remember-position-enabled`
- `close-action`, `close-action-enabled`

Backend handles:

- `startup-changed(bool)`
- `start-minimized-changed(bool)`
- `remember-position-changed(bool)`
- `close-action-changed(int)`

`close-action` values:

- `0` Quit
- `1` Hide to tray

Window-position support must remain disabled where the active window system
cannot satisfy the contract reliably.

## FansWindow

Backend/policy publishes in addition to `UiState`:

- `mutation-safety-blocked`
- `factory-reset-available`
- `curve-enabled-known`
- `curve-enabled`
- `policy-status`

Existing callbacks remain the mutation boundary:

- `fan-changed(int)`
- `fan-profile-changed(int)`
- `fan-temp-point-changed(int, int)`
- `fan-pwm-point-changed(int, int)`
- `fan-apply-clicked(bool reset_defaults)`

`curve-enabled` must come from authoritative asusd/Hardware1 state. Do not infer
it from curve point presence. Keep `mutation-safety-blocked=true` until enabled
state preservation/read-back and factory-default restoration are validated.

## ExtraWindow

This window is a **draft editor**. Backend loads authoritative values into the
window, user edits the local draft, and one explicit Apply commits the document.

Backend publishes:

- `backend-ready`
- `applying`
- `status`
- hotkey action indexes `m1-action..m5-action`
- keyboard `keyboard-brightness`, `keyboard-effect`, `keyboard-speed`
- platform booleans `boot-sound`, `status-led`, `panel-overdrive`,
  `auto-clamshell`, `disable-aspm`, `disable-standby-networking`
- `igpu-memory`, `hibernate-after`, `p-cores`, `e-cores`

Backend handles:

- `reload-requested` — discard/reload draft from authoritative state
- `apply-requested` — read the complete current window draft and submit one
  validated application transaction

On failure, retain the draft for correction and publish an error in `status`;
do not replace authoritative state with the draft.

## AutomationWindow

All visible rule values are backend-owned inputs.

Backend publishes:

- `backend-ready`, `saving`, `status`
- `enabled`
- AC/Battery performance, GPU, display and lighting indexes
- `on-resume`, `on-ac-change`, `reconcile-only`, `notify-transitions`

Backend handles request callbacks with matching names:

- `enabled-requested(bool)`
- `ac-profile-requested(int)` / `battery-profile-requested(int)`
- `ac-gpu-requested(int)` / `battery-gpu-requested(int)`
- `ac-display-requested(int)` / `battery-display-requested(int)`
- `ac-lighting-requested(int)` / `battery-lighting-requested(int)`
- `on-resume-requested(bool)`
- `on-ac-change-requested(bool)`
- `reconcile-only-requested(bool)`
- `notify-transitions-requested(bool)`
- `reset-requested`
- `save-requested`

The frontend does not claim persistence until the backend republishes the saved
rule set/status.

## DiagnosticsWindow

Read-only inputs already define the presentation model:

- kernel/platform/version/build/system
- capabilities/services/GPU/telemetry/display text
- snapshot metadata and local status

Lifecycle/action inputs:

- `refresh-enabled`, `refresh-pending`
- `copy-enabled`, `logs-enabled`, `export-enabled`

Backend handles:

- `refresh-requested`
- `copy-summary-requested`
- `open-logs-requested`
- `export-report-requested`

No diagnostics callback may mutate hardware state.

## UpdatesWindow

Backend publishes:

- `backend-ready`
- `checking`, `installing`
- installed `version`
- `channel`, `channel-enabled`
- `update-available`, `latest-version`
- `release-notes`, `status`

Backend handles:

- `channel-requested(int)`
- `check-requested`
- `install-requested`

When `backend-ready=false`, UI deliberately renders update state as Unknown —
never `Up to date`.

## Action dialog

`PreviewDialogWindow` is retained as a compatibility type name only. It is a
production request dialog.

Inputs:

- `kind`: `0` reboot, `1` logout, `2` failure, `3` generic confirmation
- `action-label`
- `action-enabled`

Callbacks:

- `dismiss-clicked`
- `confirm-clicked`

The dialog never performs reboot/logout/mutation directly.

## Theme synchronization

Every top-level window using Slint standard widgets must instantiate
`ThemeBridge`. It maps Orbis `ThemeState.mode` to the standard-widget
`Palette.color-scheme`, so ComboBox/SpinBox/ScrollView follow the in-app theme
instead of independently following the desktop theme.

## Validation before enabling a backend

For each newly wired surface:

1. run `scripts/check-ui-contract.py`;
2. run `scripts/verify task` when the Rust/Nix toolchain is available;
3. capture dark/light screenshots for affected windows;
4. verify Loading → Ready, Unavailable, ReadOnly, Pending and Error states;
5. for mutation paths, verify request → backend → read-back with no optimistic
   selected/applied state between request and confirmation.
