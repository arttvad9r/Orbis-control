# Orbis Control — user preferences and config persistence design

**Audit date:** 2026-08-18  
**Production source:** `chatgpt/production-hardening-20260818` @ `ea286e32d7e86f7965ba101865d2feee5dcf0342`  
**UI source:** `agent/light-theme-toggle` @ `4a63c6b8f36688a8b81fc2f87a94eb2216d8ee56`  
**Research branch:** `agent/preferences-persistence-design`  
**Scope:** research/documentation only. No Rust, Slint, Nix, CI, service, D-Bus, polkit, sysfs, or hardware changes are made by this branch.

The hardening branch moved during this audit from the previously observed `a745f559...` to `ea286e32...`. It was fetched again immediately before this document commit; the audited `orbis-config` files were re-read at the new HEAD. `crates/orbis-config/src/store.rs`, `lib.rs`, `paths.rs`, and the relevant current packaging files retain the persistence behavior described below.

---

# 1. Executive summary

Orbis already has a useful configuration foundation, but it should not be treated as the final production preferences design.

The existing `orbis-config` crate provides:

- XDG-shaped paths;
- TOML serialization/deserialization;
- a version field (`config_version = 1`);
- typed top-level config structs;
- limited schema validation;
- missing-file defaults;
- temp-file + rename writes;
- a backup helper intended for migrations;
- unit tests for basic round-trip/path/validation behavior.

However, the current `AppConfig` combines four semantically different classes in one file:

1. user interface preferences (`UiConfig`);
2. automation policy (`AutomationConfig`);
3. desired hardware state (`BatteryConfig::charge_limit`);
4. experimental/security-sensitive feature gates (`ExperimentalConfig`).

That layout was acceptable as an early-stage model, but it is not the recommended long-term persistence boundary. A theme preference must not acquire the lifecycle or authorization semantics of a battery desired state merely because both happen to serialize as TOML. Likewise, storing an automation rule must not cause the rule to execute.

The production design should therefore split persistence by semantic ownership:

- **safe user preferences** → `$XDG_CONFIG_HOME/orbis-control/preferences.toml`;
- **automation policy only** → `$XDG_CONFIG_HOME/orbis-control/automation.toml`;
- **desired hardware state**, if Orbis intentionally supports persistent desired state later → a separate domain file such as `$XDG_CONFIG_HOME/orbis-control/desired-state.toml`;
- **runtime/UI state** that is useful across restarts but is not a portable preference → `$XDG_STATE_HOME/orbis-control/...`;
- **Run on Startup** → actual user XDG Autostart state under `$XDG_CONFIG_HOME/autostart/`, not a canonical boolean inside `preferences.toml`.

The existing monolithic `config.toml` should become a **legacy import source**, not be destructively rewritten into the new shape in one migration.

The highest-value first production slice is theme persistence. The light-theme branch already has correct process-local multi-window synchronization. Load `preferences.toml` before constructing the first top-level Slint window, initialize the process theme from it, and keep the existing Rust fan-out to all windows. This can restore Light before the first visible frame and avoid a visible Dark → Light flash.

No persistence work in this design requires hardware writes, root, polkit, or a privileged Session1 mutation API.

---

# 2. Audited `orbis-config`

## 2.1 Current storage paths

Current source:

- `crates/orbis-config/src/lib.rs`
- `crates/orbis-config/src/paths.rs`

The current config directory is conceptually:

```text
$XDG_CONFIG_HOME/orbis-control
```

with fallback:

```text
$HOME/.config/orbis-control
```

and the current file is:

```text
$XDG_CONFIG_HOME/orbis-control/config.toml
```

The crate also has helpers for:

```text
$XDG_STATE_HOME/orbis-control
$XDG_CACHE_HOME/orbis-control
```

with conventional home fallbacks.

### Current path-resolution weakness

The current implementation accepts `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, and `XDG_CACHE_HOME` as `PathBuf` values without checking that they are non-empty absolute paths. If no home directory is available, it falls back to `.`.

For production persistence this should be tightened. XDG Base Directory Specification 0.8 requires XDG paths to be absolute and says relative values should be ignored. Empty/unset `XDG_CONFIG_HOME` should fall back to `$HOME/.config`; empty/unset `XDG_STATE_HOME` should fall back to `$HOME/.local/state`.

Orbis should never silently turn “no safe configuration home can be resolved” into “write config in the current working directory”. A typed path-resolution error is safer than `.`.

## 2.2 Current schema

`CONFIG_VERSION` is currently `1`.

`AppConfig` contains:

```text
config_version
ui
  theme
  close_to_tray
  start_minimized
  remember_position
automation
  enabled
  power_event_delay_ms
  ac
    profile
    gpu_policy
    refresh_policy
  battery
    profile
    gpu_policy
    refresh_policy
battery
  charge_limit
experimental
  enabled
  undervolting
  raw_wmi
```

### `UiConfig`

Current persisted fields and defaults:

| Field | Default | Current meaning |
|---|---|---|
| `theme` | `"dark"` | UI theme, validated as `dark` or `light` |
| `close_to_tray` | `true` | binary close behavior |
| `start_minimized` | `false` | app startup presentation preference |
| `remember_position` | `true` | whether window position should be remembered |

### `AutomationConfig`

Current persisted fields include:

- `enabled = true`;
- `power_event_delay_ms = 1500`;
- AC profile = Balanced;
- AC GPU policy = Standard;
- AC refresh policy = string `"maximum"`;
- battery profile = Silent;
- battery GPU policy = Eco;
- battery refresh policy = string `"minimum"`.

These values exist in storage, but there is no production automation executor consuming them. Their presence is **not** proof of a safe executable policy engine.

### `BatteryConfig`

`charge_limit: Option<u8>` is explicitly documented as a desired user intent. Current validation accepts `20..=100`; `None` means Orbis should not manage the threshold.

This is not a UI preference. It is desired hardware state and belongs in a different persistence domain from appearance/window behavior.

### `ExperimentalConfig`

Current fields:

- `enabled`;
- `undervolting`;
- `raw_wmi`.

These are not ordinary Preferences controls. They are security/experimental feature gates and should not be silently folded into the same public preferences schema.

## 2.3 Current validation

`AppConfig::validate()` currently proves only:

1. `config_version == CONFIG_VERSION`;
2. `ui.theme` is exactly `dark` or `light`;
3. `battery.charge_limit`, when present, is in `20..=100`.

It does **not** currently validate, for example:

- automation delay bounds;
- `refresh_policy` string values;
- relationships between automation fields;
- experimental combinations;
- any future Preferences UI ranges such as UI scale or telemetry cadence.

The current schema is therefore typed, but only partially semantically validated.

## 2.4 Current defaults

Missing `config.toml` returns `AppConfig::default()`.

Important distinction: the function is named `load_or_default()`, but it only defaults when the file is missing. Parse failures, invalid schema, unsupported versions, migration errors, permission failures, and other I/O errors are returned as errors.

This is a good fail-closed property, but the production UI needs an explicit **runtime fallback policy** for corruption so application startup does not unnecessarily fail just because a cosmetic preference file is malformed.

## 2.5 Current error handling

`ConfigError` distinguishes:

- I/O;
- TOML parse;
- TOML serialization;
- unsupported format version;
- migration failure;
- schema failure.

That taxonomy is a useful base. Production preferences should add enough context to distinguish at least:

- path-resolution failure;
- missing file;
- corrupt/invalid current-version file;
- future-version file;
- migration failure;
- write/replace failure;
- recovery-backup failure.

Missing should remain normal. Corrupt and future-version files must not be treated as the same condition.

## 2.6 Current write atomicity

`save_to_dir()` currently:

1. validates the config;
2. creates the directory;
3. serializes to TOML;
4. writes a fixed `config.toml.tmp` in the same directory;
5. renames it to `config.toml`.

Same-directory rename is the right basic atomic replacement shape: readers should see either the previous path contents or the replacement path contents, not a partially overwritten final file.

However, the production design should not call this fully crash-durable yet:

- the temporary filename is fixed, so multiple writers can collide;
- the temporary file is not opened with exclusive creation;
- no explicit temp-file `sync_all()` is performed before rename;
- no parent-directory sync is performed after rename;
- there is no explicit single-writer/locking contract;
- stale `.tmp` recovery is not defined.

For the current single-GUI-owner architecture, a documented single-writer invariant plus a unique same-directory temp file is sufficient for the first production slice. If CLI/daemon writers are introduced later, add a locking/revision protocol rather than relying on last-writer-wins behavior.

## 2.7 Current migrations

There is a `migrate()` function and a `backup_before_migration()` helper, but migration is not actually implemented yet.

Current behavior:

- v1 → accepted;
- version `< 1` → `ConfigError::Migration` saying migration is not implemented;
- version `> 1` → `UnsupportedVersion`.

`backup_before_migration()` copies the current config to a timestamped backup, but normal load/migrate flow does not invoke it.

So the accurate statement is: **migration scaffolding exists; production migrations do not yet exist**.

## 2.8 Current unknown-field behavior

The structs use `#[serde(default)]` and do not use `deny_unknown_fields`.

Consequences:

- missing fields are populated from Rust defaults;
- a missing `config_version` can inherit the current default rather than being rejected as structurally ambiguous;
- unknown TOML fields can be ignored by deserialization;
- if an ignored unknown field is then saved by an older application, that field can be lost.

This is convenient for early development but is not a strong backward/forward compatibility contract.

---

# 3. Persistence domains

## 3.1 Rule: persist by semantics, not by UI location

A setting appearing in the Preferences window does not automatically make it a “preference”, and a serializable value does not automatically belong in `preferences.toml`.

Recommended categories:

| Category | Meaning | Storage owner | Hardware side effect on load? |
|---|---|---|---|
| Safe user preference | Presentation/application behavior | `preferences.toml` | Never |
| Desired hardware state | User wants a hardware property to converge to a value | separate future `desired-state.toml` | Never merely because file was loaded |
| Automation policy | Inert desired-state/rule policy | `automation.toml` | Never merely because file was loaded |
| Runtime-only state | Observations/transient UI state | memory or XDG state | Never |
| External desktop integration | State owned by desktop/session mechanism | actual integration artifact | Only that desktop-level integration |

## 3.2 Safe user preferences

Good candidates for `preferences.toml` once their actual product behavior exists:

- theme (`Dark` / `Light`);
- language;
- close action;
- start minimized;
- remember window position toggle;
- desktop notifications preference;
- tray indicator preference;
- automatic update-check preference;
- update channel preference;
- telemetry refresh cadence, when runtime reconfiguration exists;
- UI scale, when it controls actual rendering rather than preview only.

These are user/application choices and do not require hardware authorization.

## 3.3 Desired hardware state

Examples:

- persisted battery charge threshold intent;
- future persisted performance/GPU/fan/display desired state, if deliberately supported.

These values must not share implicit lifecycle with preferences. Loading a TOML file is not permission to write hardware.

The current `BatteryConfig::charge_limit` is the clearest existing example: it is explicitly user intent and validated against the Hardware1 range, but it must not be applied merely because a config parser instantiated `AppConfig`.

If persistent desired hardware state becomes a product feature, use a separate file and an explicit reconciliation/security design.

## 3.4 Automation policy

Automation config is policy, not execution.

Persisting a rule means only:

> “remember this rule definition for later use by an explicitly designed automation subsystem.”

It must not mean:

> “execute this rule now” or “authorize future privileged writes”.

Current source already has useful domain pieces in `orbis-core::automation`, but the UI/backend audit shows no production reconciler/executor and no approved privileged-background-caller design.

Automation persistence must therefore remain inert.

## 3.5 Runtime-only state

Do not store these in user preferences:

- current telemetry values;
- current hardware capability snapshot;
- current observed performance/GPU/fan states;
- loading/error flags;
- dirty editor state;
- pending worker commands;
- D-Bus connection status;
- open popup/menu flags;
- active native window handles;
- current `THEME_LIGHT` runtime cache itself.

Window geometry is a useful nuance:

- `remember_position = true/false` is a **preference**;
- the actual previous `(x, y, width, height)` is **application state** and should live under XDG state if persisted.

## 3.6 Experimental/security-sensitive gates

`experimental.enabled`, `undervolting`, and `raw_wmi` should not be automatically promoted into ordinary user preferences.

They deserve a separate feature/security policy contract. In particular, persisting a dangerous feature gate must not implicitly create a privileged write path.

---

# 4. Current Preferences UI audit

Source: `ui/audited/preferences-window.slint` at `agent/light-theme-toggle`.

Current real Preferences controls:

| UI control | Current UI behavior | Recommended category | Persist now? |
|---|---|---|---|
| Dark / Light | Real process-local theme switch | safe user preference | **Yes: first slice** |
| Language | local selector only | safe user preference | schema-ready, behavior later |
| Close button | local 3-state selector | safe user preference | schema-ready, behavior later |
| Restore window position | local bool | safe user preference | preference yes; geometry goes to XDG state |
| Run on startup | local bool | external desktop integration | **Do not store canonical bool** |
| Desktop notifications | local bool | safe user preference | when notifications exist |
| Tray indicator | local bool | safe user preference | when tray exists |
| Automatic update checks | local bool | safe user preference | when updater exists |
| Update channel | local selector | safe user preference | when update backend exists |
| Telemetry refresh | local 0.5–5 Hz slider | safe runtime policy | persist only when runtime setter exists |
| UI scale preview | local 80–160% slider | safe user preference | persist only when actual scale behavior exists |
| Reset | local reset | preference operation | wire with persisted defaults later |
| Save | local status only | persistence operation | wire only when real storage exists |

The current UI model is broader than current `UiConfig`. Do not pretend that the entire window is already represented by `orbis-config`.

### Existing schema mismatch: Close button

The current config has:

```text
close_to_tray: bool
```

The UI has:

```text
Hide to tray
Quit
Ask every time
```

A production preferences schema therefore needs a typed three-state enum. A boolean is insufficient.

### Existing duplicate: Run on Startup

`AppWindow` has a local `startup-preview` checkbox and `PreferencesWindow` has a separate local `startup` bool. They can currently diverge.

When autostart is wired, both controls must display one Rust-owned `AutostartState` derived from the actual desktop integration, not keep independent Slint booleans.

---

# 5. Recommended preferences schema

## 5.1 Dedicated file

Recommended file:

```text
$XDG_CONFIG_HOME/orbis-control/preferences.toml
```

Example schema:

```toml
schema_version = 1

[appearance]
theme = "dark"
ui_scale_percent = 100

[localization]
language = "system"

[window]
close_action = "hide_to_tray"
start_minimized = false
remember_position = true

[desktop]
notifications = true
tray_indicator = true

[updates]
automatic_checks = true
channel = "stable"

[telemetry]
interval_ms = 1000
```

This is a **target schema**, not an assertion that all listed controls are production-functional today.

## 5.2 Typed Rust representation

Use typed enums internally instead of passing arbitrary strings around application code:

- `ThemePreference::{Dark, Light}`;
- `LanguagePreference::{System, En, Ru, De, Fr}`;
- `CloseAction::{HideToTray, Quit, Ask}`;
- `UpdateChannel::{Stable, Preview, Development}`.

TOML can still serialize those enums to stable lowercase/snake-case names.

Use integers for bounded numeric policy where practical:

- `ui_scale_percent: u16`;
- `telemetry.interval_ms: u64`.

Prefer interval milliseconds over persisted floating-point Hz. The current UI range 0.5–5 Hz corresponds to 2000–200 ms. Integer intervals avoid float serialization/equality ambiguity and map naturally to timers.

## 5.3 Defaults

Safe defaults must be conservative and deterministic:

- theme: Dark, matching current product default;
- language: System;
- close action: Hide to tray only when tray behavior is actually implemented; otherwise default Quit until that behavior exists;
- start minimized: false;
- remember position: true is acceptable, but geometry must be absent until first successful capture;
- autostart: disabled unless actual XDG Autostart state says enabled;
- automation: disabled in the new policy schema until an executor exists;
- update/notification/tray defaults should not claim behavior before their implementations exist.

A missing preferences file is not corruption and should simply produce schema defaults.

---

# 6. XDG storage design

## 6.1 Layout

Recommended production layout:

```text
$XDG_CONFIG_HOME/
  orbis-control/
    preferences.toml
    automation.toml
    desired-state.toml        # future, only if intentionally supported

$XDG_STATE_HOME/
  orbis-control/
    window-state.toml         # optional cross-restart UI state

$XDG_CONFIG_HOME/
  autostart/
    io.github.orbiscontrol.Orbis.desktop
```

`config.toml` remains a legacy import source during migration.

## 6.2 Correct XDG path semantics

Path resolution should obey XDG Base Directory Specification 0.8:

- use `XDG_CONFIG_HOME` only when non-empty and absolute;
- otherwise use `$HOME/.config`;
- use `XDG_STATE_HOME` only when non-empty and absolute;
- otherwise use `$HOME/.local/state`;
- if neither a valid XDG path nor a safe absolute home fallback can be resolved, return a typed error;
- never use current working directory as an implicit production config location.

Config directories created by Orbis should be user-owned. No root helper is involved.

## 6.3 Atomic replacement

Recommended write sequence for each Orbis-owned TOML file:

1. build and validate the complete new typed value in memory;
2. serialize before touching the current file;
3. ensure parent directory exists;
4. create a unique same-directory temporary file with exclusive creation;
5. write all bytes;
6. flush and `sync_all()` the temporary file;
7. rename temporary file over the destination;
8. sync the parent directory where supported;
9. remove stale temporary state on handled failures.

Example naming shape:

```text
.preferences.toml.tmp.<pid>.<nonce>
```

Do not use a globally shared `/tmp` file: rename must stay on the same filesystem and the temporary file must remain in the user-owned target directory.

## 6.4 Single-writer policy

Initial production invariant:

> the GUI/application preference service is the only writer of `preferences.toml` during a process lifetime.

If a CLI or daemon later edits the same file, introduce an explicit concurrency contract such as advisory locking plus revision/CAS checks. Do not silently make multiple processes last-writer-wins.

## 6.5 File permissions and secrets

Preferences contain no secrets, but Orbis should still create its own user config files with restrictive user-write/read permissions where practical, e.g. `0600`, and config directories with user-only write access.

Do not put credentials, authorization tokens, polkit grants, or other secrets into these TOML files.

---

# 7. Corruption and recovery

## 7.1 Missing is not corrupt

Missing file:

```text
preferences.toml does not exist
```

→ use defaults; no warning required.

## 7.2 Corrupt current-version file

Examples:

- malformed TOML;
- unknown current-schema enum value;
- invalid numeric range;
- missing mandatory `schema_version`;
- internal current-schema invariant violation.

Recommended behavior:

1. preserve the original file unchanged;
2. start the application with safe runtime defaults for preferences;
3. expose a non-fatal “preferences could not be loaded” state/log entry;
4. do **not** silently overwrite the file during startup;
5. on explicit user Reset/Save, preserve the bad source as `preferences.toml.corrupt-<timestamp>` and then write a new valid file atomically.

For theme specifically, corruption should result in safe Dark for the session, not application startup failure.

## 7.3 Future-version file

If `schema_version > CURRENT_SCHEMA_VERSION`:

- do not parse it as current schema;
- do not rewrite it;
- use safe runtime defaults if the UI must continue;
- mark preference persistence read-only/unavailable for the session unless the user explicitly chooses a destructive reset.

This prevents an older Orbis binary from destroying fields written by a newer binary.

## 7.4 I/O and permission failures

Permission denied, read errors, disk-full, rename failure, or sync failure are not “corruption”. Preserve the current file and return/report the exact storage error.

If a preference change already applied in memory but persistence fails, UI should state that it is active for the current session but could not be saved.

---

# 8. Versioning, migrations, and compatibility

## 8.1 Per-file version

Each domain file should have its own mandatory version:

```toml
schema_version = 1
```

Do not use one global version shared by preferences, automation policy, desired hardware state, and runtime state. They evolve independently.

## 8.2 Mandatory version parsing

Recommended load pipeline:

1. parse TOML to a minimal envelope/value only far enough to obtain `schema_version`;
2. reject missing/non-integer version;
3. if future version, stop without rewriting;
4. dispatch to the exact schema type for that version;
5. validate semantic invariants;
6. run explicit migration steps if older supported versions are encountered.

Do not rely on `#[serde(default)]` to manufacture a current version when `schema_version` is missing.

## 8.3 Strict known schema

For owned current-version sections, prefer strict decoding (`deny_unknown_fields` or an equivalent explicit check).

Reason: a misspelled key such as `themee = "light"` should not silently become Dark and then disappear on the next save.

If forward-extensible third-party data is ever required, reserve an explicit namespace such as:

```toml
[extensions.vendor-name]
...
```

Do not treat every arbitrary unknown top-level field as an extension.

## 8.4 Migration chain

Use explicit pure transformations:

```text
v1 -> v2
v2 -> v3
...
```

Each step should be unit-tested independently.

Before the first on-disk rewrite caused by migration:

1. preserve the original file in a versioned/timestamped backup;
2. migrate entirely in memory;
3. validate the final schema;
4. write the replacement atomically;
5. if any step fails, leave the original authoritative file untouched.

## 8.5 Backward compatibility rule

Any change that an older binary could misinterpret or erase should increment `schema_version`.

Older binaries encountering a future version must refuse to persist over it. This is more important than making every future field silently ignorable.

---

# 9. Legacy `config.toml` migration strategy

The existing v1 `AppConfig` should not be deleted or rewritten wholesale when `preferences.toml` is introduced.

## 9.1 Preferences import

On startup when `preferences.toml` is absent:

1. look for legacy `config.toml`;
2. parse it using the existing v1 loader;
3. import only fields with exact safe-preference semantics;
4. write `preferences.toml` only when migration/import is successful;
5. retain legacy `config.toml` because automation/battery/experimental data may still live there.

Safe imports:

| Legacy v1 field | New target |
|---|---|
| `ui.theme` | `appearance.theme` |
| `ui.start_minimized` | `window.start_minimized` |
| `ui.remember_position` | `window.remember_position` |
| `ui.close_to_tray = true` | `window.close_action = "hide_to_tray"` |
| `ui.close_to_tray = false` | `window.close_action = "quit"` |

There is no legacy representation of `Ask every time`, so migration must not invent it.

## 9.2 Do not import unrelated domains into preferences

Do not migrate these into `preferences.toml`:

- `automation.*`;
- `battery.charge_limit`;
- `experimental.*`.

They require their own domain migrations.

## 9.3 Authority after import

Once `preferences.toml` exists and is valid, it becomes authoritative for safe preferences. Legacy `config.toml.ui.*` must not override it on later starts.

This prevents two independent files from fighting over theme/window behavior.

---

# 10. Theme persistence

## 10.1 Current source behavior

`agent/light-theme-toggle` already implements the difficult multi-window part correctly for one process:

- `ThemeMode::{Dark, Light}`;
- `ThemeState` is per top-level component/window;
- Rust owns a process-local `THEME_LIGHT` value;
- `apply_theme_to_all()` updates Main, Fans + Power, Extra, Automation, Preferences, Diagnostics, Updates, and preview dialogs that already exist;
- windows created later read the current process theme before they are shown.

The missing piece is persistence lifecycle.

## 10.2 Startup default

If no valid persisted theme exists:

```text
Dark
```

This matches current product behavior.

Do not infer OS dark/light theme in this slice unless a separate “System” theme option is deliberately introduced. Current UI exposes only Dark and Light.

## 10.3 Restore before first visible render

Recommended startup order:

```text
resolve config path
→ load/recover PreferencesConfig
→ choose ThemePreference (or safe Dark fallback)
→ initialize Rust process theme state
→ construct AppWindow
→ set its ThemeState immediately
→ wire application/UI state
→ show first window
```

The current `build_app()` already sets `ThemeState` immediately after `AppWindow::new()` and the main window is shown only later. If the persisted value is loaded into the process theme before `build_app()`, Light can be selected before the first visible frame.

This avoids the common visual sequence:

```text
show Dark
→ load config
→ switch to Light
```

## 10.4 Multi-window synchronization

Keep one Rust-owned current theme as the synchronization source.

On user change:

1. apply theme immediately to every existing top-level window;
2. update the in-memory preference model;
3. atomically persist the new preference;
4. every newly created window initializes from that same current theme before `show()`.

Do not try to make each top-level Slint `ThemeState` independently load a file.

## 10.5 Persistence failure semantics

If the user switches to Light and the disk write fails:

- Light remains active for the current process;
- the UI reports that the setting could not be persisted;
- do not revert visually unless product UX explicitly chooses transactional UI behavior;
- next application start falls back to the last successfully saved preference/default.

No false “Saved” status.

## 10.6 Theme tests

Targeted tests:

1. missing preferences → Dark before first show;
2. persisted Dark → Dark before first show;
3. persisted Light → Light before first show;
4. persisted Light offscreen first frame contains Light theme, with no intermediate visible Dark render;
5. corrupt preference → safe Dark + recoverable warning state;
6. theme change updates every existing window;
7. window created after theme change inherits current theme;
8. theme change writes the new value atomically;
9. write failure keeps session theme but reports non-persistence;
10. unknown/future preferences version does not get overwritten.

---

# 11. Run on Startup

## 11.1 A boolean is not the implementation

A stored field such as:

```toml
run_on_startup = true
```

would record only intention. It would not prove that the desktop will actually launch Orbis after login.

The Preferences control must reflect and mutate the real desktop/session integration.

## 11.2 Recommended Linux mechanism: user XDG Autostart

For the GUI application, use the freedesktop Desktop Application Autostart mechanism.

User-level file:

```text
$XDG_CONFIG_HOME/autostart/io.github.orbiscontrol.Orbis.desktop
```

Default when `XDG_CONFIG_HOME` is unset/empty:

```text
$HOME/.config/autostart/io.github.orbiscontrol.Orbis.desktop
```

This mechanism is explicitly designed to start desktop applications after the user logs into a desktop environment. It is user-owned and requires no root or polkit.

The existing NixOS module's `systemd.user.services.orbis-sessiond` is a different concern: it starts the Orbis read-only/session daemon. It is not the canonical implementation of the GUI “Run on Startup” toggle.

## 11.3 NixOS-specific conclusion

Do **not** make the GUI toggle:

- edit `/etc/nixos/configuration.nix`;
- run `nixos-rebuild`;
- modify system services;
- ask hardwared/root/polkit to manage GUI startup;
- use Session1 as a startup-management deputy.

NixOS currently ships systemd user graphical targets and integration for XDG autostart generation, but desktop-session behavior varies enough that Orbis should use the desktop-standard XDG Autostart contract for a GUI login application rather than inventing a NixOS-only preference protocol.

## 11.4 Package integration

Current Orbis package installs the binaries and D-Bus/polkit assets but does not install an application `.desktop` launcher or an autostart desktop file.

A future startup integration slice should install a normal application desktop entry as package data. The per-user toggle should create/manage the user autostart entry; the package should not force a system-wide enabled autostart entry by default.

For Nix-store robustness, the managed autostart entry should prefer stable command-name semantics such as:

```ini
Exec=orbis-control
TryExec=orbis-control
```

rather than persisting the currently resolved `/nix/store/<hash>-.../bin/orbis-control` path, which can become stale after package upgrades/garbage collection.

Before enabling, Orbis should prove that the command is resolvable in the graphical-session execution environment. If it cannot, report autostart unavailable rather than writing a known-broken entry.

## 11.5 Source of truth

Recommended runtime model:

```text
AutostartState
  Disabled
  EnabledManaged
  EnabledExternal
  DisabledByUserOverride
  Conflict
  Unavailable(reason)
```

Exact enum names may differ; the important point is that UI derives state from the actual autostart files/desktop-entry semantics.

`preferences.toml` must not be the authority for this control.

Both Main and Preferences “Run on Startup” controls should bind to this one state.

## 11.6 Respect external/admin entries

XDG Autostart gives the user configuration directory higher precedence than system autostart directories. A user entry with the same filename and `Hidden=true` suppresses a lower-priority system entry.

Orbis should therefore:

- inspect same-name user/system entries;
- use `Hidden=true` when a user needs to disable an externally provided same-name system autostart entry;
- mark files it creates with a harmless namespaced key such as `X-Orbis-Control-Managed=true`;
- avoid deleting or blindly overwriting an unfamiliar user-authored file;
- report a conflict when safe ownership cannot be established.

## 11.7 Startup tests

Using temporary XDG roots only:

1. no entry → Disabled;
2. managed user entry → EnabledManaged;
3. system entry without user override → EnabledExternal;
4. user `Hidden=true` override → disabled state;
5. enable creates exact managed `.desktop` atomically;
6. disable removes an Orbis-managed user entry or writes a safe Hidden override when needed;
7. unfamiliar user entry is preserved and becomes Conflict, not overwritten;
8. `TryExec`/command unavailable becomes Unavailable;
9. Main and Preferences controls display identical state;
10. no root, polkit, Hardware1, Session1 mutation, or system configuration write occurs.

---

# 12. Automation policy persistence

## 12.1 Persistence is not an executor

The initial automation persistence slice must have this invariant:

> Loading or saving `automation.toml` performs zero hardware operations and emits zero hardware mutation commands.

No rule should be executed as a side effect of:

- deserialization;
- migration;
- opening AutomationWindow;
- clicking Save Rules;
- application startup.

## 12.2 Separate file

Recommended:

```text
$XDG_CONFIG_HOME/orbis-control/automation.toml
```

with its own `schema_version`.

Initial default should be conservative:

```text
enabled = false
```

The current legacy `AutomationConfig::default()` uses `enabled = true`, but there is currently no executor. Importing that early-stage default into a future executable subsystem without a separate security/lifecycle decision would be unsafe.

## 12.3 Typed policy

Prefer typed domain values over the current `refresh_policy: Option<String>`.

`orbis-core::automation::RefreshPolicy` already has:

- Minimum;
- Maximum;
- Auto;
- Fixed(RefreshHz).

The initial persistence schema should only encode policy values whose semantics are actually proven.

Do not guess mappings merely because the Automation UI has a selector.

Examples of current gaps:

- product GPU mode semantics are not fully proven in production;
- display refresh mutation backend is not production-wired;
- Automation lighting UI has Off / Dim / Normal / Keep current, while current core action `SetLighting(bool)` cannot faithfully represent those four states;
- background privileged caller/authorization/reconciliation semantics are not established.

Persisting semantically unproven selectors as executable actions would create false confidence. Either store them as explicitly inert draft UI state in a separate draft schema or, preferably, defer those fields until exact domain semantics exist.

## 12.4 No privileged execution through sessiond

This design does not add setter methods to Session1 and does not turn sessiond into a privileged automation deputy.

Future automation execution needs its own security design covering:

- who owns the executor;
- how user consent is represented;
- how privileged operations are authorized in the background;
- desired-vs-observed reconciliation;
- idempotency;
- cooldown/debounce;
- resume/login lifecycle;
- failures and partial application;
- auditability.

None of that is granted by `automation.toml` existing.

## 12.5 Custom commands

`AutomationAction::CustomCommand(Vec<String>)` exists in core with a comment requiring explicit permission and no `/bin/sh -c`.

Do not include CustomCommand in the first production automation persistence slice. Command persistence/execution is a separate security surface and should be designed and reviewed independently.

---

# 13. Desired hardware state

This task is not designing a desired-state executor, but the storage boundary must be explicit now so Preferences does not accidentally absorb it later.

Recommended future location if product chooses persistent hardware intent:

```text
$XDG_CONFIG_HOME/orbis-control/desired-state.toml
```

Potentially includes:

- battery charge-limit intent;
- other hardware desired state only after each capability's lifecycle semantics are proven.

Required invariant:

> Loading `desired-state.toml` does not itself authorize or perform hardware writes.

Application of persisted hardware state must be a separately explicit reconciliation feature with normal capability checks, Hardware1/polkit security, and authoritative read-back.

The existing legacy `battery.charge_limit` should remain in legacy config until that design exists. Do not move it into `preferences.toml` just to simplify migration.

---

# 14. Runtime/UI state storage

`XDG_STATE_HOME` is appropriate for state that should survive restart but is not a portable user preference.

Example future file:

```text
$XDG_STATE_HOME/orbis-control/window-state.toml
```

Potential contents:

- last main-window position;
- last main-window size if product allows resizable geometry;
- perhaps last selected non-hardware UI tab where useful.

Do not put:

- telemetry snapshots;
- capability registry snapshots;
- hardware read-back values intended to masquerade as current on next boot;
- pending privileged operations;
- authentication/authorization state.

If `remember_position = false`, ignore/delete saved window geometry without changing unrelated preferences.

---

# 15. Recommended load/save ownership

## 15.1 Application-side preference service

Preferences are an application concern, not a provider/hardware concern.

Recommended future boundary:

```text
orbis-config typed store
→ application/UI preference service
→ Rust window lifecycle
→ Slint presentation
```

No Session1 round trip is needed to read Dark/Light or close behavior.

## 15.2 One in-memory snapshot

At process startup:

- load one immutable/current `PreferencesConfig` snapshot;
- derive UI/runtime presentation state from it;
- on edits, validate a replacement snapshot and atomically persist it;
- publish the replacement to all interested windows/components.

This avoids each Preferences control independently opening/parsing/writing TOML.

## 15.3 Save semantics

Two UX models are possible:

1. immediate-save per preference change;
2. staged edit + explicit Save.

Theme already applies immediately, so immediate apply + immediate persistence is the natural first implementation.

For the wider Preferences window, staged Save can remain if desired, but the UI must distinguish:

- edited in window;
- applied to runtime;
- persisted successfully.

Do not display “saved” before storage confirms success.

---

# 16. Minimal implementation slices

These are future implementation slices. This research branch does not make any of these changes.

## Slice 1 — production config schema/storage

### Goal

Create a durable, domain-separated preference store and safe legacy import without wiring hardware or the UI yet.

### Likely files

- `crates/orbis-config/src/lib.rs`
- `crates/orbis-config/src/paths.rs`
- `crates/orbis-config/src/store.rs` or split into:
  - `preferences.rs`
  - `legacy.rs`
  - shared atomic storage helper
- `crates/orbis-config/Cargo.toml` only if a narrowly justified storage helper dependency is needed

### Work

- add `PreferencesConfig` with mandatory `schema_version`;
- typed theme/language/close/update enums;
- bounded UI scale and telemetry interval;
- correct XDG absolute/empty path handling;
- remove current-working-directory fallback from production path resolution;
- unique same-directory atomic temp replacement;
- define corrupt/future-version recovery behavior;
- implement one-way safe import of legacy `AppConfig.ui` fields;
- preserve legacy config after import;
- no automation/battery/experimental import into preferences.

### Tests

- absolute `XDG_CONFIG_HOME`;
- unset and empty XDG env fallback;
- relative XDG env ignored;
- no safe HOME → typed error, never `.`;
- missing preferences → defaults;
- missing `schema_version` rejected;
- future schema version preserved/not rewritten;
- unknown current-version field rejected or explicitly namespaced;
- invalid enum/range rejected;
- corrupt file preserved;
- explicit recovery creates backup then valid replacement;
- temp file is unique and same-directory;
- interrupted/stale temp does not replace final file;
- legacy theme/start-minimized/remember-position/close mapping import;
- legacy automation/battery/experimental not imported;
- round-trip deterministic enough for fixtures.

### Risk

**Medium.** No hardware risk, but migration/data-loss mistakes can affect user configuration. Treat file preservation and older/newer version behavior as the primary risks.

### Hardware writes

**Zero.**

---

## Slice 2 — theme persistence wiring

### Goal

Make Dark/Light survive restart without changing current hardware/backend behavior.

### Likely files

- `crates/orbis-ui/Cargo.toml` — dependency on `orbis-config`;
- `crates/orbis-ui/src/main.rs`;
- optionally a small `crates/orbis-ui/src/preferences.rs` lifecycle helper;
- `ui/audited/preferences-window.slint` only to expose real save/error status and remove session-only wording.

### Work

- load preferences before the first top-level window is shown;
- initialize process theme from persisted value;
- retain existing `apply_theme_to_all()` synchronization;
- persist on user theme change;
- newly created windows inherit current theme before `show()`;
- no OS theme inference in this slice;
- save failure is visible and does not crash the UI.

### Tests

- default Dark;
- persisted Light restored before first visible frame;
- no visible first-frame Dark flash for persisted Light in offscreen render;
- all open windows update;
- later-created windows inherit;
- persistence round-trip;
- corruption → safe Dark + warning;
- write failure → session theme active, persistence failure reported.

### Risk

**Low/Medium.** Presentation-only. Primary risks are startup ordering, multi-window drift, and falsely reporting a failed save as persistent.

### Hardware writes

**Zero.**

---

## Slice 3 — user Run on Startup integration

### Goal

Turn both visible Run on Startup controls into one real user-level desktop autostart capability.

### Likely files

- new `crates/orbis-ui/src/autostart.rs` or a small unprivileged desktop-integration crate/module;
- `crates/orbis-ui/src/main.rs`;
- `ui/audited/main-window.slint`;
- `ui/audited/preferences-window.slint`;
- future packaging data for a normal application desktop entry;
- future `packaging/nix/package.nix` update to install that desktop entry as package data.

No NixOS runtime configuration mutation is required.

### Work

- probe actual XDG Autostart state;
- create/remove or override a user `.desktop` entry safely;
- use command-name `Exec`/`TryExec` semantics appropriate for Nix package upgrades;
- protect unfamiliar user-managed files;
- synchronize Main and Preferences toggles from one state;
- do not persist a competing `run_on_startup` bool in preferences;
- no root/polkit/system config writes.

### Tests

- absent/managed/external/Hidden/conflict states;
- atomic enable/disable under temp XDG config roots;
- preserve foreign user edits;
- unavailable executable;
- both windows remain synchronized;
- no calls to Hardware1, Session1 mutation, root, `nixos-rebuild`, or system service management.

### Risk

**Medium.** No hardware risk, but desktop-entry precedence/ownership and Nix-store upgrade behavior need exact handling.

### Hardware writes

**Zero.**

---

## Slice 4 — automation-policy persistence only

### Goal

Allow AutomationWindow to remember inert policy without creating an executor.

### Likely files

- `crates/orbis-config/src/automation.rs` or equivalent domain store;
- `crates/orbis-config/src/lib.rs`;
- `crates/orbis-ui/src/main.rs` or a dedicated policy presentation helper;
- `ui/audited/automation-window.slint`;
- `orbis-core` only if an exact new typed policy enum is required to represent proven UI semantics.

Explicitly **not**:

- `orbis-sessiond` setters;
- `orbis-hardwared` changes;
- polkit changes;
- background hardware executor.

### Work

- add independent `automation.toml` schema/version;
- default new policy to disabled;
- persist only exact typed/proven rule semantics;
- replace free-form refresh policy strings with typed representation;
- defer semantically unproven lighting/GPU/display executable mappings;
- Save Rules performs config write only;
- loading policy performs zero operations.

### Tests

- round-trip typed policy;
- default disabled;
- migration/future-version/corruption behavior;
- unproven values rejected or omitted;
- Save Rules emits no `WorkerCommand` hardware mutation;
- load emits no provider/D-Bus/sysfs write;
- persistence does not change current hardware/UI observed state;
- CustomCommand absent from initial schema.

### Risk

**Medium.** The storage itself is simple; the risk is accidentally treating persisted policy as authorization or execution intent. Encode “inert persistence” as a testable architectural invariant.

### Hardware writes

**Zero.**

---

# 17. Recommended implementation order

```text
1. preferences schema/storage
2. theme persistence
3. user autostart integration
4. automation-policy persistence
```

Rationale:

- Slice 1 establishes the data-loss/versioning contract first.
- Slice 2 immediately converts an already-real UI feature from session-local to persistent with very low semantic risk.
- Slice 3 is independent of TOML preference persistence and should be tested against the actual XDG Autostart artifact.
- Slice 4 should come after file-domain separation so early automation fields do not remain mixed with UI preferences.

Desired hardware state should be handled in a separate future design, not smuggled into any of these four slices.

---

# 18. Non-goals and safety boundaries

This design does **not** propose:

- applying hardware state on config load;
- changing Hardware1;
- changing polkit;
- adding privileged Session1 methods;
- using sessiond as an automation mutation executor;
- writing sysfs;
- changing fan/GPU/battery mutation semantics;
- running arbitrary commands from Preferences;
- editing NixOS system configuration from the GUI;
- using root to enable GUI autostart;
- silently discarding corrupt/future config files;
- treating config persistence as proof that a visual control has a production backend.

All proposed preference/autostart/policy persistence operations are user-level filesystem operations only.

---

# 19. Source inventory

Audited repository sources:

- `crates/orbis-config/src/lib.rs`
- `crates/orbis-config/src/paths.rs`
- `crates/orbis-config/src/store.rs`
- `crates/orbis-config/Cargo.toml`
- `crates/orbis-core/src/automation.rs`
- `crates/orbis-ui/src/main.rs` on `agent/light-theme-toggle`
- `crates/orbis-ui/Cargo.toml` on `agent/light-theme-toggle`
- `ui/themes/dark.slint` on `agent/light-theme-toggle`
- `ui/audited/preferences-window.slint` on `agent/light-theme-toggle`
- `ui/audited/main-window.slint` on `agent/light-theme-toggle`
- `ui/audited/automation-window.slint` on `agent/light-theme-toggle`
- `packaging/nix/module.nix`
- `packaging/nix/package.nix`
- PR #5 `docs/ui-backend-wiring-matrix.md` for current wiring classification.

External primary references used for desktop/path semantics:

- freedesktop.org **XDG Base Directory Specification 0.8**;
- freedesktop.org **Desktop Application Autostart Specification 0.5**;
- current Nixpkgs/NixOS systemd-user/display-session source for `graphical-session.target` / XDG autostart integration context.

---

# 20. Decision summary

1. Keep `orbis-config`, but evolve it from one legacy composite `AppConfig` into domain-specific stores.
2. `preferences.toml` contains only safe application/UI preferences.
3. Theme is the first production persistence target.
4. Load theme before first visible window and retain the existing Rust multi-window synchronization spine.
5. Run on Startup is owned by user XDG Autostart state, not a TOML bool.
6. No root/polkit/NixOS configuration mutation is needed for GUI autostart.
7. Automation policy gets a separate file and remains inert; persistence is not an executor or authorization mechanism.
8. Desired hardware state remains a separate domain and is never applied merely because it was loaded.
9. Runtime/UI restart state belongs under XDG state, not preferences.
10. Missing config uses defaults; corruption/future versions are preserved and never silently overwritten.
11. Schema versions are mandatory per file, migrations are explicit chains, and older binaries must not rewrite future-version files.
12. Atomic replacement should be strengthened beyond the current fixed `.tmp` + rename implementation before calling it production-durable.
