//! XDG-пути и временные каталоги для тестов.

use std::path::PathBuf;

use thiserror::Error;

use crate::{CONFIG_DIR_NAME, config_dir};

/// Имя файла конфигурации.
pub const CONFIG_FILE: &str = "config.toml";

/// Имя каталога состояния в XDG.
pub const STATE_DIR_NAME: &str = "orbis-control";

/// Имя каталога кэша в XDG.
pub const CACHE_DIR_NAME: &str = "orbis-control";

/// Typed failure for legacy/general XDG path resolution.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PathResolutionError {
    /// Explicit XDG path was empty or relative.
    #[error("invalid {key}: {path:?}; expected a non-empty absolute path")]
    InvalidXdg {
        /// Environment variable name.
        key: &'static str,
        /// Rejected value.
        path: PathBuf,
    },
    /// No home directory was available for the fallback.
    #[error("cannot resolve XDG path: no home directory is available")]
    MissingHome,
    /// Home fallback was empty or relative.
    #[error("invalid home directory for XDG fallback: {0:?}; expected a non-empty absolute path")]
    InvalidHome(PathBuf),
}

fn resolve_dir_checked_with(
    env_key: &'static str,
    xdg_home: Option<PathBuf>,
    home: Option<PathBuf>,
    fallback_subdir: &str,
) -> Result<PathBuf, PathResolutionError> {
    if let Some(xdg) = xdg_home {
        if xdg.as_os_str().is_empty() || !xdg.is_absolute() {
            return Err(PathResolutionError::InvalidXdg {
                key: env_key,
                path: xdg,
            });
        }
        return Ok(xdg.join(CONFIG_DIR_NAME));
    }

    match home {
        Some(home) if !home.as_os_str().is_empty() && home.is_absolute() => {
            Ok(home.join(fallback_subdir).join(CONFIG_DIR_NAME))
        }
        Some(home) => Err(PathResolutionError::InvalidHome(home)),
        None => Err(PathResolutionError::MissingHome),
    }
}

fn resolve_dir_checked(
    env_key: &'static str,
    fallback_subdir: &str,
) -> Result<PathBuf, PathResolutionError> {
    resolve_dir_checked_with(
        env_key,
        std::env::var_os(env_key).map(PathBuf::from),
        dirs::home_dir(),
        fallback_subdir,
    )
}

/// Fail-closed config directory resolver for legacy migration/compatibility code.
pub fn config_dir_checked() -> Result<PathBuf, PathResolutionError> {
    resolve_dir_checked("XDG_CONFIG_HOME", ".config")
}

/// Fail-closed state directory resolver for new production consumers.
pub fn state_dir_checked() -> Result<PathBuf, PathResolutionError> {
    resolve_dir_checked("XDG_STATE_HOME", ".local/state")
}

/// Fail-closed cache directory resolver for new production consumers.
pub fn cache_dir_checked() -> Result<PathBuf, PathResolutionError> {
    resolve_dir_checked("XDG_CACHE_HOME", ".cache")
}

/// Pure fail-closed config directory resolver for tests/compatibility migration.
pub fn config_dir_with_checked(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, PathResolutionError> {
    resolve_dir_checked_with("XDG_CONFIG_HOME", xdg_config_home, home, ".config")
}

/// Legacy resolver retained only for compatibility.
fn resolve_dir_legacy(env_key: &str, fallback_subdir: &str) -> PathBuf {
    std::env::var_os(env_key)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(fallback_subdir))
                .unwrap_or_else(|| PathBuf::from("."))
        })
        .join(CONFIG_DIR_NAME)
}

/// Полный путь к legacy-файлу конфигурации.
#[deprecated(note = "legacy compatibility path; new production code must use hardened stores")]
pub fn config_file() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

/// Legacy state path with historical CWD fallback.
///
/// New production code must use [`state_dir_checked`].
#[deprecated(note = "use state_dir_checked(); legacy function may fall back to CWD")]
pub fn state_dir() -> PathBuf {
    resolve_dir_legacy("XDG_STATE_HOME", ".local/state")
}

/// Legacy cache path with historical CWD fallback.
///
/// New production code must use [`cache_dir_checked`].
#[deprecated(note = "use cache_dir_checked(); legacy function may fall back to CWD")]
pub fn cache_dir() -> PathBuf {
    resolve_dir_legacy("XDG_CACHE_HOME", ".cache")
}

/// Legacy pure config resolver retained for compatibility tests.
///
/// New code must use [`config_dir_with_checked`].
#[deprecated(note = "use config_dir_with_checked(); legacy function may fall back to CWD")]
pub fn config_dir_with(xdg: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    xdg.unwrap_or_else(|| home.unwrap_or_else(|| PathBuf::from(".")).join(".config"))
        .join(CONFIG_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_paths_from_explicit_dirs() {
        let td = tempfile::tempdir().unwrap();
        let cfg = td.path().join("cfg");
        assert_eq!(
            config_dir_with_checked(Some(cfg.clone()), None).unwrap(),
            cfg.join("orbis-control")
        );
    }

    #[test]
    fn checked_fallback_to_home() {
        let home = PathBuf::from("/tmp/fake-home");
        assert_eq!(
            config_dir_with_checked(None, Some(home.clone())).unwrap(),
            home.join(".config").join("orbis-control")
        );
    }

    #[test]
    fn checked_paths_reject_missing_or_relative_bases() {
        assert_eq!(
            config_dir_with_checked(None, None),
            Err(PathResolutionError::MissingHome)
        );
        assert!(matches!(
            config_dir_with_checked(Some(PathBuf::from("relative")), None),
            Err(PathResolutionError::InvalidXdg { .. })
        ));
        assert!(matches!(
            config_dir_with_checked(None, Some(PathBuf::from("relative-home"))),
            Err(PathResolutionError::InvalidHome(_))
        ));
    }

    #[test]
    #[allow(deprecated)]
    fn legacy_config_file_joins_dir() {
        let td = tempfile::tempdir().unwrap();
        let cfg = td.path().join("cfg");
        let dir = config_dir_with(Some(cfg), None);
        assert_eq!(dir.join(CONFIG_FILE).file_name().unwrap(), CONFIG_FILE);
    }
}
