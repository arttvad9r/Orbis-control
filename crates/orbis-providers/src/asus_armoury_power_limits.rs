use std::fs;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use orbis_capabilities::probe::{ProbeClassification, ProbeContext, ProbeOperationResult};
use orbis_core::capability::{Capability, CapabilityConstraints, CapabilityOperations};
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};

use crate::error::ProviderError;
use crate::traits::{PowerLimitProvider, Provider, ProviderHealth};

/// Fixed ASUS Armoury CPU package-limit attribute directories.
pub const ASUS_ARMOURY_POWER_LIMITS: &[(PowerLimitField, &str)] = &[
    (PowerLimitField::Spl, "ppt_pl1_spl"),
    (PowerLimitField::Sppt, "ppt_pl2_sppt"),
    (PowerLimitField::Fppt, "ppt_fppt"),
];

/// Read-only provider for the three ASUS Armoury CPU package limits.
pub struct AsusArmouryPowerLimitProvider {
    sysfs_root: PathBuf,
}

impl AsusArmouryPowerLimitProvider {
    /// Create a provider rooted at `/sys` or a fixture tree.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    fn attribute_dir(&self, relative: &str) -> PathBuf {
        self.sysfs_root
            .join("class/firmware-attributes/asus-armoury/attributes")
            .join(relative)
    }
}

impl Default for AsusArmouryPowerLimitProvider {
    fn default() -> Self {
        Self::new("/sys")
    }
}

impl Provider for AsusArmouryPowerLimitProvider {
    fn id(&self) -> &'static str {
        "asus-armoury-cpu-package-limits"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-armoury")
    }
    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("asus-armoury: функция '{feature}' недоступна")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-armoury-cpu-package-limits",
            "read-only ASUS Armoury CPU package-limit metadata",
        )]
    }
}

pub(crate) fn map_power_limit_error(
    path: &std::path::Path,
    error: std::io::Error,
) -> ProviderError {
    if error.raw_os_error() == Some(19) {
        return ProviderError::BackendUnavailable(format!(
            "asus-armoury ENODEV: {}",
            path.display()
        ));
    }
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus-armoury power-limit attribute absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury power-limit read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

fn read_required(dir: &std::path::Path, name: &str) -> Result<String, ProviderError> {
    let path = dir.join(name);
    fs::read_to_string(&path).map_err(|error| map_power_limit_error(&path, error))
}

fn read_integer(dir: &std::path::Path, name: &str) -> Result<i32, ProviderError> {
    let raw = read_required(dir, name)?;
    raw.trim()
        .parse()
        .map_err(|_| ProviderError::Internal(format!("asus-armoury malformed {name}: {raw:?}")))
}

fn read_optional_integer(dir: &std::path::Path, name: &str) -> Result<Option<i32>, ProviderError> {
    let path = dir.join(name);
    match fs::read_to_string(&path) {
        Ok(raw) => raw.trim().parse().map(Some).map_err(|_| {
            ProviderError::Internal(format!("asus-armoury malformed {name}: {raw:?}"))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(map_power_limit_error(&path, error)),
    }
}

#[async_trait]
impl PowerLimitProvider for AsusArmouryPowerLimitProvider {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let mut fields = std::collections::BTreeMap::new();
        for (field, relative) in ASUS_ARMOURY_POWER_LIMITS {
            let dir = self.attribute_dir(relative);
            let value = read_integer(&dir, "current_value")?;
            let min = read_integer(&dir, "min_value")?;
            let max = read_integer(&dir, "max_value")?;
            if read_required(&dir, "type")?.trim() != "integer" || min > max {
                return Err(ProviderError::Internal(format!(
                    "asus-armoury inconsistent metadata for {relative}"
                )));
            }
            let default = read_optional_integer(&dir, "default_value")?;
            let value = PowerLimitValue::new(value, min, max, 1, default, Unit::Watts)
                .map_err(|error| ProviderError::Internal(error.to_string()))?;
            fields.insert(field.clone(), value);
        }
        Ok(PowerLimits { fields })
    }

    async fn set_power_limit(
        &self,
        _field: PowerLimitField,
        _value: i32,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "ASUS Armoury CPU package limits are read-only".into(),
        ))
    }

    async fn restore_defaults(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "ASUS Armoury CPU package limits are read-only".into(),
        ))
    }

    fn validate_power_limit(
        &self,
        _field: &PowerLimitField,
        _value: i32,
    ) -> crate::error::ValidationResult {
        crate::error::ValidationResult::invalid("ASUS Armoury CPU package limits are read-only")
    }
}

/// Probe the package-limit read contract without exposing mutation support.
pub(crate) async fn probe_power_limits_unbounded<P: PowerLimitProvider + ?Sized>(
    provider: &P,
) -> Result<Capability, orbis_capabilities::ProbeError> {
    let limits = match provider.power_limits().await {
        Ok(value) => value,
        Err(error) => {
            let operation = error
                .into_probe_result(ProbeContext::BackendDiscovery)?
                .into_operation();
            return Ok(orbis_capabilities::capability_from_operations(
                CapabilityOperations {
                    read: operation.clone(),
                    write: operation,
                },
                CapabilityConstraints::Unknown,
            ));
        }
    };
    let constraints = CapabilityConstraints::PowerLimits(
        limits
            .fields
            .iter()
            .map(
                |(field, value)| orbis_core::capability::PowerLimitConstraint {
                    field: field.clone(),
                    range: orbis_core::capability::IntegerConstraints {
                        min: Some(value.min),
                        max: Some(value.max),
                        step: Some(value.step),
                        default: value.default,
                    },
                    unit: value.unit,
                },
            )
            .collect(),
    );
    Ok(orbis_capabilities::capability_from_operations(
        CapabilityOperations {
            read: ProbeOperationResult::classified(ProbeClassification::Supported).into_operation(),
            write: orbis_core::capability::OperationCapability {
                status: orbis_core::capability::CapabilityStatus::ReadOnly,
                reason: Some(orbis_core::capability::CapabilityReason {
                    reason: "ASUS Armoury package limits have no typed write owner".into(),
                    suggestion: String::new(),
                    backend: None,
                    endpoint: None,
                    requirement: None,
                    risk: orbis_core::capability::RiskLevel::Safe,
                    checked_at: None,
                }),
            },
        },
        constraints,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn valid_fixture_reads_all_three_cpu_package_limits() {
        let (root, provider) = fixture("5", "5", "integer", Some("45"));
        let limits = provider.power_limits().await.unwrap();
        assert_eq!(limits.fields.len(), 3);
        assert_eq!(limits.get(&PowerLimitField::Spl).unwrap().value, 5);
        assert_eq!(limits.get(&PowerLimitField::Sppt).unwrap().min, 5);
        assert_eq!(
            limits.get(&PowerLimitField::Fppt).unwrap().default,
            Some(45)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn absent_attribute_is_unsupported() {
        let root = fixture_root();
        let provider = AsusArmouryPowerLimitProvider::new(&root);
        let error = provider.power_limits().await.unwrap_err();
        assert!(matches!(error, ProviderError::Unsupported(_)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enodev_is_transient_backend_unavailable() {
        let root = fixture_root();
        let provider = AsusArmouryPowerLimitProvider::new(&root);
        let error = map_power_limit_error(
            &root.join("current_value"),
            std::io::Error::from_raw_os_error(19),
        );
        assert!(matches!(error, ProviderError::BackendUnavailable(_)));
        fs::remove_dir_all(root).unwrap();
        let _ = provider;
    }

    #[tokio::test]
    async fn malformed_or_inconsistent_metadata_is_internal() {
        let (root, provider) = fixture("5", "10", "string", Some("100"));
        let error = provider.power_limits().await.unwrap_err();
        assert!(matches!(error, ProviderError::Internal(_)));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn capability_constraints_are_read_only_and_writes_rejected() {
        let (root, provider) = fixture("5", "5", "integer", None);
        let limits = provider.power_limits().await.unwrap();
        let capability = crate::probe_power_limits(&provider).await.unwrap();
        assert!(matches!(
            capability.constraints,
            CapabilityConstraints::PowerLimits(_)
        ));
        assert_eq!(
            capability.operations.write.status,
            orbis_core::capability::CapabilityStatus::ReadOnly
        );
        assert!(matches!(
            provider.set_power_limit(PowerLimitField::Spl, 10).await,
            Err(ProviderError::Unsupported(_))
        ));
        assert_eq!(limits.fields.len(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    fn fixture(
        current: &str,
        min: &str,
        kind: &str,
        default: Option<&str>,
    ) -> (PathBuf, AsusArmouryPowerLimitProvider) {
        let root = fixture_root();
        for (_, dir) in ASUS_ARMOURY_POWER_LIMITS {
            let path = root
                .join("class/firmware-attributes/asus-armoury/attributes")
                .join(dir);
            fs::create_dir_all(&path).unwrap();
            for (name, value) in [
                ("current_value", current),
                ("min_value", min),
                ("max_value", "80"),
                ("type", kind),
            ] {
                fs::write(path.join(name), value).unwrap();
            }
            if let Some(value) = default {
                fs::write(path.join("default_value"), value).unwrap();
            }
        }
        (root.clone(), AsusArmouryPowerLimitProvider::new(root))
    }

    fn fixture_root() -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "orbis-armoury-power-limits-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
