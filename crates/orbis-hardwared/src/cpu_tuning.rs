//! Typed CPU tuning mutations: energy/performance preference and boost.
//!
//! Paths are fixed under the kernel CPU sysfs root; callers only choose a typed
//! value. Every write is followed by a read-back of every touched attribute.

use std::fs;
use std::path::{Path, PathBuf};

use orbis_core::action::ApplyResult;
use orbis_core::cpu_tuning::EnergyPreference;
use orbis_providers::error::ProviderError;

pub const CPU_SYSFS_ROOT: &str = "/sys/devices/system/cpu";
const EPP_ATTRIBUTE: &str = "cpufreq/energy_performance_preference";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuTuningStatus {
    Supported,
    Unsupported,
    TemporarilyUnavailable,
    PermissionDenied,
    Unknown,
}

pub mod mutation_wire {
    use super::CpuTuningStatus;

    pub fn to_wire(status: CpuTuningStatus) -> u8 {
        match status {
            CpuTuningStatus::Supported => 0,
            CpuTuningStatus::Unsupported => 1,
            CpuTuningStatus::TemporarilyUnavailable => 2,
            CpuTuningStatus::PermissionDenied => 3,
            CpuTuningStatus::Unknown => 4,
        }
    }
}

pub struct CpuTuningWriter {
    root: PathBuf,
}

impl Default for CpuTuningWriter {
    fn default() -> Self {
        Self::with_root(CPU_SYSFS_ROOT)
    }
}

fn classify(error: &std::io::Error) -> CpuTuningStatus {
    match error.kind() {
        std::io::ErrorKind::NotFound => CpuTuningStatus::Unsupported,
        std::io::ErrorKind::PermissionDenied => CpuTuningStatus::PermissionDenied,
        _ => CpuTuningStatus::TemporarilyUnavailable,
    }
}

impl CpuTuningWriter {
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn boost_path(&self) -> PathBuf {
        self.root.join("cpufreq/boost")
    }

    /// EPP attributes of all CPUs that currently expose cpufreq, in CPU order.
    fn epp_paths(&self) -> Result<Vec<PathBuf>, ProviderError> {
        let mut cpus: Vec<(u32, PathBuf)> = fs::read_dir(&self.root)
            .map_err(ProviderError::Io)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let index = name.strip_prefix("cpu")?.parse::<u32>().ok()?;
                let path = entry.path().join(EPP_ATTRIBUTE);
                path.exists().then_some((index, path))
            })
            .collect();
        cpus.sort();
        Ok(cpus.into_iter().map(|(_, path)| path).collect())
    }

    pub fn epp_status(&self) -> CpuTuningStatus {
        match self.epp_paths() {
            Ok(paths) if paths.is_empty() => CpuTuningStatus::Unsupported,
            Ok(paths) => match fs::read_to_string(&paths[0]) {
                Ok(_) => CpuTuningStatus::Supported,
                Err(error) => classify(&error),
            },
            Err(ProviderError::Io(error)) => classify(&error),
            Err(_) => CpuTuningStatus::Unknown,
        }
    }

    pub fn boost_status(&self) -> CpuTuningStatus {
        match fs::read_to_string(self.boost_path()) {
            Ok(text) if matches!(text.trim(), "0" | "1") => CpuTuningStatus::Supported,
            Ok(_) => CpuTuningStatus::Unknown,
            Err(error) => classify(&error),
        }
    }

    pub fn set_epp(&self, preference: EnergyPreference) -> Result<ApplyResult, ProviderError> {
        let paths = self.epp_paths()?;
        if paths.is_empty() {
            return Err(ProviderError::Unsupported(
                "hardwared: no CPU exposes energy_performance_preference".into(),
            ));
        }
        for path in &paths {
            write_attribute(path, preference.sysfs())?;
        }
        for path in &paths {
            let observed = fs::read_to_string(path).map_err(ProviderError::Io)?;
            if EnergyPreference::from_sysfs(&observed) != Some(preference) {
                return Err(ProviderError::Conflict(format!(
                    "hardwared: EPP read-back of {} did not confirm '{}'",
                    path.display(),
                    preference.sysfs()
                )));
            }
        }
        Ok(ApplyResult::Applied)
    }

    pub fn set_boost(&self, enabled: bool) -> Result<ApplyResult, ProviderError> {
        let path = self.boost_path();
        let token = if enabled { "1" } else { "0" };
        write_attribute(&path, token)?;
        let observed = fs::read_to_string(&path).map_err(ProviderError::Io)?;
        if observed.trim() != token {
            return Err(ProviderError::Conflict(format!(
                "hardwared: boost read-back did not confirm '{token}'"
            )));
        }
        Ok(ApplyResult::Applied)
    }
}

fn write_attribute(path: &Path, token: &str) -> Result<(), ProviderError> {
    fs::write(path, format!("{token}\n")).map_err(|error| match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ProviderError::PermissionDenied(format!("hardwared: write denied: {}", path.display()))
        }
        _ => ProviderError::Io(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for cpu in 0..3 {
            let cpufreq = dir.path().join(format!("cpu{cpu}/cpufreq"));
            fs::create_dir_all(&cpufreq).unwrap();
            fs::write(
                cpufreq.join("energy_performance_preference"),
                "balance_power\n",
            )
            .unwrap();
        }
        fs::create_dir_all(dir.path().join("cpu3")).unwrap();
        fs::create_dir_all(dir.path().join("cpufreq")).unwrap();
        fs::write(dir.path().join("cpufreq/boost"), "1\n").unwrap();
        dir
    }

    #[test]
    fn epp_is_written_to_every_online_cpu_and_read_back() {
        let dir = fixture();
        let writer = CpuTuningWriter::with_root(dir.path());
        assert_eq!(writer.epp_status(), CpuTuningStatus::Supported);
        assert_eq!(
            writer.set_epp(EnergyPreference::Performance).unwrap(),
            ApplyResult::Applied
        );
        for cpu in 0..3 {
            let text = fs::read_to_string(
                dir.path()
                    .join(format!("cpu{cpu}/cpufreq/energy_performance_preference")),
            )
            .unwrap();
            assert_eq!(text, "performance\n");
        }
        assert!(
            !dir.path().join("cpu3/cpufreq").exists(),
            "offline CPU untouched"
        );
    }

    #[test]
    fn boost_round_trips_and_missing_paths_are_unsupported() {
        let dir = fixture();
        let writer = CpuTuningWriter::with_root(dir.path());
        assert_eq!(writer.boost_status(), CpuTuningStatus::Supported);
        assert_eq!(writer.set_boost(false).unwrap(), ApplyResult::Applied);
        assert_eq!(
            fs::read_to_string(dir.path().join("cpufreq/boost")).unwrap(),
            "0\n"
        );

        let empty = tempfile::tempdir().unwrap();
        let writer = CpuTuningWriter::with_root(empty.path());
        assert_eq!(writer.epp_status(), CpuTuningStatus::Unsupported);
        assert_eq!(writer.boost_status(), CpuTuningStatus::Unsupported);
        assert!(matches!(
            writer.set_epp(EnergyPreference::Power),
            Err(ProviderError::Unsupported(_))
        ));
        assert!(writer.set_boost(true).is_err());
    }
}
