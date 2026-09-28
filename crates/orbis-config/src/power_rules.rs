//! Power-source rules (AC / battery): what Orbis switches when the charger is
//! plugged or unplugged. Loading is hardware-inert; the worker applies a rule
//! only on an observed change or an explicit user edit.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use orbis_core::profile::PerformanceProfile;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DesiredStatePathError, desired_state_dir};

/// File name inside the desired-state directory.
pub const POWER_RULES_FILE: &str = "power-rules.toml";

/// What to switch for one power source. `None` leaves the setting alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PowerRule {
    /// Performance profile to activate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<PerformanceProfile>,
}

impl PowerRule {
    /// True when the rule changes nothing.
    pub fn is_empty(&self) -> bool {
        self.profile.is_none()
    }
}

/// Rules for both power sources and the master switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PowerRules {
    /// Master switch; off by default.
    #[serde(default)]
    pub enabled: bool,
    /// Applied when the charger is connected.
    #[serde(default)]
    pub ac: PowerRule,
    /// Applied when running on battery.
    #[serde(default)]
    pub battery: PowerRule,
}

impl PowerRules {
    /// Rule for a source (`true` = AC).
    pub fn rule(&self, ac: bool) -> PowerRule {
        if ac { self.ac } else { self.battery }
    }
}

/// Failure to read or write the power rules file.
#[derive(Debug, Error)]
pub enum PowerRulesError {
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
    #[error("invalid power rules file {path:?}: {message}")]
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

/// Load from the production location; a missing file is the disabled default.
pub fn load_power_rules() -> Result<PowerRules, PowerRulesError> {
    load_power_rules_from_dir(&desired_state_dir()?)
}

/// Save to the production location.
pub fn save_power_rules(rules: &PowerRules) -> Result<(), PowerRulesError> {
    save_power_rules_to_dir(rules, &desired_state_dir()?)
}

/// Load from an explicit directory.
pub fn load_power_rules_from_dir(dir: &Path) -> Result<PowerRules, PowerRulesError> {
    let path = dir.join(POWER_RULES_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|error| PowerRulesError::Invalid {
            path,
            message: error.to_string(),
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(PowerRules::default()),
        Err(source) => Err(PowerRulesError::Io {
            operation: "read",
            path,
            source,
        }),
    }
}

/// Atomically save to an explicit directory. An invalid existing file is moved
/// aside to `<name>.invalid` first instead of being silently overwritten.
pub fn save_power_rules_to_dir(rules: &PowerRules, dir: &Path) -> Result<(), PowerRulesError> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| PowerRulesError::Io {
            operation,
            path,
            source,
        }
    };
    let path = dir.join(POWER_RULES_FILE);
    if let Err(PowerRulesError::Invalid { .. }) = load_power_rules_from_dir(dir) {
        let aside = dir.join(format!("{POWER_RULES_FILE}.invalid"));
        fs::rename(&path, &aside).map_err(io("preserve invalid", &path))?;
    }
    let text = toml::to_string_pretty(rules)?;
    fs::create_dir_all(dir).map_err(io("create directory", dir))?;
    let temp = dir.join(format!("{POWER_RULES_FILE}.tmp"));
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
    fn missing_file_is_disabled_and_round_trip_keeps_rules() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load_power_rules_from_dir(dir.path()).unwrap(),
            PowerRules::default()
        );
        let rules = PowerRules {
            enabled: true,
            ac: PowerRule {
                profile: Some(PerformanceProfile::Turbo),
            },
            battery: PowerRule {
                profile: Some(PerformanceProfile::Silent),
            },
        };
        save_power_rules_to_dir(&rules, dir.path()).unwrap();
        assert_eq!(load_power_rules_from_dir(dir.path()).unwrap(), rules);
    }

    #[test]
    fn invalid_file_is_rejected_and_preserved_on_save() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(POWER_RULES_FILE), "enabled = 3").unwrap();
        assert!(matches!(
            load_power_rules_from_dir(dir.path()),
            Err(PowerRulesError::Invalid { .. })
        ));
        save_power_rules_to_dir(&PowerRules::default(), dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("power-rules.toml.invalid")).unwrap(),
            "enabled = 3"
        );
    }
}
