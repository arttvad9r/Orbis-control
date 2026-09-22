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

pub const TIMEOUT_PREFIX: &str = "orbis power-limit timeout: ";
pub const CONFLICT_PREFIX: &str = "orbis power-limit conflict: ";
pub fn operation_timeout() -> Duration {
    Duration::from_secs(2)
}

#[cfg(test)]
mod tests {
    use super::*;

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
