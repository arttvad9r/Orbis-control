//! # Orbis Config
//!
//! Пользовательская конфигурация: TOML, атомарная запись, миграции, XDG-пути.
//!
//! На Этапе 2 конфигурация хранит mock-настройки UI и пользовательские намерения;
//! она НЕ имитирует постоянное применение настроек к железу.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod desired_state;
pub mod paths;
pub mod store;

pub use desired_state::{
    DESIRED_STATE_FILE, DESIRED_STATE_SCHEMA_VERSION, DesiredStateDocument, DesiredStateError,
    DesiredStateLoad, DesiredStateLoadSource, DesiredStatePathError, DesiredStateWarning,
    DesiredStateWarningKind, desired_state_dir, desired_state_dir_with, desired_state_file,
    load_desired_state, load_desired_state_from_dir, save_desired_state, save_desired_state_to_dir,
};

pub use store::{
    AppConfig, AutomationConfig, BatteryConfig, ExperimentalConfig, UiConfig, load_or_default,
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
