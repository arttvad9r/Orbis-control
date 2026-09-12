//! Read-only power/thermal fields exposed by the Linux ASUS platform driver.

use std::path::{Path, PathBuf};

use orbis_core::{PowerLimitField, PowerLimitObservation, Unit};
use orbis_providers::error::ProviderError;

/// Linux sysfs root used by the ASUS platform driver.
const ASUS_NB_WMI_ROOT: &str = "/sys/devices/platform/asus-nb-wmi";

/// Read all power/thermal fields that are actually exposed by this machine.
pub fn read_power_limit_observations() -> Result<Vec<PowerLimitObservation>, ProviderError> {
    let fields = [
        ("ppt_pl1_spl", PowerLimitField::Spl, Unit::Watts),
        ("ppt_pl2_sppt", PowerLimitField::Sppt, Unit::Watts),
        ("ppt_pl3_fppt", PowerLimitField::Fppt, Unit::Watts),
        (
            "nv_dynamic_boost",
            PowerLimitField::GpuDynamicBoost,
            Unit::Watts,
        ),
        (
            "nv_temp_target",
            PowerLimitField::GpuTempTarget,
            Unit::DegreesC,
        ),
    ];
    let mut observations = Vec::new();
    for (name, field, unit) in fields {
        let path = Path::new(ASUS_NB_WMI_ROOT).join(name);
        if !path.exists() {
            continue;
        }
        let value = read_integer(&path, name)?;
        observations.push(PowerLimitObservation::without_metadata(field, value, unit));
    }
    Ok(observations)
}

fn read_integer(path: &Path, name: &str) -> Result<i32, ProviderError> {
    let raw = std::fs::read_to_string(path).map_err(ProviderError::Io)?;
    raw.trim().parse::<i32>().map_err(|error| {
        ProviderError::Internal(format!("power field {name} is malformed: {error}"))
    })
}

/// Testable reader with an alternate sysfs root.
pub fn read_from_root(
    root: impl Into<PathBuf>,
) -> Result<Vec<PowerLimitObservation>, ProviderError> {
    let root = root.into();
    let fields = [
        ("ppt_pl1_spl", PowerLimitField::Spl, Unit::Watts),
        ("ppt_pl2_sppt", PowerLimitField::Sppt, Unit::Watts),
        ("ppt_pl3_fppt", PowerLimitField::Fppt, Unit::Watts),
        (
            "nv_dynamic_boost",
            PowerLimitField::GpuDynamicBoost,
            Unit::Watts,
        ),
        (
            "nv_temp_target",
            PowerLimitField::GpuTempTarget,
            Unit::DegreesC,
        ),
    ];
    let mut observations = Vec::new();
    for (name, field, unit) in fields {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        observations.push(PowerLimitObservation::without_metadata(
            field,
            read_integer(&path, name)?,
            unit,
        ));
    }
    Ok(observations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_present_fields_without_fake_metadata() {
        let root = std::env::temp_dir().join(format!("orbis-power-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("ppt_pl1_spl"), "5\n").unwrap();
        std::fs::write(root.join("nv_temp_target"), "75\n").unwrap();
        let values = read_from_root(&root).unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].value, 5);
        assert_eq!(values[1].value, 75);
        assert!(values.iter().all(|value| !value.has_editable_metadata()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_field_is_an_error() {
        let root =
            std::env::temp_dir().join(format!("orbis-power-malformed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("ppt_pl1_spl"), "not-a-number\n").unwrap();
        assert!(read_from_root(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
