use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orbis_core::diagnostics::{CpuFrequencyDiagnostics, DiagnosticEntry};
use orbis_core::identity::BackendIdentity;

use crate::error::ProviderError;
use crate::traits::{CpuFrequencyProvider, Provider, ProviderHealth};

/// Read-only Linux CPU frequency policy provider.
pub struct SysfsCpuFrequencyProvider {
    sysfs_root: PathBuf,
}

impl SysfsCpuFrequencyProvider {
    /// Create a provider rooted at `/sys` or a fixture tree.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.sysfs_root
            .join("devices/system/cpu/cpu0/cpufreq")
            .join(name)
    }
}

impl Default for SysfsCpuFrequencyProvider {
    fn default() -> Self {
        Self::new("/sys")
    }
}

impl Provider for SysfsCpuFrequencyProvider {
    fn id(&self) -> &'static str {
        "sysfs-cpu-frequency"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("linux-cpufreq")
    }

    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("linux-cpufreq: feature '{feature}' unavailable")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.sysfs-cpu-frequency",
            "read-only CPU frequency policy evidence; no EPP or boost write owner",
        )]
    }
}

fn read(path: &Path) -> Result<String, ProviderError> {
    fs::read_to_string(path).map_err(|error| {
        if error.raw_os_error() == Some(19) {
            ProviderError::BackendUnavailable(format!("cpu frequency ENODEV: {}", path.display()))
        } else {
            match error.kind() {
                std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
                    "cpu frequency attribute absent: {}",
                    path.display()
                )),
                std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
                    "cpu frequency read denied: {}",
                    path.display()
                )),
                _ => ProviderError::Io(error),
            }
        }
    })
}

#[async_trait]
impl CpuFrequencyProvider for SysfsCpuFrequencyProvider {
    async fn cpu_frequency(&self) -> Result<CpuFrequencyDiagnostics, ProviderError> {
        let driver = read(&self.path("scaling_driver"))?.trim().to_owned();
        let available_epp_preferences =
            read(&self.path("energy_performance_available_preferences"))?
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
        let current_epp_preference = read(&self.path("energy_performance_preference"))?
            .trim()
            .to_owned();
        let boost_raw = read(&self.path("boost"))?;
        let boost = match boost_raw.trim() {
            "0" => false,
            "1" => true,
            value => {
                return Err(ProviderError::Internal(format!(
                    "cpu frequency malformed boost: {value:?}"
                )));
            }
        };
        if driver.is_empty()
            || available_epp_preferences.is_empty()
            || current_epp_preference.is_empty()
        {
            return Err(ProviderError::Internal(
                "cpu frequency contains an empty required value".into(),
            ));
        }
        Ok(CpuFrequencyDiagnostics {
            driver,
            available_epp_preferences,
            current_epp_preference,
            boost,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[tokio::test]
    async fn reads_amd_pstate_epp_evidence_without_writes() {
        let root = fixture("amd-pstate-epp", "default performance power", "power", "0");
        let value = SysfsCpuFrequencyProvider::new(&root)
            .cpu_frequency()
            .await
            .unwrap();
        assert_eq!(value.driver, "amd-pstate-epp");
        assert_eq!(
            value.available_epp_preferences,
            ["default", "performance", "power"]
        );
        assert_eq!(value.current_epp_preference, "power");
        assert!(!value.boost);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn malformed_boost_is_internal_error() {
        let root = fixture("amd-pstate-epp", "power", "power", "maybe");
        assert!(matches!(
            SysfsCpuFrequencyProvider::new(&root).cpu_frequency().await,
            Err(ProviderError::Internal(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn absent_policy_is_unsupported() {
        let root =
            std::env::temp_dir().join(format!("orbis-cpu-frequency-absent-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert!(matches!(
            SysfsCpuFrequencyProvider::new(&root).cpu_frequency().await,
            Err(ProviderError::Unsupported(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    fn fixture(driver: &str, available: &str, current: &str, boost: &str) -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "orbis-cpu-frequency-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = root.join("devices/system/cpu/cpu0/cpufreq");
        fs::create_dir_all(&dir).unwrap();
        for (name, value) in [
            ("scaling_driver", driver),
            ("energy_performance_available_preferences", available),
            ("energy_performance_preference", current),
            ("boost", boost),
        ] {
            fs::write(dir.join(name), value).unwrap();
        }
        root
    }
}
