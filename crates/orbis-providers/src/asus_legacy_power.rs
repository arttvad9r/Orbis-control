//! Typed read-only adapter for the legacy `asus-nb-wmi` PPT ABI.
//!
//! The legacy driver publishes current PPT values but, on kernels observed by
//! Orbis, does not publish authoritative per-field metadata.  This adapter
//! therefore proves the fixed-path read evidence while refusing to advertise a
//! PowerLimitField until current value and metadata are both available.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[cfg(test)]
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::limits::{PowerLimitField, PowerLimits, Unit};

use crate::error::{ProviderError, ValidationResult};
use crate::traits::{PowerLimitProvider, Provider, ProviderHealth};

/// Fixed production root of the legacy ASUS PPT ABI.
pub const ASUS_NB_WMI_ROOT: &str = "/sys/devices/platform/asus-nb-wmi";

const FIELDS: &[(PowerLimitField, &str, Unit)] = &[
    (PowerLimitField::Spl, "ppt_pl1_spl", Unit::Watts),
    (PowerLimitField::Sppt, "ppt_pl2_sppt", Unit::Watts),
    (PowerLimitField::Fppt, "ppt_fppt", Unit::Watts),
];

/// Fixed-path legacy ASUS PPT source.
pub struct AsusNbWmiPowerLimitProvider {
    root: PathBuf,
}

impl AsusNbWmiPowerLimitProvider {
    /// Construct the provider over the fixed production root.
    pub fn new() -> Self {
        Self {
            root: PathBuf::from(ASUS_NB_WMI_ROOT),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Default for AsusNbWmiPowerLimitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AsusNbWmiPowerLimitProvider {
    fn id(&self) -> &'static str {
        "asus-nb-wmi-power-limits"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-nb-wmi")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!(
            "asus-nb-wmi: {feature} has no readable authoritative metadata (min/max/step/default)"
        )
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-nb-wmi-power-limits",
            "fixed legacy ASUS PPT paths; metadata required",
        )]
    }
}

#[async_trait]
impl PowerLimitProvider for AsusNbWmiPowerLimitProvider {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let mut fields = BTreeMap::new();
        let mut current_seen = Vec::new();
        for (field, name, _unit) in FIELDS {
            let current_path = self.path(name);
            match fs::read_to_string(&current_path) {
                Ok(raw) => {
                    let current = raw.trim().parse::<i32>().map_err(|error| {
                        ProviderError::Internal(format!(
                            "asus-nb-wmi {name} malformed current value: {error}"
                        ))
                    })?;
                    current_seen.push(name);
                    // The legacy ABI has no metadata files. If a future kernel
                    // adds the same standard metadata, consume it without
                    // guessing ranges for today's kernels.
                    let read = |property: &str| -> Result<i32, ProviderError> {
                        let path = self.path(&format!("{name}_{property}"));
                        let raw =
                            fs::read_to_string(&path).map_err(|error| match error.kind() {
                                std::io::ErrorKind::NotFound => {
                                    ProviderError::Unsupported(format!(
                                        "asus-nb-wmi {name}: metadata absent ({})",
                                        path.display()
                                    ))
                                }
                                std::io::ErrorKind::PermissionDenied => {
                                    ProviderError::PermissionDenied(format!(
                                        "asus-nb-wmi {name}: metadata denied ({})",
                                        path.display()
                                    ))
                                }
                                _ => ProviderError::Io(error),
                            })?;
                        raw.trim().parse().map_err(|error| {
                            ProviderError::Internal(format!(
                                "asus-nb-wmi {name}/{property} malformed: {error}"
                            ))
                        })
                    };
                    let min = match read("min_value") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let max = match read("max_value") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let step = match read("scalar_increment") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let default = read("default_value").ok();
                    let value = orbis_core::limits::PowerLimitValue::new(
                        current, min, max, step, default, *_unit,
                    )
                    .map_err(|error| {
                        ProviderError::Internal(format!("asus-nb-wmi {name}: {error}"))
                    })?;
                    fields.insert(field.clone(), value);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err(ProviderError::PermissionDenied(format!(
                        "asus-nb-wmi read denied: {}",
                        current_path.display()
                    )));
                }
                Err(error) => return Err(ProviderError::Io(error)),
            }
        }
        if fields.is_empty() {
            return Err(ProviderError::Unsupported(format!(
                "asus-nb-wmi PPT current paths seen: {:?}, but no field has complete current+metadata evidence",
                current_seen
            )));
        }
        Ok(PowerLimits { fields })
    }

    async fn set_power_limit(
        &self,
        _field: PowerLimitField,
        _value: i32,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "asus-nb-wmi PPT adapter is read-only until metadata contract is present".into(),
        ))
    }
    async fn restore_defaults(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "asus-nb-wmi PPT reset is not a typed Orbis contract".into(),
        ))
    }
    fn validate_power_limit(&self, _field: &PowerLimitField, _value: i32) -> ValidationResult {
        ValidationResult::invalid("asus-nb-wmi PPT metadata is unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn current_without_metadata_is_not_advertised() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ppt_pl1_spl"), "5\n").unwrap();
        let result = AsusNbWmiPowerLimitProvider::with_root(dir.path())
            .power_limits()
            .await;
        assert!(matches!(result, Err(ProviderError::Unsupported(_))));
    }

    #[tokio::test]
    async fn complete_metadata_is_typed() {
        let dir = tempfile::tempdir().unwrap();
        for (name, value) in [
            ("ppt_pl1_spl", "45"),
            ("ppt_pl1_spl_min_value", "20"),
            ("ppt_pl1_spl_max_value", "80"),
            ("ppt_pl1_spl_scalar_increment", "5"),
            ("ppt_pl1_spl_default_value", "45"),
        ] {
            fs::write(dir.path().join(name), format!("{value}\n")).unwrap();
        }
        let limits = AsusNbWmiPowerLimitProvider::with_root(dir.path())
            .power_limits()
            .await
            .unwrap();
        assert_eq!(limits.get(&PowerLimitField::Spl).unwrap().value, 45);
    }

    #[test]
    fn production_path_is_fixed() {
        assert_eq!(
            Path::new(ASUS_NB_WMI_ROOT).to_string_lossy(),
            "/sys/devices/platform/asus-nb-wmi"
        );
    }
}
