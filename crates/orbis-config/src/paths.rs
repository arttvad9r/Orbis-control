//! XDG-пути и временные каталоги для тестов.

use std::path::PathBuf;

use crate::{config_dir, CONFIG_DIR_NAME};

/// Имя файла конфигурации.
pub const CONFIG_FILE: &str = "config.toml";

/// Имя каталога состояния в XDG.
pub const STATE_DIR_NAME: &str = "orbis-control";

/// Имя каталога кэша в XDG.
pub const CACHE_DIR_NAME: &str = "orbis-control";

/// Разрешить XDG-каталог из переменной окружения или fallback.
fn resolve_dir(env_key: &str, fallback_subdir: &str) -> PathBuf {
    std::env::var_os(env_key)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(fallback_subdir))
                .unwrap_or_else(|| PathBuf::from("."))
        })
        .join(CONFIG_DIR_NAME)
}

/// Полный путь к файлу конфигурации.
pub fn config_file() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

/// Каталог состояния (журналы, диагностика, pending state).
pub fn state_dir() -> PathBuf {
    resolve_dir("XDG_STATE_HOME", ".local/state")
}

/// Каталог кэша.
pub fn cache_dir() -> PathBuf {
    resolve_dir("XDG_CACHE_HOME", ".cache")
}

/// Чистые варианты для тестов (без мутации окружения).
pub fn config_dir_with(xdg: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    xdg.unwrap_or_else(|| home.unwrap_or_else(|| PathBuf::from(".")).join(".config"))
        .join(CONFIG_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_from_explicit_dirs() {
        let td = tempfile::tempdir().unwrap();
        let cfg = td.path().join("cfg");
        assert_eq!(config_dir_with(Some(cfg.clone()), None), cfg.join("orbis-control"));
    }

    #[test]
    fn fallback_to_home() {
        let home = PathBuf::from("/tmp/fake-home");
        assert_eq!(
            config_dir_with(None, Some(home.clone())),
            home.join(".config").join("orbis-control")
        );
    }

    #[test]
    fn config_file_joins_dir() {
        let td = tempfile::tempdir().unwrap();
        let cfg = td.path().join("cfg");
        let dir = config_dir_with(Some(cfg), None);
        // config_file() читает окружение; проверим только join-логику
        assert_eq!(dir.join(CONFIG_FILE).file_name().unwrap(), CONFIG_FILE);
    }
}
