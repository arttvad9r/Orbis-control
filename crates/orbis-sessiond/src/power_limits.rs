//! Read-only power/thermal limits provider backed by the kernel ASUS Armoury ABI.
//!
//! asusd is deliberately not used here. The authoritative owner is the fixed
//! kernel firmware-attributes sysfs interface. A field is exposed only when
//! current value and all required metadata can be read at runtime.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{PowerLimitProvider, Provider, ProviderHealth};

/// Fixed kernel firmware-attributes root for ASUS Armoury power controls.
pub const ARMOURY_ATTRIBUTES_ROOT: &str = "/sys/class/firmware-attributes/asus-armoury/attributes";

const BACKEND_CANDIDATES: &str = "candidates: kernel ASUS Armoury firmware-attributes (ENODEV); AMD amd_pstate/powercap/hwmon (no package-limit contract); ryzenadj/SMU (read initialization denied); NVIDIA NVML/nvidia-smi (GPU ceiling telemetry, not Dynamic Boost owner); asusd Armoury (fields unavailable)";

#[derive(Debug, Clone)]
struct FieldSpec {
    field: PowerLimitField,
    name: &'static str,
    unit: Unit,
}

const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        field: PowerLimitField::Spl,
        name: "ppt_pl1_spl",
        unit: Unit::Watts,
    },
    FieldSpec {
        field: PowerLimitField::Sppt,
        name: "ppt_pl2_sppt",
        unit: Unit::Watts,
    },
    FieldSpec {
        field: PowerLimitField::Fppt,
        name: "ppt_pl3_fppt",
        unit: Unit::Watts,
    },
    FieldSpec {
        field: PowerLimitField::GpuDynamicBoost,
        name: "nv_dynamic_boost",
        unit: Unit::Watts,
    },
    FieldSpec {
        field: PowerLimitField::GpuTempTarget,
        name: "nv_temp_target",
        unit: Unit::DegreesC,
    },
];

fn attr_path(name: &str, property: &str) -> PathBuf {
    Path::new(ARMOURY_ATTRIBUTES_ROOT).join(name).join(property)
}

fn read_i32(name: &str, property: &str) -> Result<i32, ProviderError> {
    let path = attr_path(name, property);
    let raw = fs::read_to_string(&path).map_err(|error| map_read_error(name, property, error))?;
    raw.trim().parse().map_err(|error| {
        ProviderError::Internal(format!(
            "kernel Armoury {name}/{property} malformed: {error}"
        ))
    })
}

fn map_read_error(name: &str, property: &str, error: io::Error) -> ProviderError {
    let detail = format!("kernel Armoury {name}/{property}: {error}");
    match error.kind() {
        io::ErrorKind::NotFound => ProviderError::Unsupported(detail),
        io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(detail),
        _ if error.raw_os_error() == Some(19) => ProviderError::Unsupported(detail),
        _ => ProviderError::BackendUnavailable(detail),
    }
}

fn read_field(spec: &FieldSpec) -> Result<orbis_core::limits::PowerLimitValue, ProviderError> {
    let value = read_i32(spec.name, "current_value")?;
    let min = read_i32(spec.name, "min_value")?;
    let max = read_i32(spec.name, "max_value")?;
    let step = read_i32(spec.name, "scalar_increment")?;
    let default_raw = match read_i32(spec.name, "default_value") {
        Ok(value) => Some(value),
        Err(ProviderError::Unsupported(_)) => None,
        Err(error) => return Err(error),
    };
    if step <= 0 {
        return Err(ProviderError::Internal(format!(
            "kernel Armoury {} metadata: malformed step {step}",
            spec.name
        )));
    }
    PowerLimitValue::new(
        value,
        min,
        max,
        step,
        default_raw.filter(|value| *value >= 0),
        spec.unit,
    )
    .map_err(|error| {
        ProviderError::Internal(format!("kernel Armoury {} metadata: {error}", spec.name))
    })
}

/// Read-only provider for the kernel's fixed ASUS Armoury attributes.
pub struct KernelAsusPowerLimitProvider;

impl KernelAsusPowerLimitProvider {
    /// Construct the provider without performing I/O.
    pub fn new() -> Self {
        Self
    }
}

impl Default for KernelAsusPowerLimitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for KernelAsusPowerLimitProvider {
    fn id(&self) -> &'static str {
        "kernel-asus-armoury-power-limits"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("kernel/asus-armoury firmware attributes")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!(
            "kernel ASUS Armoury power-limit field '{feature}' unavailable; {BACKEND_CANDIDATES}"
        )
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        let mut entries = vec![
            DiagnosticEntry::new(
                "provider.kernel-asus-armoury-power-limits",
                "read-only kernel ASUS Armoury firmware-attributes power-limit metadata",
            ),
            DiagnosticEntry::new(
                "provider.kernel-asus-armoury-power-limits.candidates",
                BACKEND_CANDIDATES,
            ),
        ];
        entries.extend(orbis_providers::ryzenadj_diagnostics::diagnostics());
        entries
    }
}

#[async_trait]
impl PowerLimitProvider for KernelAsusPowerLimitProvider {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let mut fields = BTreeMap::new();
        let mut first_error = None;
        for spec in FIELDS.iter().cloned() {
            match read_field(&spec) {
                Ok(value) => {
                    fields.insert(spec.field.clone(), value);
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        if fields.is_empty() {
            return Err(ProviderError::Unsupported(format!(
                "kernel ASUS Armoury exposes no power-limit fields; {BACKEND_CANDIDATES}; first probe: {}",
                first_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "no field probe result".into())
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
            "kernel power-limit read provider is read-only".into(),
        ))
    }

    async fn restore_defaults(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "kernel power-limit provider does not expose reset".into(),
        ))
    }

    fn validate_power_limit(&self, _field: &PowerLimitField, _value: i32) -> ValidationResult {
        ValidationResult::invalid("kernel power-limit read provider is read-only")
    }
}

/// Armoury-first selector for kernels exposing both ASUS PPT ABIs.
///
/// A legacy source is a fallback only when Armoury is unavailable. When both
/// sources produce a field, values must match; conflicts are surfaced rather
/// than hidden behind a precedence rule.
pub struct AsusDualPowerLimitProvider {
    armoury: Arc<dyn PowerLimitProvider>,
    legacy: Arc<dyn PowerLimitProvider>,
}

impl AsusDualPowerLimitProvider {
    /// Construct the selector without performing I/O.
    pub fn new(armoury: Arc<dyn PowerLimitProvider>, legacy: Arc<dyn PowerLimitProvider>) -> Self {
        Self { armoury, legacy }
    }
}

impl Provider for AsusDualPowerLimitProvider {
    fn id(&self) -> &'static str {
        "asus-dual-power-limits"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-armoury > asus-nb-wmi (consistency checked)")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("ASUS PPT field '{feature}' unavailable from Armoury or asus-nb-wmi")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-dual-power-limits",
            "Armoury-first selector with exact legacy consistency check",
        )]
    }
}

#[async_trait]
impl PowerLimitProvider for AsusDualPowerLimitProvider {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let primary = self.armoury.power_limits().await;
        let fallback = self.legacy.power_limits().await;
        match (primary, fallback) {
            (Ok(primary), Ok(fallback)) => {
                for (field, value) in &primary.fields {
                    if let Some(other) = fallback.fields.get(field) {
                        if value != other {
                            return Err(ProviderError::Conflict(format!(
                                "ASUS PPT mismatch for {field:?}: Armoury={value:?}, asus-nb-wmi={other:?}"
                            )));
                        }
                    }
                }
                Ok(primary)
            }
            (Ok(primary), Err(_)) => Ok(primary),
            (
                Err(ProviderError::Unsupported(_) | ProviderError::BackendUnavailable(_)),
                Ok(fallback),
            ) => Ok(fallback),
            (Err(primary), Err(_)) => Err(primary),
            (Err(primary), Ok(_)) => Err(primary),
        }
    }

    async fn set_power_limit(
        &self,
        _field: PowerLimitField,
        _value: i32,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session ASUS power selector is read-only".into(),
        ))
    }
    async fn restore_defaults(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session ASUS power selector is read-only".into(),
        ))
    }
    fn validate_power_limit(&self, _field: &PowerLimitField, _value: i32) -> ValidationResult {
        ValidationResult::invalid("session ASUS power selector is read-only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_temperature_limit_and_cpu_boost_have_no_kernel_armoury_field() {
        assert!(
            !FIELDS
                .iter()
                .any(|spec| spec.field == PowerLimitField::CpuTempLimit)
        );
        assert_eq!(
            ARMOURY_ATTRIBUTES_ROOT,
            "/sys/class/firmware-attributes/asus-armoury/attributes"
        );
    }

    #[test]
    fn current_host_does_not_promote_enodev_armoury_fields() {
        let error = map_read_error(
            "ppt_pl1_spl",
            "current_value",
            io::Error::from_raw_os_error(19),
        );
        assert!(matches!(error, ProviderError::Unsupported(_)));
    }

    #[test]
    fn unavailable_diagnostic_names_all_checked_backend_candidates() {
        let provider = KernelAsusPowerLimitProvider::new();
        let reason = provider.explain_unsupported("ppt_pl1_spl");
        for candidate in ["amd_pstate", "powercap", "ryzenadj", "NVML", "asusd"] {
            assert!(reason.contains(candidate), "missing candidate {candidate}");
        }
        assert!(reason.contains("ENODEV"));
    }
}
