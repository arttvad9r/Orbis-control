//! Read-only authoritative ASUS Armoury firmware-attribute provider.
//!
//! Реализует Panel Overdrive read path поверх kernel `asus-armoury` ABI
//! (evidence: `drivers/platform/x86/asus-armoury.c`, `ASUS_ATTR_GROUP_BOOL_RW
//! (panel_od, "panel_overdrive", ASUS_WMI_DEVID_PANEL_OD, ...)`):
//!
//! - read: `/sys/class/firmware-attributes/asus-armoury/attributes/panel_overdrive/current_value`
//!   (bool; `0` = off, `1` = on);
//! - присутствие атрибута доказывается наличием/чтением файла, а не
//!   предположением по модели ноутбука;
//! - каждый вызов выполняет fresh authoritative read; кэш отсутствует;
//! - отсутствующий атрибут (NotFound) → `Unsupported`, а не `Disabled`;
//! - malformed значение (не `0`/`1`) → `Internal`, а не fake default;
//! - `PermissionDenied` сохраняется отдельно от `Unsupported`.
//!
//! Провайдер только читает. Никаких writes и никакого direct mutation API.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::display::PanelOverdriveState;
use orbis_core::identity::BackendIdentity;

use crate::error::ProviderError;
use crate::traits::{PanelOverdriveProvider, Provider, ProviderHealth};

/// Фиксированный production relative path kernel `asus-armoury` ABI.
///
/// Атрибут создаётся драйвером только если WMI devid
/// `ASUS_WMI_DEVID_PANEL_OD` присутствует (`armoury_has_devstate`).
pub const ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH: &str =
    "class/firmware-attributes/asus-armoury/attributes/panel_overdrive/current_value";

/// Read-only Panel Overdrive provider над kernel firmware-attributes ABI.
///
/// Корень sysfs инъектируется извне (production — `/sys`, тесты — временное
/// fixture-дерево). Конструктор не выполняет I/O.
pub struct AsusArmouryPanelOverdriveProvider {
    sysfs_root: PathBuf,
}

impl AsusArmouryPanelOverdriveProvider {
    /// Создать provider над заданным корнем sysfs.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    /// Производственный путь authoritative current value.
    fn current_value_path(&self) -> PathBuf {
        self.sysfs_root
            .join(ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH)
    }
}

impl Default for AsusArmouryPanelOverdriveProvider {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

impl Provider for AsusArmouryPanelOverdriveProvider {
    fn id(&self) -> &'static str {
        "asus-armoury-panel-overdrive"
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
            "provider.asus-armoury-panel-overdrive",
            "read-only kernel asus-armoury panel_overdrive backend",
        )]
    }
}

/// Map a read error on the fixed attribute path into the typed provider error.
///
/// `NotFound` proves structural absence (attribute not created by the driver),
/// `PermissionDenied` is preserved distinctly, every other I/O failure stays an
/// I/O error. This mapping is deliberately pure so the classification is
/// deterministic and testable without touching the real filesystem.
fn map_read_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus-armoury panel_overdrive attribute absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury panel_overdrive read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

#[async_trait]
impl PanelOverdriveProvider for AsusArmouryPanelOverdriveProvider {
    async fn panel_overdrive_state(&self) -> Result<PanelOverdriveState, ProviderError> {
        let path = self.current_value_path();
        let raw = std::fs::read_to_string(&path).map_err(|error| map_read_error(&path, error))?;

        match raw.trim() {
            "0" => Ok(PanelOverdriveState::Disabled),
            "1" => Ok(PanelOverdriveState::Enabled),
            other => Err(ProviderError::Internal(format!(
                "asus-armoury panel_overdrive malformed value: {other:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Create a unique temporary fixture root (house pattern: no tempfile dep).
    fn fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbis-providers-panel-overdrive-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(content: &str) -> (std::path::PathBuf, AsusArmouryPanelOverdriveProvider) {
        let dir = fixture_root();
        let path = dir.join(ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        let provider = AsusArmouryPanelOverdriveProvider::new(&dir);
        (dir, provider)
    }

    #[tokio::test]
    async fn reads_zero_as_disabled() {
        let (dir, provider) = fixture("0\n");
        assert_eq!(
            provider.panel_overdrive_state().await.unwrap(),
            PanelOverdriveState::Disabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reads_one_as_enabled() {
        let (dir, provider) = fixture("1\n");
        assert_eq!(
            provider.panel_overdrive_state().await.unwrap(),
            PanelOverdriveState::Enabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn disabled_is_not_unknown() {
        // `Some(false)`-аналог: выключенное состояние не становится Unknown.
        let (dir, provider) = fixture("0\n");
        let state = provider.panel_overdrive_state().await.unwrap();
        assert_eq!(state, PanelOverdriveState::Disabled);
        assert_ne!(state, PanelOverdriveState::Unknown);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reads_are_fresh_not_cached() {
        let dir = fixture_root();
        let path = dir.join(ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "0\n").unwrap();
        let provider = AsusArmouryPanelOverdriveProvider::new(&dir);

        assert_eq!(
            provider.panel_overdrive_state().await.unwrap(),
            PanelOverdriveState::Disabled
        );
        fs::write(&path, "1\n").unwrap();
        assert_eq!(
            provider.panel_overdrive_state().await.unwrap(),
            PanelOverdriveState::Enabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_attribute_is_unsupported() {
        let dir = fixture_root();
        let provider = AsusArmouryPanelOverdriveProvider::new(&dir);
        let path = provider.current_value_path();
        let error = map_read_error(
            &path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "absent"),
        );
        assert!(matches!(error, ProviderError::Unsupported(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn permission_denied_is_preserved_separate_from_unsupported() {
        let dir = fixture_root();
        let provider = AsusArmouryPanelOverdriveProvider::new(&dir);
        let path = provider.current_value_path();
        let error = map_read_error(
            &path,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
        assert!(!matches!(error, ProviderError::Unsupported(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn other_io_errors_stay_io() {
        let dir = fixture_root();
        let provider = AsusArmouryPanelOverdriveProvider::new(&dir);
        let path = provider.current_value_path();
        let error = map_read_error(&path, std::io::Error::other("temporary"));
        assert!(matches!(error, ProviderError::Io(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn malformed_value_is_internal_not_fake_default() {
        for malformed in ["2\n", "true\n", "abc\n", "\n", ""] {
            let (dir, provider) = fixture(malformed);
            let error = provider
                .panel_overdrive_state()
                .await
                .expect_err("malformed");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "malformed {malformed:?} must be Internal, got {error:?}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn production_relative_path_is_fixed() {
        assert_eq!(
            ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH,
            "class/firmware-attributes/asus-armoury/attributes/panel_overdrive/current_value"
        );
    }
}
