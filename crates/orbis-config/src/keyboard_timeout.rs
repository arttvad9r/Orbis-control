//! Keyboard backlight auto-off timeout per power source. Loading is
//! hardware-inert; the session runtime only dims after observed idleness.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DesiredStatePathError, desired_state_dir};

/// File name inside the desired-state directory.
pub const KEYBOARD_TIMEOUT_FILE: &str = "keyboard-timeout.toml";

/// Seconds of inactivity before the backlight is switched off; `None` never.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct KeyboardTimeout {
    /// While the charger is connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ac_secs: Option<u32>,
    /// While running on battery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_secs: Option<u32>,
}

impl KeyboardTimeout {
    /// Timeout for a power source (`true` = AC).
    pub fn for_source(&self, ac: bool) -> Option<u32> {
        if ac { self.ac_secs } else { self.battery_secs }
    }
}

/// Failure to read or write the keyboard timeout file.
#[derive(Debug, Error)]
pub enum KeyboardTimeoutError {
    /// Directory resolution failed.
    #[error(transparent)]
    Path(#[from] DesiredStatePathError),
    /// Filesystem failure.
    #[error("{operation} {path:?}: {source}")]
    Io {
        /// Failed operation.
        operation: &'static str,
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The stored file is not valid; it was left untouched.
    #[error("invalid keyboard timeout file {path:?}: {message}")]
    Invalid {
        /// Path of the invalid file.
        path: PathBuf,
        /// Parser message.
        message: String,
    },
    /// Serialisation failed.
    #[error(transparent)]
    Serialize(#[from] toml::ser::Error),
}

/// Load from the production location; a missing file means "never".
pub fn load_keyboard_timeout() -> Result<KeyboardTimeout, KeyboardTimeoutError> {
    load_keyboard_timeout_from_dir(&desired_state_dir()?)
}

/// Save to the production location.
pub fn save_keyboard_timeout(timeout: &KeyboardTimeout) -> Result<(), KeyboardTimeoutError> {
    save_keyboard_timeout_to_dir(timeout, &desired_state_dir()?)
}

/// Load from an explicit directory.
pub fn load_keyboard_timeout_from_dir(dir: &Path) -> Result<KeyboardTimeout, KeyboardTimeoutError> {
    let path = dir.join(KEYBOARD_TIMEOUT_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|error| KeyboardTimeoutError::Invalid {
            path,
            message: error.to_string(),
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(KeyboardTimeout::default())
        }
        Err(source) => Err(KeyboardTimeoutError::Io {
            operation: "read",
            path,
            source,
        }),
    }
}

/// Atomically save to an explicit directory. An invalid existing file is moved
/// aside to `<name>.invalid` first instead of being silently overwritten.
pub fn save_keyboard_timeout_to_dir(
    timeout: &KeyboardTimeout,
    dir: &Path,
) -> Result<(), KeyboardTimeoutError> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| KeyboardTimeoutError::Io {
            operation,
            path,
            source,
        }
    };
    let path = dir.join(KEYBOARD_TIMEOUT_FILE);
    if let Err(KeyboardTimeoutError::Invalid { .. }) = load_keyboard_timeout_from_dir(dir) {
        let aside = dir.join(format!("{KEYBOARD_TIMEOUT_FILE}.invalid"));
        fs::rename(&path, &aside).map_err(io("preserve invalid", &path))?;
    }
    let text = toml::to_string_pretty(timeout)?;
    fs::create_dir_all(dir).map_err(io("create directory", dir))?;
    let temp = dir.join(format!("{KEYBOARD_TIMEOUT_FILE}.tmp"));
    let mut file = fs::File::create(&temp).map_err(io("create temp", &temp))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(io("write temp", &temp))?;
    fs::rename(&temp, &path).map_err(io("replace", &path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_means_never_and_round_trip_keeps_values() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load_keyboard_timeout_from_dir(dir.path()).unwrap(),
            KeyboardTimeout::default()
        );
        let timeout = KeyboardTimeout {
            ac_secs: None,
            battery_secs: Some(30),
        };
        save_keyboard_timeout_to_dir(&timeout, dir.path()).unwrap();
        let loaded = load_keyboard_timeout_from_dir(dir.path()).unwrap();
        assert_eq!(loaded, timeout);
        assert_eq!(loaded.for_source(true), None);
        assert_eq!(loaded.for_source(false), Some(30));
    }

    #[test]
    fn invalid_file_is_rejected_and_preserved_on_save() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(KEYBOARD_TIMEOUT_FILE), "ac_secs = \"x\"").unwrap();
        assert!(matches!(
            load_keyboard_timeout_from_dir(dir.path()),
            Err(KeyboardTimeoutError::Invalid { .. })
        ));
        save_keyboard_timeout_to_dir(&KeyboardTimeout::default(), dir.path()).unwrap();
        assert!(dir.path().join("keyboard-timeout.toml.invalid").exists());
    }
}
