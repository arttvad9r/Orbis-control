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

Для Quick Controls отдельно различаются **read readiness** (`*-state-ready`) и
**mutation readiness** (`*-control-ready`). Read-only backend может показывать
реальное наблюдаемое значение при `state-ready=true`, но control остаётся
некликабельным, пока write capability не доказана.

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
- Display и Keyboard Quick Controls: authoritative observed state подключён;
  mutation остаётся fail-closed по текущим backend/product contracts.

### Display quick control

#### Подключено

Backend publishes:

- `display-state-ready: bool`
- `display-mode: int`
  - `-1` — current mode не соответствует быстрому preset или target неоднозначен
  - `1` — observed refresh около 60 Hz
  - `2` — observed refresh около 120 Hz
- `display-status: string`
- `display-control-ready: false` в текущем production composition

Observed state читается через существующий read-only
`WaylandDisplayOutputProvider<WaylandCompositorOutputSource>` из compositor
`wl_output` current mode. Refresh хранится lossless в mHz; UI только группирует
59–61 Hz и 118–122 Hz для подсветки соответствующего preset, не переписывая
source state.

Если compositor показывает несколько outputs, backend не угадывает внутреннюю
панель: `display-mode=-1`, status сообщает `target ambiguous`. Пустой output list
также не превращается в fake internal display.

`display-mode-requested(int)` зарегистрирован, но текущий handler fail-closed и
не выполняет mutation. Существующий production `DisplayOutputProvider`
намеренно read-only и не содержит modeset API. Значение `0` (`Auto`) остаётся
зарезервированным frontend preset для будущего product-level DisplayRefresh
contract и не выводится из `wl_output` observation.

Initial read выполняется при запуске приложения; затем observed state
переопрашивается не чаще чем раз в 10 секунд на telemetry lifecycle events.
Каждый provider read ограничен 2-секундным timeout.

#### Для включения write

Нужен отдельный typed DisplayRefresh mutation owner с однозначным target
selection, capability evidence и authoritative compositor read-back. Только
после этого допустимо выставлять `display-control-ready=true`.

### Keyboard quick control

#### Подключено

Backend publishes:

- `keyboard-state-ready: bool`
- `keyboard-brightness: int` — фактический raw hardware level
- `keyboard-status: string` — включает observed `current/max`
- `keyboard-control-ready` выводится из operation-level `Supported` evidence

Read path использует существующий `AsusKeyboardBacklightProvider`, который
читает `/sys/class/leds/asus::kbd_backlight/{brightness,max_brightness}`. Max
level определяется hardware и не hardcode-ится как `3`; если устройство
сообщает, например, `4`, UI сохраняет это в status и не подделывает выбранный
preset.

В `orbis-hardwared` существует typed keyboard mutation implementation с
validation, отдельным polkit action и fresh read-back. Текущая FA707NV
production-композиция использует этот backend, точные sandbox paths и
capability-specific polkit. `keyboard-brightness-requested(int)` проходит только
при `Supported`; live `3→0→3` read-back подтверждён.

Initial/periodic read lifecycle и 2-секундный timeout совпадают с Display.

#### Для включения write

Сначала должно быть принято отдельное product-policy решение и capability
registry должен получить effective write evidence от production Hardware1.
После mutation UI обязан принять только fresh read-back observed level; наличие
typed writer implementation само по себе не является разрешением включить
кнопки.

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
  реальный window-state restore/save lifecycle не проверен для active window system.
  Hardened `window_state` store уже существует, но Slint window positioning может
  быть unavailable на Wayland, поэтому наличие store само по себе не является
  runtime capability evidence.
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

### Подключено

`refresh-requested` подключён к существующему production
`DiagnosticsRuntime` → `DiagnosticsUiDto` → `DiagnosticsWindowModel` pipeline.
Refresh выполняется на Tokio runtime, а Slint получает только готовую immutable
presentation model.

Один snapshot выполняет только read-only observation:

- application/build metadata;
- kernel/session/system metadata;
- privacy-safe DMI hardware identity;
- D-Bus service presence;
- canonical capability snapshot;
- session GPU primitive observations;
- sysfs telemetry reads;
- Wayland display/output observation.

Diagnostics использует clone уже открытых session/system bus connections и
initial immutable capability snapshot из `ApplicationRuntime`. Каждый успешный
worker `RegistryChange` заменяет snapshot у Diagnostics через
`replace_capabilities`; само Diagnostics окно capability probes не запускает и
registry не редактирует.

Во время refresh `refresh-pending=true`; повторный request блокируется. После
готового snapshot публикуются kernel/platform/version/build/system/capabilities/
services/GPU/telemetry/display поля и Refresh снова включается.

`export-report-requested` теперь использует только последний frozen
`DiagnosticsUiDto` и существующий privacy-bounded `diagnostics_export::report_json`.
Экспорт выполняется через blocking worker, не на Slint callback thread. Файл:

- создаётся только под fail-closed `XDG_STATE_HOME/orbis-control/diagnostics`
  (с documented home fallback через `state_dir_checked()`);
- получает уникальное имя `orbis-diagnostics-<snapshot-unix-seconds>[-N].json`;
- создаётся через `create_new`, поэтому существующий report не перезаписывается;
- на Unix создаётся с mode `0600`;
- flush/sync выполняются до публикации success в UI;
- использует allowlisted JSON projection без serial/UUID/asset-tag/journal/
  environment/raw telemetry surfaces.

`export-enabled` становится `true` только после первого frozen snapshot и
валидного fail-closed XDG state path. Refresh и Export взаимно исключаются. При
ошибке Export снова блокируется до следующего Refresh, чтобы повторная попытка
не выглядела как доказанная availability после неуспешной записи.

### Ещё не подключено

- `copy-summary-requested`
- `open-logs-requested`

`copy-enabled` и `logs-enabled` остаются `false`. Clipboard API Slint доступен
через platform abstraction, но текущая application composition не владеет
public platform handle для стабильного host clipboard action; shell helper
fallback не вводится. Logs также не открываются через journal/shell из
Diagnostics bridge.

Ни один Diagnostics callback не должен мутировать hardware.

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
