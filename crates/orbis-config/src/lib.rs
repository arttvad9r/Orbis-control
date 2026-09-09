//! # Orbis Config
//!
//! User-owned configuration and state persistence for Orbis Control.
//!
//! Hardened production stores keep concerns separate: application preferences,
//! window state, XDG autostart and desired hardware state are independent. The
//! legacy `store::AppConfig` API remains for compatibility only and must not be
//! treated as an automatic reconciliation source (see issue #113).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod autostart;
pub mod desired_state;
pub mod paths;
pub mod policy_desired;
pub mod preferences;
pub mod store;
pub mod window_state;

pub use autostart::{
    AUTOSTART_ENTRY, AUTOSTART_FILE_NAME, AutostartError, AutostartPathError, AutostartStatus,
    autostart_dir, autostart_dir_with, autostart_file, autostart_status, autostart_status_from_dir,
    disable_autostart, disable_autostart_from_dir, enable_autostart, enable_autostart_from_dir,
};
pub use desired_state::{
    DESIRED_STATE_FILE, DESIRED_STATE_SCHEMA_VERSION, DesiredStateDocument, DesiredStateError,
    DesiredStateLoad, DesiredStateLoadSource, DesiredStatePathError, DesiredStateWarning,
    DesiredStateWarningKind, desired_state_dir, desired_state_dir_with, desired_state_file,
    load_desired_state, load_desired_state_from_dir, save_desired_state, save_desired_state_to_dir,
};
pub use policy_desired::{
    PolicyDesiredState, UnspecifiedPresetFieldPolicy, preset_intent_from_desired,
};
pub use preferences::{
    AppearancePreferences, CloseAction, PREFERENCES_FILE, PREFERENCES_SCHEMA_VERSION,
    PreferencesConfig, PreferencesError, PreferencesLoad, PreferencesLoadSource,
    PreferencesPathError, PreferencesWarning, PreferencesWarningKind, ThemePreference,
    WindowPreferences, load_preferences, load_preferences_from_dir, preferences_dir,
    preferences_dir_with, preferences_file, save_preferences, save_preferences_to_dir,
};
pub use store::{AppConfig, BatteryConfig, ExperimentalConfig, UiConfig, load_or_default};
pub use window_state::{
    WINDOW_STATE_FILE, WINDOW_STATE_SCHEMA_VERSION, WindowPositionState, WindowState,
    WindowStateError, WindowStateLoad, WindowStateLoadSource, WindowStatePathError,
    WindowStateWarning, WindowStateWarningKind, load_window_state, load_window_state_from_dir,
    save_window_state, save_window_state_to_dir, window_state_dir, window_state_dir_with,
    window_state_file,
};

/// Имя каталога конфигурации.
pub const CONFIG_DIR_NAME: &str = "orbis-control";

/// Текущая версия формата конфигурации.
pub const CONFIG_VERSION: u32 = 1;
