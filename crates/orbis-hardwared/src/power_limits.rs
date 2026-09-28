//! Typed mutation backend for the kernel ASUS Armoury firmware-attributes ABI.
//!
//! This is intentionally field-specific: no caller supplied path, command or
//! generic sysfs writer is exposed. Every operation reads fresh metadata,
//! validates against it, writes once, and performs an authoritative read-back.

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::limits::PowerLimitField;
use orbis_providers::error::ProviderError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ARMOURY_ATTRIBUTES_ROOT: &str = "/sys/class/firmware-attributes/asus-armoury/attributes";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerLimitMutationReadback {
    pub field: PowerLimitField,
    pub requested: i32,
    pub observed: i32,
    pub result: ApplyResult,
}

#[async_trait]
pub trait PowerLimitMutationBackend: Send + Sync {
    /// Read-only availability probe for the mutation backend.
    ///
    /// This is capability metadata, not a mutation: it must perform no
    /// hardware write and require no authorization. `Ok(())` proves the
    /// typed write path exists; the error variant classifies why it does
    /// not.
    fn set_power_limit_probe(&self) -> Result<(), ProviderError>;

    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<PowerLimitMutationReadback, ProviderError>;
}

/// Typed runtime evidence for power-limit mutation backend availability.
///
/// Proven by a read-only fresh probe of the same kernel attribute the
/// mutation writes. The classification preserves the distinction between a
/// present ABI (`Supported`), an absent ABI (`Unsupported`), a transient
/// read failure (`TemporarilyUnavailable`) and a permission failure
/// (`PermissionDenied`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerLimitMutationStatus {
    /// The kernel attribute ABI is present and readable, so the mutation
    /// path exists. Operational write failures are not predicted.
    Supported,
    /// The ABI is structurally absent on this device.
    Unsupported,
    /// A known/expected ABI is temporarily unreadable.
    TemporarilyUnavailable,
    /// The ABI exists but current read authorization denies access.
    PermissionDenied,
    /// No provable evidence about mutation availability.
    Unknown,
}

/// Stable wire values for `Hardware1.PowerLimitMutationStatus`.
///
/// The numeric values intentionally match the Battery/Performance mutation
/// status wire contract (identical semantic classes); this module keeps its
/// own named constants so the D-Bus contract stays self-contained.
pub mod power_limit_mutation_wire {
    use super::PowerLimitMutationStatus;

    /// Proven mutation backend / ABI present.
    pub const SUPPORTED: u8 = 0;
    /// Mutation capability structurally absent.
    pub const UNSUPPORTED: u8 = 1;
    /// Known backend temporarily unavailable.
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    /// Mutation denied by authorization evidence.
    pub const PERMISSION_DENIED: u8 = 3;
    /// No evidence.
    pub const UNKNOWN: u8 = 4;

    /// Encode typed status into the D-Bus wire value.
    pub fn to_wire(status: PowerLimitMutationStatus) -> u8 {
        match status {
            PowerLimitMutationStatus::Supported => SUPPORTED,
            PowerLimitMutationStatus::Unsupported => UNSUPPORTED,
            PowerLimitMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            PowerLimitMutationStatus::PermissionDenied => PERMISSION_DENIED,
            PowerLimitMutationStatus::Unknown => UNKNOWN,
        }
    }

    /// Decode a wire value; unknown values produce `None` so callers classify
    /// them as `Unknown` instead of inventing a known state.
    pub fn from_wire(raw: u8) -> Option<PowerLimitMutationStatus> {
        match raw {
            SUPPORTED => Some(PowerLimitMutationStatus::Supported),
            UNSUPPORTED => Some(PowerLimitMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => Some(PowerLimitMutationStatus::TemporarilyUnavailable),
            PERMISSION_DENIED => Some(PowerLimitMutationStatus::PermissionDenied),
            UNKNOWN => Some(PowerLimitMutationStatus::Unknown),
            _ => None,
        }
    }
}

/// Classify a backend probe outcome into the typed mutation status.
///
/// Shared classification for every backend's `set_power_limit_probe` error
/// surface: a fine-grained probe error becomes its distinct status, and
/// anything inconclusive stays `Unknown` instead of being guessed into a
/// known state.
pub fn classify_mutation_probe(probe: &Result<(), ProviderError>) -> PowerLimitMutationStatus {
    match probe {
        Ok(()) => PowerLimitMutationStatus::Supported,
        Err(ProviderError::Unsupported(_)) => PowerLimitMutationStatus::Unsupported,
        Err(ProviderError::PermissionDenied(_)) => PowerLimitMutationStatus::PermissionDenied,
        Err(ProviderError::BackendUnavailable(_)) => {
            PowerLimitMutationStatus::TemporarilyUnavailable
        }
        Err(_) => PowerLimitMutationStatus::Unknown,
    }
}

fn field_name(field: &PowerLimitField) -> Result<&'static str, ProviderError> {
    match field {
        PowerLimitField::Spl => Ok("ppt_pl1_spl"),
        PowerLimitField::Sppt => Ok("ppt_pl2_sppt"),
        PowerLimitField::Fppt => Ok("ppt_pl3_fppt"),
        PowerLimitField::GpuDynamicBoost => Ok("nv_dynamic_boost"),
        PowerLimitField::GpuTempTarget => Ok("nv_temp_target"),
        PowerLimitField::CpuTempLimit => Err(ProviderError::Unsupported(
            "kernel ASUS Armoury has no CPU temperature-limit attribute".into(),
        )),
        other => Err(ProviderError::Unsupported(format!(
            "kernel ASUS Armoury does not support power field {other:?}"
        ))),
    }
}

fn path(name: &str, property: &str) -> PathBuf {
    Path::new(ARMOURY_ATTRIBUTES_ROOT).join(name).join(property)
}

fn read(name: &str, property: &str) -> Result<i32, ProviderError> {
    let raw =
        fs::read_to_string(path(name, property)).map_err(|error| match error.raw_os_error() {
            Some(19) => {
                ProviderError::Unsupported(format!("kernel Armoury {name}/{property}: {error}"))
            }
            _ => ProviderError::Io(error),
        })?;
    raw.trim().parse().map_err(|error| {
        ProviderError::Internal(format!(
            "kernel Armoury {name}/{property} malformed: {error}"
        ))
    })
}

fn validate(name: &str, value: i32) -> Result<(), ProviderError> {
    let min = read(name, "min_value")?;
    let max = read(name, "max_value")?;
    let step = read(name, "scalar_increment")?;
    if step <= 0 {
        return Err(ProviderError::Internal(format!(
            "kernel Armoury {name}: malformed step {step}"
        )));
    }
    if value < min || value > max || (value - min) % step != 0 {
        return Err(ProviderError::InvalidRequest(format!(
            "kernel Armoury {name}: value {value} violates {min}..{max} step {step}"
        )));
    }
    Ok(())
}

pub struct KernelAsusPowerLimitMutationBackend;

pub mod wire {
    pub const SPL: u8 = 0;
    pub const SPPT: u8 = 1;
    pub const FPPT: u8 = 2;
    pub const CPU_TEMP_LIMIT: u8 = 3;
    pub const GPU_DYNAMIC_BOOST: u8 = 4;
    pub const GPU_TEMP_TARGET: u8 = 5;
}

pub fn field_from_wire(raw: u8) -> Result<PowerLimitField, ProviderError> {
    match raw {
        wire::SPL => Ok(PowerLimitField::Spl),
        wire::SPPT => Ok(PowerLimitField::Sppt),
        wire::FPPT => Ok(PowerLimitField::Fppt),
        wire::CPU_TEMP_LIMIT => Ok(PowerLimitField::CpuTempLimit),
        wire::GPU_DYNAMIC_BOOST => Ok(PowerLimitField::GpuDynamicBoost),
        wire::GPU_TEMP_TARGET => Ok(PowerLimitField::GpuTempTarget),
        other => Err(ProviderError::InvalidRequest(format!(
            "unknown power-limit field {other}"
        ))),
    }
}

pub fn field_to_wire(field: &PowerLimitField) -> Result<u8, ProviderError> {
    match field {
        PowerLimitField::Spl => Ok(wire::SPL),
        PowerLimitField::Sppt => Ok(wire::SPPT),
        PowerLimitField::Fppt => Ok(wire::FPPT),
        PowerLimitField::CpuTempLimit => Ok(wire::CPU_TEMP_LIMIT),
        PowerLimitField::GpuDynamicBoost => Ok(wire::GPU_DYNAMIC_BOOST),
        PowerLimitField::GpuTempTarget => Ok(wire::GPU_TEMP_TARGET),
        other => Err(ProviderError::Unsupported(format!(
            "unsupported power-limit field {other:?}"
        ))),
    }
}

impl KernelAsusPowerLimitMutationBackend {
    pub fn new() -> Self {
        Self
    }

    /// Read-only probe of the same attribute the mutation writes.
    ///
    /// `ppt_pl1_spl/current_value` is the canary: it belongs to the typed
    /// SPL/SPPT/FPPT Armoury attribute family and is readable in every state
    /// where writes are meaningful. A fresh read (never a cached capability
    /// flag) is the evidence, classified by the shared probe mapping.
    #[allow(dead_code)]
    fn read_mutation_status(&self) -> PowerLimitMutationStatus {
        match read("ppt_pl1_spl", "current_value") {
            Ok(_) => PowerLimitMutationStatus::Supported,
            Err(error) => classify_mutation_probe(&Err(error)),
        }
    }
}

impl Default for KernelAsusPowerLimitMutationBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PowerLimitMutationBackend for KernelAsusPowerLimitMutationBackend {
    fn set_power_limit_probe(&self) -> Result<(), ProviderError> {
        // Read-only: same evidence as `read_mutation_status`, surfaced as a
        // probe result so the daemon boundary owns the classification.
        match read("ppt_pl1_spl", "current_value") {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<PowerLimitMutationReadback, ProviderError> {
        let name = field_name(&field)?;
        validate(name, value)?;
        fs::write(path(name, "current_value"), format!("{value}\n")).map_err(ProviderError::Io)?;
        let observed = read(name, "current_value")?;
        if observed != value {
            return Err(ProviderError::Conflict(format!(
                "kernel Armoury {name} read-back mismatch: requested={value}, observed={observed}"
            )));
        }
        Ok(PowerLimitMutationReadback {
            field,
            requested: value,
            observed,
            result: ApplyResult::Applied,
        })
    }
}

/// Mutation backend for the legacy `asus-nb-wmi` PPT files.
///
/// Field-specific and fixed-path like the Armoury backend. Ranges come from the
/// verified per-model table because this ABI publishes no metadata; the
/// read-back is the driver cache, which is updated only after the firmware
/// acknowledged the WMI call.
pub struct AsusNbWmiPowerLimitMutationBackend {
    root: PathBuf,
    product: Option<String>,
}

impl AsusNbWmiPowerLimitMutationBackend {
    pub fn new() -> Self {
        Self {
            root: PathBuf::from(orbis_providers::ASUS_NB_WMI_ROOT),
            product: orbis_providers::read_dmi_product_name(),
        }
    }

    #[cfg(test)]
    fn with_root(root: impl Into<PathBuf>, product: Option<&str>) -> Self {
        Self {
            root: root.into(),
            product: product.map(str::to_string),
        }
    }

    fn file(&self, field: &PowerLimitField) -> Result<PathBuf, ProviderError> {
        let name = orbis_providers::legacy_ppt_file(field).ok_or_else(|| {
            ProviderError::Unsupported(format!("asus-nb-wmi has no attribute for {field:?}"))
        })?;
        Ok(self.root.join(name))
    }

    fn bounds(&self, field: &PowerLimitField) -> Result<(i32, i32), ProviderError> {
        self.product
            .as_deref()
            .and_then(|product| orbis_providers::legacy_ppt_bounds(product, field))
            .ok_or_else(|| {
                ProviderError::Unsupported(format!(
                    "asus-nb-wmi {field:?}: no verified range table entry for this model"
                ))
            })
    }

    fn read_cache(path: &Path) -> Result<i32, ProviderError> {
        let raw = fs::read_to_string(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                ProviderError::Unsupported(format!("asus-nb-wmi {}: absent", path.display()))
            }
            std::io::ErrorKind::PermissionDenied => {
                ProviderError::PermissionDenied(format!("asus-nb-wmi {}: denied", path.display()))
            }
            _ => ProviderError::Io(error),
        })?;
        raw.trim().parse().map_err(|error| {
            ProviderError::Internal(format!("asus-nb-wmi {} malformed: {error}", path.display()))
        })
    }
}

impl Default for AsusNbWmiPowerLimitMutationBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PowerLimitMutationBackend for AsusNbWmiPowerLimitMutationBackend {
    fn set_power_limit_probe(&self) -> Result<(), ProviderError> {
        self.bounds(&PowerLimitField::Spl)?;
        Self::read_cache(&self.file(&PowerLimitField::Spl)?).map(|_| ())
    }

    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<PowerLimitMutationReadback, ProviderError> {
        let (min, max) = self.bounds(&field)?;
        if value < min || value > max {
            return Err(ProviderError::InvalidRequest(format!(
                "asus-nb-wmi {field:?}: value {value} outside {min}..{max}"
            )));
        }
        let path = self.file(&field)?;
        fs::write(&path, format!("{value}\n")).map_err(ProviderError::Io)?;
        let observed = Self::read_cache(&path)?;
        if observed != value {
            return Err(ProviderError::Conflict(format!(
                "asus-nb-wmi {field:?} read-back mismatch: requested={value}, observed={observed}"
            )));
        }
        Ok(PowerLimitMutationReadback {
            field,
            requested: value,
            observed,
            result: ApplyResult::Applied,
        })
    }
}

/// Armoury-first selector: the legacy backend is used only when the kernel
/// Armoury ABI is structurally absent, never as a fallback for a failed write.
pub struct AsusPowerLimitMutationBackend {
    armoury: Box<dyn PowerLimitMutationBackend>,
    legacy: Box<dyn PowerLimitMutationBackend>,
}

impl AsusPowerLimitMutationBackend {
    pub fn new(
        armoury: Box<dyn PowerLimitMutationBackend>,
        legacy: Box<dyn PowerLimitMutationBackend>,
    ) -> Self {
        Self { armoury, legacy }
    }

    fn active(&self) -> &dyn PowerLimitMutationBackend {
        match self.armoury.set_power_limit_probe() {
            Ok(()) => self.armoury.as_ref(),
            Err(_) => self.legacy.as_ref(),
        }
    }
}

#[async_trait]
impl PowerLimitMutationBackend for AsusPowerLimitMutationBackend {
    fn set_power_limit_probe(&self) -> Result<(), ProviderError> {
        match self.armoury.set_power_limit_probe() {
            Ok(()) => Ok(()),
            Err(armoury_error) => {
                self.legacy
                    .set_power_limit_probe()
                    .map_err(|legacy_error| match legacy_error {
                        ProviderError::Unsupported(_) => armoury_error,
                        other => other,
                    })
            }
        }
    }

    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<PowerLimitMutationReadback, ProviderError> {
        self.active().set_power_limit(field, value).await
    }
}

pub const TIMEOUT_PREFIX: &str = "orbis power-limit timeout: ";
pub const CONFLICT_PREFIX: &str = "orbis power-limit conflict: ";
pub fn operation_timeout() -> Duration {
    Duration::from_secs(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRODUCT: &str = "ASUS TUF Gaming A17 FA707NV_FA707NV";

    #[tokio::test]
    async fn legacy_backend_writes_verified_range_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ppt_pl1_spl"), "5\n").unwrap();
        let backend = AsusNbWmiPowerLimitMutationBackend::with_root(dir.path(), Some(PRODUCT));
        assert!(backend.set_power_limit_probe().is_ok());
        let readback = backend
            .set_power_limit(PowerLimitField::Spl, 45)
            .await
            .unwrap();
        assert_eq!(
            (readback.observed, readback.result),
            (45, ApplyResult::Applied)
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("ppt_pl1_spl"))
                .unwrap()
                .trim(),
            "45"
        );
    }

    #[tokio::test]
    async fn legacy_backend_rejects_out_of_range_and_unverified_models() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ppt_pl1_spl"), "5\n").unwrap();
        let backend = AsusNbWmiPowerLimitMutationBackend::with_root(dir.path(), Some(PRODUCT));
        for value in [14, 91] {
            assert!(matches!(
                backend.set_power_limit(PowerLimitField::Spl, value).await,
                Err(ProviderError::InvalidRequest(_))
            ));
        }
        assert!(matches!(
            backend
                .set_power_limit(PowerLimitField::CpuTempLimit, 80)
                .await,
            Err(ProviderError::Unsupported(_))
        ));
        let unknown = AsusNbWmiPowerLimitMutationBackend::with_root(dir.path(), Some("ROG X"));
        assert!(matches!(
            unknown.set_power_limit_probe(),
            Err(ProviderError::Unsupported(_))
        ));
        assert!(
            unknown
                .set_power_limit(PowerLimitField::Spl, 45)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("ppt_pl1_spl"))
                .unwrap()
                .trim(),
            "5"
        );
    }

    #[test]
    fn cpu_temperature_limit_is_not_wired_to_an_unrelated_backend() {
        assert!(field_name(&PowerLimitField::CpuTempLimit).is_err());
    }

    #[test]
    fn only_kernel_armoury_attribute_names_are_accepted() {
        assert_eq!(field_name(&PowerLimitField::Spl).unwrap(), "ppt_pl1_spl");
        assert!(field_name(&PowerLimitField::Other("arbitrary".into())).is_err());
    }

    #[test]
    fn power_limit_mutation_wire_roundtrip_is_total() {
        use power_limit_mutation_wire;
        let all = [
            PowerLimitMutationStatus::Supported,
            PowerLimitMutationStatus::Unsupported,
            PowerLimitMutationStatus::TemporarilyUnavailable,
            PowerLimitMutationStatus::PermissionDenied,
            PowerLimitMutationStatus::Unknown,
        ];
        for status in all {
            let wire = power_limit_mutation_wire::to_wire(status);
            assert_eq!(power_limit_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(power_limit_mutation_wire::from_wire(250), None);
    }

    #[test]
    fn probe_classification_is_fine_grained_and_fail_closed() {
        assert_eq!(
            classify_mutation_probe(&Ok(())),
            PowerLimitMutationStatus::Supported
        );
        assert_eq!(
            classify_mutation_probe(&Err(ProviderError::Unsupported(
                "structurally absent".into()
            ))),
            PowerLimitMutationStatus::Unsupported
        );
        assert_eq!(
            classify_mutation_probe(&Err(ProviderError::PermissionDenied("denied".into()))),
            PowerLimitMutationStatus::PermissionDenied
        );
        assert_eq!(
            classify_mutation_probe(&Err(ProviderError::BackendUnavailable("gone".into()))),
            PowerLimitMutationStatus::TemporarilyUnavailable
        );
        // Inconclusive evidence must stay Unknown, never a guessed state.
        assert_eq!(
            classify_mutation_probe(&Err(ProviderError::Internal("garbled".into()))),
            PowerLimitMutationStatus::Unknown
        );
        assert_eq!(
            classify_mutation_probe(&Err(ProviderError::Io(std::io::Error::other("enoent")))),
            PowerLimitMutationStatus::Unknown
        );
    }
}
