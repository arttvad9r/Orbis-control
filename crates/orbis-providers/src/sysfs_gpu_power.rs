//! Read-only NVIDIA dGPU runtime power provider backed by kernel sysfs.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::gpu::GpuPowerState;
use orbis_core::identity::BackendIdentity;

use crate::error::ProviderError;
use crate::traits::{GpuPowerProvider, Provider, ProviderHealth};

/// Provider for the NVIDIA DRM runtime power state.
pub struct SysfsGpuPowerProvider {
    sysfs_root: PathBuf,
}

impl SysfsGpuPowerProvider {
    /// Create a provider over a sysfs root. No I/O occurs here.
    pub fn new(sysfs_root: PathBuf) -> Self {
        Self { sysfs_root }
    }

    fn read_state(&self) -> Result<GpuPowerState, ProviderError> {
        let drm_root = self.sysfs_root.join("class/drm");
        let entries = fs::read_dir(&drm_root).map_err(ProviderError::Io)?;
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(ProviderError::Io)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            let device = entry.path().join("device");
            if read_trimmed(&device.join("vendor"))?.as_deref() == Some("0x10de") {
                candidates.push(device);
            }
        }
        let device = candidates
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::Unsupported("no NVIDIA DRM device found".into()))?;

        if let Some(state) = read_trimmed(&device.join("power/runtime_status"))? {
            return Ok(match state.as_str() {
                "active" => GpuPowerState::Active,
                "suspended" => GpuPowerState::Suspended,
                "off" => GpuPowerState::Off,
                _ => GpuPowerState::Unknown,
            });
        }
        Ok(
            match read_trimmed(&device.join("power_state"))?.as_deref() {
                Some("D0") => GpuPowerState::Active,
                Some("D3cold") | Some("D3hot") => GpuPowerState::Suspended,
                Some(_) => GpuPowerState::Unknown,
                None => GpuPowerState::Unknown,
            },
        )
    }
}

impl Default for SysfsGpuPowerProvider {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

fn read_trimmed(path: &std::path::Path) -> Result<Option<String>, ProviderError> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProviderError::Io(error)),
    }
}

impl Provider for SysfsGpuPowerProvider {
    fn id(&self) -> &'static str {
        "sysfs-gpu-power"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("sysfs-gpu-power")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("sysfs GPU power: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.sysfs-gpu-power",
            "read-only NVIDIA dGPU runtime power via kernel sysfs",
        )]
    }
}

#[async_trait]
impl GpuPowerProvider for SysfsGpuPowerProvider {
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        self.read_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nvidia_runtime_power_without_supergfxd() {
        let root = test_root("suspended");
        let provider = SysfsGpuPowerProvider::new(root.clone());

        assert_eq!(provider.read_state().unwrap(), GpuPowerState::Suspended);
        fs::remove_dir_all(root).unwrap();
    }

    fn test_root(runtime_status: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "orbis-sysfs-gpu-power-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let device = root.join("class/drm/card0/device/power");
        fs::create_dir_all(&device).unwrap();
        fs::write(root.join("class/drm/card0/device/vendor"), "0x10de\n").unwrap();
        fs::write(device.join("runtime_status"), format!("{runtime_status}\n")).unwrap();
        fs::write(device.join("power_state"), "D3cold\n").unwrap();
        root
    }
}
