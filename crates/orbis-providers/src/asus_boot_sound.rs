//! Read-only ASUS BIOS/POST boot-sound provider.
//!
//! Authority is the modern kernel firmware-attributes ABI:
//! `/sys/class/firmware-attributes/asus-armoury/attributes/boot_sound/current_value`.
//! The provider never writes the attribute. Absence, permission denial and
//! malformed values remain distinct provider errors rather than fake `false`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::firmware::BootSoundState;
use orbis_core::identity::BackendIdentity;

use crate::error::ProviderError;
use crate::traits::{Provider, ProviderHealth};

/// Fixed relative path of the kernel `asus-armoury` ABI.
pub const ASUS_ARMOURY_BOOT_SOUND_RELATIVE_PATH: &str =
    "class/firmware-attributes/asus-armoury/attributes/boot_sound/current_value";

/// Read-only provider over one injected sysfs root.
pub struct AsusBootSoundProvider {
    sysfs_root: PathBuf,
}

impl AsusBootSoundProvider {
    /// Construct without I/O. Production uses `/sys`; tests may inject a fixture.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    fn current_value_path(&self) -> PathBuf {
        self.sysfs_root.join(ASUS_ARMOURY_BOOT_SOUND_RELATIVE_PATH)
    }

    /// Fresh authoritative boot-sound observation.
    pub async fn boot_sound_state(&self) -> Result<BootSoundState, ProviderError> {
        let path = self.current_value_path();
        let raw = std::fs::read_to_string(&path).map_err(|error| map_read_error(&path, error))?;
        let value = raw.trim().parse::<u32>().map_err(|error| {
            ProviderError::Internal(format!(
                "asus-armoury boot_sound malformed value {:?}: {error}",
                raw.trim()
            ))
        })?;
        BootSoundState::from_kernel_value(value).ok_or_else(|| {
            ProviderError::Internal(format!(
                "asus-armoury boot_sound value outside boolean ABI: {value}"
            ))
        })
    }
}

impl Default for AsusBootSoundProvider {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

impl Provider for AsusBootSoundProvider {
    fn id(&self) -> &'static str {
        "asus-armoury-boot-sound"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-armoury")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("asus-armoury: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-armoury-boot-sound",
            "read-only kernel asus-armoury boot_sound backend",
        )]
    }
}

fn map_read_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus-armoury boot_sound attribute absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury boot_sound read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "orbis-boot-sound-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        root
    }

    #[tokio::test]
    async fn reads_boolean_kernel_state_strictly() {
        let root = fixture_root("strict");
        let path = root.join(ASUS_ARMOURY_BOOT_SOUND_RELATIVE_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "1\n").unwrap();
        let provider = AsusBootSoundProvider::new(&root);
        assert_eq!(
            provider.boot_sound_state().await.unwrap(),
            BootSoundState::Enabled
        );

        std::fs::write(&path, "0\n").unwrap();
        assert_eq!(
            provider.boot_sound_state().await.unwrap(),
            BootSoundState::Disabled
        );

        std::fs::write(&path, "2\n").unwrap();
        assert!(matches!(
            provider.boot_sound_state().await,
            Err(ProviderError::Internal(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_attribute_is_unsupported_not_disabled() {
        let root = fixture_root("missing");
        let provider = AsusBootSoundProvider::new(&root);
        assert!(matches!(
            provider.boot_sound_state().await,
            Err(ProviderError::Unsupported(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_has_no_write_surface() {
        let source = include_str!("asus_boot_sound.rs");
        let forbidden = [
            ["write", "_brightness"].concat(),
            ["set_", "boot_sound"].concat(),
            ["Command", "::new"].concat(),
        ];
        for token in forbidden {
            assert!(
                !source.contains(&token),
                "unexpected mutation surface: {token}"
            );
        }
    }
}
