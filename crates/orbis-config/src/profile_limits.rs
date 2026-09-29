//! Per-profile power-limit intent (G-Helper style `limit_*_N` + auto-apply).
//!
//! Only values the user explicitly applied are stored; enabling auto-apply never
//! invents values from an observation. Loading is hardware-inert.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use orbis_core::cpu_tuning::EnergyPreference;
use orbis_core::limits::PowerLimitField;
use orbis_core::nvidia_tuning::NvidiaField;
use orbis_core::profile::PerformanceProfile;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DesiredStatePathError, desired_state_dir};

/// File name inside the desired-state directory.
pub const PROFILE_LIMITS_FILE: &str = "profile-limits.toml";

/// Stored intent for one performance profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileLimitSet {
    /// Re-apply the stored values whenever this profile becomes active.
    #[serde(default)]
    pub auto_apply: bool,
    /// Explicitly applied values keyed by [`limit_key`].
    #[serde(default)]
    pub values: BTreeMap<String, i32>,
    /// Explicitly applied CPU energy preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epp: Option<EnergyPreference>,
    /// Explicitly applied CPU boost state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_boost: Option<bool>,
    /// Explicitly applied NVIDIA graphics clock offset (MHz).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nvidia_core: Option<i32>,
    /// Explicitly applied NVIDIA memory clock offset (MHz).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nvidia_memory: Option<i32>,
    /// Explicitly applied NVIDIA power limit (W).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nvidia_power: Option<i32>,
    /// Explicitly applied AMD all-core Curve Optimizer offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_optimizer: Option<i32>,
}

impl ProfileLimitSet {
    fn nvidia_slot(&mut self, field: NvidiaField) -> &mut Option<i32> {
        match field {
            NvidiaField::CoreOffset => &mut self.nvidia_core,
            NvidiaField::MemoryOffset => &mut self.nvidia_memory,
            NvidiaField::PowerLimit => &mut self.nvidia_power,
        }
    }

    /// Stored NVIDIA value for a field, if any.
    pub fn nvidia(&self, field: NvidiaField) -> Option<i32> {
        match field {
            NvidiaField::CoreOffset => self.nvidia_core,
            NvidiaField::MemoryOffset => self.nvidia_memory,
            NvidiaField::PowerLimit => self.nvidia_power,
        }
    }

    /// Remember an explicitly applied NVIDIA value.
    pub fn set_nvidia(&mut self, field: NvidiaField, value: i32) {
        *self.nvidia_slot(field) = Some(value);
    }

    /// Stored NVIDIA (field, value) pairs in display order.
    pub fn nvidia_entries(&self) -> Vec<(NvidiaField, i32)> {
        NvidiaField::ALL
            .into_iter()
            .filter_map(|field| self.nvidia(field).map(|value| (field, value)))
            .collect()
    }

    /// Stored value for a field, if any.
    pub fn value(&self, field: &PowerLimitField) -> Option<i32> {
        limit_key(field).and_then(|key| self.values.get(key).copied())
    }

    /// Stored (field, value) pairs in stable order.
    pub fn entries(&self) -> Vec<(PowerLimitField, i32)> {
        KNOWN_FIELDS
            .iter()
            .filter_map(|(key, field)| self.values.get(*key).map(|value| (field.clone(), *value)))
            .collect()
    }
}

/// Stored intent for all profiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileLimits {
    /// Silent profile.
    #[serde(default)]
    pub silent: ProfileLimitSet,
    /// Balanced profile.
    #[serde(default)]
    pub balanced: ProfileLimitSet,
    /// Turbo profile.
    #[serde(default)]
    pub turbo: ProfileLimitSet,
}

impl ProfileLimits {
    /// Set for a profile.
    pub fn get(&self, profile: PerformanceProfile) -> &ProfileLimitSet {
        match profile {
            PerformanceProfile::Silent => &self.silent,
            PerformanceProfile::Balanced => &self.balanced,
            PerformanceProfile::Turbo => &self.turbo,
        }
    }

    /// Mutable set for a profile.
    pub fn get_mut(&mut self, profile: PerformanceProfile) -> &mut ProfileLimitSet {
        match profile {
            PerformanceProfile::Silent => &mut self.silent,
            PerformanceProfile::Balanced => &mut self.balanced,
            PerformanceProfile::Turbo => &mut self.turbo,
        }
    }
}

const KNOWN_FIELDS: [(&str, PowerLimitField); 6] = [
    ("spl", PowerLimitField::Spl),
    ("sppt", PowerLimitField::Sppt),
    ("fppt", PowerLimitField::Fppt),
    ("cpu_temp_limit", PowerLimitField::CpuTempLimit),
    ("gpu_dynamic_boost", PowerLimitField::GpuDynamicBoost),
    ("gpu_temp_target", PowerLimitField::GpuTempTarget),
];

/// Stable storage key for a known field; `None` for backend-specific fields.
pub fn limit_key(field: &PowerLimitField) -> Option<&'static str> {
    KNOWN_FIELDS
        .iter()
        .find(|(_, known)| known == field)
        .map(|(key, _)| *key)
}

/// Failure to read or write the per-profile limits file.
#[derive(Debug, Error)]
pub enum ProfileLimitsError {
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
    #[error("invalid profile limits file {path:?}: {message}")]
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

/// Load from the production location; a missing file is the empty default.
pub fn load_profile_limits() -> Result<ProfileLimits, ProfileLimitsError> {
    load_profile_limits_from_dir(&desired_state_dir()?)
}

/// Save to the production location.
pub fn save_profile_limits(limits: &ProfileLimits) -> Result<(), ProfileLimitsError> {
    save_profile_limits_to_dir(limits, &desired_state_dir()?)
}

/// Load from an explicit directory.
pub fn load_profile_limits_from_dir(dir: &Path) -> Result<ProfileLimits, ProfileLimitsError> {
    let path = dir.join(PROFILE_LIMITS_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|error| ProfileLimitsError::Invalid {
            path,
            message: error.to_string(),
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(ProfileLimits::default())
        }
        Err(source) => Err(ProfileLimitsError::Io {
            operation: "read",
            path,
            source,
        }),
    }
}

/// Atomically save to an explicit directory. An invalid existing file is moved
/// aside to `<name>.invalid` first instead of being silently overwritten.
pub fn save_profile_limits_to_dir(
    limits: &ProfileLimits,
    dir: &Path,
) -> Result<(), ProfileLimitsError> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| ProfileLimitsError::Io {
            operation,
            path,
            source,
        }
    };
    let path = dir.join(PROFILE_LIMITS_FILE);
    if let Err(ProfileLimitsError::Invalid { .. }) = load_profile_limits_from_dir(dir) {
        let aside = dir.join(format!("{PROFILE_LIMITS_FILE}.invalid"));
        fs::rename(&path, &aside).map_err(io("preserve invalid", &path))?;
    }
    let text = toml::to_string_pretty(limits)?;
    fs::create_dir_all(dir).map_err(io("create directory", dir))?;
    let temp = dir.join(format!("{PROFILE_LIMITS_FILE}.tmp"));
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
    fn missing_file_is_default_and_round_trip_keeps_values() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load_profile_limits_from_dir(dir.path()).unwrap(),
            ProfileLimits::default()
        );
        let mut limits = ProfileLimits::default();
        let turbo = limits.get_mut(PerformanceProfile::Turbo);
        turbo.auto_apply = true;
        turbo.values.insert("spl".into(), 80);
        turbo.values.insert("gpu_temp_target".into(), 80);
        turbo.epp = Some(EnergyPreference::BalancePerformance);
        turbo.cpu_boost = Some(false);
        turbo.set_nvidia(NvidiaField::MemoryOffset, 1800);
        save_profile_limits_to_dir(&limits, dir.path()).unwrap();
        let loaded = load_profile_limits_from_dir(dir.path()).unwrap();
        assert_eq!(loaded, limits);
        assert_eq!(
            loaded.turbo.entries(),
            vec![
                (PowerLimitField::Spl, 80),
                (PowerLimitField::GpuTempTarget, 80)
            ]
        );
        assert_eq!(
            loaded.turbo.nvidia_entries(),
            vec![(NvidiaField::MemoryOffset, 1800)]
        );
        assert!(!loaded.silent.auto_apply);
    }

    #[test]
    fn invalid_file_is_rejected_and_preserved_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROFILE_LIMITS_FILE);
        fs::write(&path, "silent = 3").unwrap();
        assert!(matches!(
            load_profile_limits_from_dir(dir.path()),
            Err(ProfileLimitsError::Invalid { .. })
        ));
        save_profile_limits_to_dir(&ProfileLimits::default(), dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("profile-limits.toml.invalid")).unwrap(),
            "silent = 3"
        );
        assert!(load_profile_limits_from_dir(dir.path()).is_ok());
    }

    #[test]
    fn backend_specific_fields_have_no_key() {
        assert_eq!(limit_key(&PowerLimitField::Other("x".into())), None);
    }
}
