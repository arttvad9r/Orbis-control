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
    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<PowerLimitMutationReadback, ProviderError>;
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
}

impl Default for KernelAsusPowerLimitMutationBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PowerLimitMutationBackend for KernelAsusPowerLimitMutationBackend {
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
}
