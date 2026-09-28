//! Typed adapter for the legacy `asus-nb-wmi` PPT ABI.
//!
//! The legacy driver publishes the last value written since boot but no
//! min/max/step metadata, and its initial value is an uninitialised floor.
//! Ranges therefore come from an explicit table of models whose limits were
//! verified by their DMI product token; models outside the table are reported
//! as unsupported instead of being guessed.  A successful write is confirmed by
//! the driver cache, which only updates after the firmware acknowledged the
//! WMI call.

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
/// Fixed DMI product-name source used to select the verified range table.
pub const DMI_PRODUCT_NAME_PATH: &str = "/sys/class/dmi/id/product_name";

/// Every legacy attribute Orbis may write: field, file name, unit.
pub const LEGACY_PPT_FIELDS: &[(PowerLimitField, &str, Unit)] = &[
    (PowerLimitField::Spl, "ppt_pl1_spl", Unit::Watts),
    (PowerLimitField::Sppt, "ppt_pl2_sppt", Unit::Watts),
    (PowerLimitField::Fppt, "ppt_fppt", Unit::Watts),
    (
        PowerLimitField::GpuDynamicBoost,
        "nv_dynamic_boost",
        Unit::Watts,
    ),
    (
        PowerLimitField::GpuTempTarget,
        "nv_temp_target",
        Unit::DegreesC,
    ),
];

/// Verified inclusive ranges for one model.
struct ModelBounds {
    /// Exact alphanumeric token of the DMI product name.
    product_token: &'static str,
    total_power: (i32, i32),
    gpu_boost: (i32, i32),
    gpu_temp: (i32, i32),
}

/// Ranges follow G-Helper's limits for this platform (5 W is its firmware
/// floor, treated there as "unset"); the lower bound here is a safe 15 W.
const VERIFIED_MODELS: &[ModelBounds] = &[ModelBounds {
    product_token: "FA707NV",
    total_power: (15, 90),
    gpu_boost: (5, 25),
    gpu_temp: (75, 87),
}];

fn product_tokens(product: &str) -> impl Iterator<Item = &str> {
    product
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
}

/// Verified `(min, max)` for a legacy attribute on the given DMI product name.
pub fn legacy_ppt_bounds(product: &str, field: &PowerLimitField) -> Option<(i32, i32)> {
    let model = VERIFIED_MODELS.iter().find(|model| {
        product_tokens(product).any(|token| token.eq_ignore_ascii_case(model.product_token))
    })?;
    match field {
        PowerLimitField::Spl | PowerLimitField::Sppt | PowerLimitField::Fppt => {
            Some(model.total_power)
        }
        PowerLimitField::GpuDynamicBoost => Some(model.gpu_boost),
        PowerLimitField::GpuTempTarget => Some(model.gpu_temp),
        _ => None,
    }
}

/// Fixed attribute file name for a legacy field.
pub fn legacy_ppt_file(field: &PowerLimitField) -> Option<&'static str> {
    LEGACY_PPT_FIELDS
        .iter()
        .find(|(candidate, _, _)| candidate == field)
        .map(|(_, name, _)| *name)
}

/// Read the DMI product name from the fixed path.
pub fn read_dmi_product_name() -> Option<String> {
    fs::read_to_string(DMI_PRODUCT_NAME_PATH)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Fixed-path legacy ASUS PPT source.
pub struct AsusNbWmiPowerLimitProvider {
    root: PathBuf,
    product: Option<String>,
}

impl AsusNbWmiPowerLimitProvider {
    /// Construct the provider over the fixed production root.
    pub fn new() -> Self {
        Self {
            root: PathBuf::from(ASUS_NB_WMI_ROOT),
            product: read_dmi_product_name(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: impl Into<PathBuf>, product: Option<&str>) -> Self {
        Self {
            root: root.into(),
            product: product.map(str::to_string),
        }
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
        format!("asus-nb-wmi: {feature} has no verified range table entry for this model")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-nb-wmi-power-limits",
            "fixed legacy ASUS PPT paths; ranges from the verified per-model table",
        )]
    }
}

#[async_trait]
impl PowerLimitProvider for AsusNbWmiPowerLimitProvider {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let mut fields = BTreeMap::new();
        let mut current_seen = Vec::new();
        for (field, name, unit) in LEGACY_PPT_FIELDS {
            let current_path = self.path(name);
            match fs::read_to_string(&current_path) {
                Ok(raw) => {
                    let current = raw.trim().parse::<i32>().map_err(|error| {
                        ProviderError::Internal(format!(
                            "asus-nb-wmi {name} malformed current value: {error}"
                        ))
                    })?;
                    current_seen.push(*name);
                    let Some((min, max)) = self
                        .product
                        .as_deref()
                        .and_then(|product| legacy_ppt_bounds(product, field))
                    else {
                        continue;
                    };
                    // The driver cache holds an uninitialised floor until the
                    // first write; show the range floor instead of failing.
                    let shown = if (min..=max).contains(&current) {
                        current
                    } else {
                        min
                    };
                    let value =
                        orbis_core::limits::PowerLimitValue::new(shown, min, max, 1, None, *unit)
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
                "asus-nb-wmi PPT paths seen: {current_seen:?}, but this model has no verified range table entry"
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
            "asus-nb-wmi PPT writes belong to the privileged Hardware1 service".into(),
        ))
    }
    async fn restore_defaults(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "asus-nb-wmi PPT reset is not a typed Orbis contract".into(),
        ))
    }
    fn validate_power_limit(&self, field: &PowerLimitField, value: i32) -> ValidationResult {
        match self
            .product
            .as_deref()
            .and_then(|product| legacy_ppt_bounds(product, field))
        {
            Some((min, max)) if (min..=max).contains(&value) => ValidationResult::ok(),
            Some((min, max)) => {
                ValidationResult::invalid(format!("asus-nb-wmi: {value} outside {min}..{max}"))
            }
            None => ValidationResult::invalid("asus-nb-wmi: no verified range for this model"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const PRODUCT: &str = "ASUS TUF Gaming A17 FA707NV_FA707NV";

    #[tokio::test]
    async fn unknown_model_is_not_advertised() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ppt_pl1_spl"), "45\n").unwrap();
        let result = AsusNbWmiPowerLimitProvider::with_root(dir.path(), Some("ROG Something X1"))
            .power_limits()
            .await;
        assert!(matches!(result, Err(ProviderError::Unsupported(_))));
        let no_dmi = AsusNbWmiPowerLimitProvider::with_root(dir.path(), None)
            .power_limits()
            .await;
        assert!(matches!(no_dmi, Err(ProviderError::Unsupported(_))));
    }

    #[tokio::test]
    async fn verified_model_gets_table_ranges_and_floor_is_not_shown_as_current() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ppt_pl1_spl"), "5\n").unwrap();
        fs::write(dir.path().join("ppt_pl2_sppt"), "60\n").unwrap();
        fs::write(dir.path().join("nv_temp_target"), "75\n").unwrap();
        let limits = AsusNbWmiPowerLimitProvider::with_root(dir.path(), Some(PRODUCT))
            .power_limits()
            .await
            .unwrap();
        let spl = limits.get(&PowerLimitField::Spl).unwrap();
        assert_eq!((spl.value, spl.min, spl.max), (15, 15, 90));
        assert_eq!(limits.get(&PowerLimitField::Sppt).unwrap().value, 60);
        let temp = limits.get(&PowerLimitField::GpuTempTarget).unwrap();
        assert_eq!((temp.min, temp.max, temp.unit), (75, 87, Unit::DegreesC));
        assert!(limits.get(&PowerLimitField::Fppt).is_none());
    }

    #[test]
    fn model_match_is_an_exact_product_token() {
        assert!(legacy_ppt_bounds(PRODUCT, &PowerLimitField::Spl).is_some());
        assert!(legacy_ppt_bounds("FA707NVR", &PowerLimitField::Spl).is_none());
        assert!(legacy_ppt_bounds(PRODUCT, &PowerLimitField::CpuTempLimit).is_none());
    }

    #[test]
    fn production_path_is_fixed() {
        assert_eq!(
            Path::new(ASUS_NB_WMI_ROOT).to_string_lossy(),
            "/sys/devices/platform/asus-nb-wmi"
        );
    }
}
