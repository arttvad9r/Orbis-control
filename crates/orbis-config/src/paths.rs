//! XDG-пути и временные каталоги для тестов.

use std::path::PathBuf;

use thiserror::Error;

use crate::CONFIG_DIR_NAME;

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
}
