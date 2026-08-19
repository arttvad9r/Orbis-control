//! # Orbis Config
//!
//! Пользовательская конфигурация: TOML, атомарная запись, миграции, XDG-пути.
//!
//! На Этапе 2 конфигурация хранит mock-настройки UI и пользовательские намерения;
//! она НЕ имитирует постоянное применение настроек к железу.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod paths;
pub mod preferences;
pub mod store;
pub mod window_state;

pub use preferences::{
    AppearancePreferences, CloseAction, PREFERENCES_FILE, PREFERENCES_SCHEMA_VERSION,
    PreferencesConfig, PreferencesError, PreferencesLoad, PreferencesLoadSource,
    PreferencesPathError, PreferencesWarning, PreferencesWarningKind, ThemePreference,
    WindowPreferences, load_preferences, load_preferences_from_dir, preferences_dir,
    preferences_dir_with, preferences_file, save_preferences, save_preferences_to_dir,
};
pub use store::{
    AppConfig, AutomationConfig, BatteryConfig, ExperimentalConfig, UiConfig, load_or_default,
};
pub use window_state::{
    WINDOW_STATE_FILE, WINDOW_STATE_SCHEMA_VERSION, WindowPositionState, WindowState,
    WindowStateError, WindowStateLoad, WindowStateLoadSource, WindowStatePathError,
    WindowStateWarning, WindowStateWarningKind, load_window_state, load_window_state_from_dir,
    save_window_state, save_window_state_to_dir, window_state_dir, window_state_dir_with,
    window_state_file,
};

use std::path::PathBuf;

/// Имя каталога конфигурации в XDG.
pub const CONFIG_DIR_NAME: &str = "orbis-control";

/// Текущая версия формата конфигурации.
pub const CONFIG_VERSION: u32 = 1;

/// Определить каталог конфигурации по XDG.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".config"))
                .unwrap_or_else(|| PathBuf::from("."))
        })
        .join(CONFIG_DIR_NAME)
}
