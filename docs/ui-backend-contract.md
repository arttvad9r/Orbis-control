# UI ↔ Backend Wiring Contract

> Статус: frontend contract для текущего production UI.
> Обновлено: 2026-08-20.
>
> Этот документ описывает оставшуюся интеграцию после UI redesign. UI не должен
> менять hardware-looking authoritative state до подтверждения backend/read-back.

## Общие правила

1. Backend сначала регистрирует handlers для всех callback'ов surface.
2. Только после этого backend публикует readiness/availability и наблюдаемое значение.
3. UI включает control только при доказанной availability/policy capability.
4. Пользовательское действие вызывает request callback.
5. Backend выполняет mutation вне Slint callback thread, где это требуется.
6. После mutation backend делает authoritative read-back.
7. Только read-back обновляет выбранное/применённое hardware-looking состояние.
8. Pending/Error/Unavailable не маскируются optimistic local state.

`*-ready`, `*-enabled` и `backend-ready` являются не декоративными флагами, а
интеграционным барьером. Их нельзя выставлять `true`, если соответствующий
request handler ещё не зарегистрирован или backend не может вернуть
authoritative state после операции.

Локальный draft разрешён только для конфигурационных окон с явным `Apply`/`Save`.
Draft controls могут меняться локально, но это не является hardware/system
success. Backend-owned toggles используют request-only presentation и не меняют
`checked` сами.

## AppWindow

### Уже подключено

- Performance: `ui-state.perf-*` → `perf-clicked(int)` → worker/provider path.
- Battery charge limit: `ui-state.charge-limit-*` → `charge-changed(float)` → worker/provider path.
- GPU callback/worker path существует, но production mutation capability остаётся
  product/policy disabled; UI должен оставаться read-only/disabled по evidence.
- Fans window navigation, fan selection/profile refresh and existing fan callbacks.
- Theme and secondary-window navigation.

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

The keyboard read capability and write capability must remain distinct. A
read-only device may expose status without enabling brightness buttons.

## PreferencesWindow

### Подключено

- Theme: `theme-changed(bool)` меняет runtime theme и атомарно сохраняет только
  appearance field в safe `preferences.toml`.
- Run on startup: `startup/startup-enabled/startup-status` читаются из
  принадлежащего Orbis XDG autostart entry; `startup-changed(bool)` меняет только
  этот user-owned `.desktop` и затем делает authoritative read-back. Изменённая
  или чужая запись никогда не показывается как Enabled; включение может
  восстановить canonical owned entry.
- Start Minimized: `start-minimized` читается из production preferences store,
  `start-minimized-changed(bool)` сохраняет только это safe lifecycle field, а
  runtime уже применяет его при следующем запуске до первого показа окна.
- Если preferences load вернул preserved warning, editing остаётся disabled и
  существующий проблемный файл не перезаписывается.

### Ещё не подключено

- `remember-position` / `remember-position-changed(bool)` остаются disabled, пока
  реальный window-state restore/save lifecycle не проверен для активного window system.
- `close-action` / `close-action-changed(int)` остаются disabled, пока нет
  production tray/reopen lifecycle. Persisted `HideToTray` без работающего tray
  не считается допустимой реализацией.

`close-action` UI values после подключения runtime lifecycle:

- `0` Quit
- `1` Hide to tray

Все backend-owned controls остаются request-only: клик не меняет presented
setting до accepted persistence/read-back.

## FansWindow

Backend publishes through `UiState`:

- fan state/readability/writability/error/dirty
- selected fan/profile
- 8 temperature points
- 8 PWM points

Additional presentation evidence:

- `mutation-safety-blocked`
- `factory-reset-available`
- `curve-enabled-known`
- `curve-enabled`
- `policy-status`

Frontend emits:

- `fan-changed(int)`
- `fan-profile-changed(int)`
- `fan-temp-point-changed(int, int)`
- `fan-pwm-point-changed(int, int)`
- `fan-apply-clicked(bool)`

Current production keeps mutation safety-blocked. Read/profile refresh is useful
and may remain available. Do not enable Apply/Factory Defaults until the known
fan mutation safety issues are resolved and authoritative enabled-state evidence
is carried through the model.

## ExtraWindow

This surface is intentionally a local **draft editor** after `backend-ready=true`.
The backend loads authoritative values into the in-out draft properties, then:

- `reload-requested` discards/reloads from backend;
- `apply-requested` is the only commit boundary;
- `applying=true` freezes controls until completion;
- `status` describes success/failure/pending without claiming success before the
  backend confirms it.

Backend must validate every advanced setting independently; `backend-ready`
does not imply every individual hardware mutation is permitted. Unsupported
advanced settings should either be omitted in a future typed capability model or
remain disabled through per-feature evidence before writes are introduced.

## AutomationWindow

Backend publishes the policy document and `backend-ready`. UI policy rows and
request-only toggles emit draft-update requests:

- `enabled-requested(bool)`
- AC/Battery Performance/GPU/Display/Lighting request callbacks
- resume/AC-change/reconcile/notification request callbacks

Persistence boundaries:

- `reset-requested`
- `save-requested`
- `saving=true` freezes edits

The backend owns the draft and republishes accepted values. Save persists policy;
it is not proof that every hardware target was immediately reconciled.

## DiagnosticsWindow

Read-only backend publishes kernel/platform/version/build/capability/service/GPU/
telemetry/display text and enabled/pending flags.

Frontend emits:

- `refresh-requested`
- `copy-summary-requested`
- `open-logs-requested`
- `export-report-requested`

No diagnostics callback may mutate hardware.

## UpdatesWindow

Backend publishes:

- `backend-ready`
- installed/current channel state
- `checking`, `installing`
- `update-available`
- `latest-version`, `release-notes`, `status`

Frontend emits:

- `channel-requested(int)`
- `check-requested`
- `install-requested`

`backend-ready=false` renders Unknown/Unavailable and disables network/package
actions. The UI never infers `Up to date` without an authoritative backend check.

## Action dialog

`PreviewDialogWindow` retains its compatibility name only. The production
contract is request-only:

- `dismiss-clicked`
- `confirm-clicked`
- `kind`, `action-label`, `action-enabled`

Reboot/logout/confirmation are host/runtime responsibilities. The Slint dialog
does not execute system actions itself.

## Widget/theme integration

The build pins Slint standard widgets to the cross-platform `fluent` style.
`ThemeBridge` then controls `std-widgets` `Palette.color-scheme` from Orbis
`ThemeState`, keeping ComboBox/SpinBox/ScrollView aligned with dark/light themes.

## Backend integration completion checklist

For each surface before setting readiness true:

1. register all request handlers;
2. perform initial authoritative read;
3. publish state + capability/readiness;
4. disable the surface while a non-repeatable mutation has unknown outcome;
5. read back after mutation/persistence;
6. publish confirmed state or explicit error;
7. test unavailable/read-only/pending/error paths as well as success.

When these steps are complete, no UI redesign should be required; backend work is
property publication, callback handling, lifecycle/persistence, and evidence.
