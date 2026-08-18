//! Read-only ASUS keyboard backlight brightness provider.
//!
//! Authority: kernel LED class `/sys/class/leds/asus::kbd_backlight/`.
//!
//! - `brightness` — текущий hardware level (read/write через kernel);
//! - `max_brightness` — максимальный hardware level (read-only);
//! - отсутствие LED device → `Unsupported`;
//! - malformed brightness/max → `Internal`, не default;
//! - current > max → inconsistency error;
//! - max НЕ hardcode-ится, всегда читается из sysfs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::keyboard_backlight::{KeyboardBacklightState, KeyboardBrightnessLevel};

use crate::error::ProviderError;
use crate::traits::{KeyboardBacklightProvider, Provider, ProviderHealth};

/// Фиксированный production path kernel LED class.
pub const ASUS_KBD_BACKLIGHT_DIR: &str = "/sys/class/leds/asus::kbd_backlight";

/// Read-only provider keyboard backlight brightness над kernel LED class.
pub struct AsusKeyboardBacklightProvider {
    led_dir: PathBuf,
}

impl AsusKeyboardBacklightProvider {
    /// Создать provider над указанным LED directory.
    pub fn new(led_dir: impl Into<PathBuf>) -> Self {
        Self {
            led_dir: led_dir.into(),
        }
    }

    /// Production default path.
    fn default_path() -> PathBuf {
        PathBuf::from(ASUS_KBD_BACKLIGHT_DIR)
    }

    /// Прочитать brightness (u32 из sysfs).
    fn read_brightness(&self) -> Result<u32, ProviderError> {
        let path = self.led_dir.join("brightness");
        let raw = std::fs::read_to_string(&path).map_err(|error| map_led_error(&path, error))?;
        let trimmed = raw.trim();
        trimmed.parse::<u32>().map_err(|error| {
            ProviderError::Internal(format!(
                "asus kbd_backlight: malformed brightness {:?}: {error}",
                trimmed
            ))
        })
    }

    /// Прочитать max_brightness (u32 из sysfs).
    fn read_max_brightness(&self) -> Result<u32, ProviderError> {
        let path = self.led_dir.join("max_brightness");
        let raw = std::fs::read_to_string(&path).map_err(|error| map_led_error(&path, error))?;
        let trimmed = raw.trim();
        trimmed.parse::<u32>().map_err(|error| {
            ProviderError::Internal(format!(
                "asus kbd_backlight: malformed max_brightness {:?}: {error}",
                trimmed
            ))
        })
    }
}

/// Map LED read error into typed provider error.
///
/// `NotFound` proves device absence → `Unsupported`.
/// `PermissionDenied` preserved distinctly.
/// Other I/O → `Io`.
fn map_led_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus kbd_backlight LED device absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus kbd_backlight read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

impl Default for AsusKeyboardBacklightProvider {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

impl Provider for AsusKeyboardBacklightProvider {
    fn id(&self) -> &'static str {
        "asus-kbd-backlight"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-kbd-backlight")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("asus kbd_backlight: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-kbd-backlight",
            "read-only kernel LED class keyboard backlight backend",
        )]
    }
}

#[async_trait]
impl KeyboardBacklightProvider for AsusKeyboardBacklightProvider {
    async fn keyboard_backlight_state(&self) -> Result<KeyboardBacklightState, ProviderError> {
        let max = self.read_max_brightness()?;
        let current = self.read_brightness()?;

        if current > max {
            return Err(ProviderError::Internal(format!(
                "asus kbd_backlight: current ({current}) > max ({max})"
            )));
        }

        Ok(KeyboardBacklightState {
            current: KeyboardBrightnessLevel::new(current as u8),
            max: KeyboardBrightnessLevel::new(max as u8),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn fixture_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbis-providers-kbd-backlight-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(brightness: &str, max: &str) -> (PathBuf, AsusKeyboardBacklightProvider) {
        let dir = fixture_root();
        fs::write(dir.join("brightness"), brightness).unwrap();
        fs::write(dir.join("max_brightness"), max).unwrap();
        let provider = AsusKeyboardBacklightProvider::new(&dir);
        (dir, provider)
    }

    #[tokio::test]
    async fn reads_valid_level() {
        let (dir, provider) = fixture("3\n", "3\n");
        let state = provider.keyboard_backlight_state().await.unwrap();
        assert_eq!(state.current.get(), 3);
        assert_eq!(state.max.get(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reads_zero_level() {
        let (dir, provider) = fixture("0\n", "3\n");
        let state = provider.keyboard_backlight_state().await.unwrap();
        assert_eq!(state.current.get(), 0);
        assert_eq!(state.max.get(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn device_missing_is_unsupported() {
        let dir = fixture_root();
        let provider = AsusKeyboardBacklightProvider::new(&dir);
        let error = provider
            .keyboard_backlight_state()
            .await
            .expect_err("missing device");
        assert!(matches!(error, ProviderError::Unsupported(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn malformed_brightness_is_internal() {
        let (dir, provider) = fixture("abc\n", "3\n");
        let error = provider
            .keyboard_backlight_state()
            .await
            .expect_err("malformed brightness");
        assert!(matches!(error, ProviderError::Internal(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn malformed_max_is_internal() {
        let (dir, provider) = fixture("0\n", "xyz\n");
        let error = provider
            .keyboard_backlight_state()
            .await
            .expect_err("malformed max");
        assert!(matches!(error, ProviderError::Internal(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn current_exceeding_max_is_internal() {
        let (dir, provider) = fixture("5\n", "3\n");
        let error = provider
            .keyboard_backlight_state()
            .await
            .expect_err("current > max");
        assert!(
            matches!(error, ProviderError::Internal(_)),
            "expected Internal for current>max"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn empty_brightness_is_internal() {
        let (dir, provider) = fixture("\n", "3\n");
        let error = provider
            .keyboard_backlight_state()
            .await
            .expect_err("empty brightness");
        assert!(matches!(error, ProviderError::Internal(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn fresh_reads_not_cached() {
        let dir = fixture_root();
        let path_brightness = dir.join("brightness");
        let path_max = dir.join("max_brightness");
        fs::write(&path_brightness, "1\n").unwrap();
        fs::write(&path_max, "3\n").unwrap();
        let provider = AsusKeyboardBacklightProvider::new(&dir);

        let state1 = provider.keyboard_backlight_state().await.unwrap();
        assert_eq!(state1.current.get(), 1);

        fs::write(&path_brightness, "2\n").unwrap();
        let state2 = provider.keyboard_backlight_state().await.unwrap();
        assert_eq!(state2.current.get(), 2);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn permission_denied_is_preserved() {
        let dir = fixture_root();
        let provider = AsusKeyboardBacklightProvider::new(&dir);
        let path = provider.led_dir.join("brightness");
        let error = map_led_error(
            &path,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn transient_io_error_is_preserved() {
        let dir = fixture_root();
        let provider = AsusKeyboardBacklightProvider::new(&dir);
        let path = provider.led_dir.join("brightness");
        let error = map_led_error(&path, std::io::Error::other("temporary"));
        assert!(matches!(error, ProviderError::Io(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn production_path_is_fixed() {
        assert_eq!(
            ASUS_KBD_BACKLIGHT_DIR,
            "/sys/class/leds/asus::kbd_backlight"
        );
    }
}
