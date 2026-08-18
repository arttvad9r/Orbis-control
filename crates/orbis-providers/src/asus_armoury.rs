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
use orbis_core::display::{
    MiniLedModeState, MiniLedModeValue, PanelOverdriveState, ScreenAutoBrightnessState,
    interpret_mini_led_mode,
};
use orbis_core::identity::BackendIdentity;

use crate::error::ProviderError;
use crate::traits::{
    MiniLedModeProvider, PanelOverdriveProvider, Provider, ProviderHealth,
    ScreenAutoBrightnessProvider,
};

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

// ---------------------------------------------------------------------------
// MiniLED mode (read-only)
// ---------------------------------------------------------------------------

/// Фиксированный production relative directory kernel `asus-armoury` ABI.
///
/// Атрибут создаётся драйвером только если WMI devid
/// `ASUS_WMI_DEVID_MINI_LED_MODE` или `ASUS_WMI_DEVID_MINI_LED_MODE2`
/// присутствует (`armoury_has_devstate`).
pub const ASUS_ARMOURY_MINI_LED_MODE_RELATIVE_DIR: &str =
    "class/firmware-attributes/asus-armoury/attributes/mini_led_mode";

/// Распарсить authoritative `possible_values`.
///
/// Точный kernel ABI format (`armoury_attr_enum_list` в asus-armoury.c):
/// десятичные индексы, разделённые `;`, в порядке upstream, newline в конце.
/// Примеры: `"0;1\n"` (gen1), `"0;1;2\n"` (gen2). Документирован в
/// `Documentation/ABI/testing/sysfs-class-firmware-attributes`: значения
/// разделяются semi-colon (`;`).
///
/// Правила (fail-closed, без fake default):
/// - пустой список → `Internal` (kernel никогда не публикует пустой enum);
/// - malformed token → `Internal`;
/// - duplicate token → `Internal` (противоречивое enumeration evidence,
///   не нормализуется молча);
/// - порядок upstream сохраняется; сортировка не изобретается.
pub fn parse_possible_values(input: &str) -> Result<Vec<MiniLedModeValue>, ProviderError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ProviderError::Internal(
            "asus-armoury mini_led_mode possible_values пуст".into(),
        ));
    }
    let mut values = Vec::new();
    for token in trimmed.split(';') {
        let token = token.trim();
        if token.is_empty() {
            return Err(ProviderError::Internal(
                "asus-armoury mini_led_mode possible_values malformed empty token".to_string(),
            ));
        }
        let raw = token.parse::<u32>().map_err(|error| {
            ProviderError::Internal(format!(
                "asus-armoury mini_led_mode possible_values malformed token {token:?}: {error}"
            ))
        })?;
        let value = MiniLedModeValue::new(raw);
        if values.contains(&value) {
            return Err(ProviderError::Internal(format!(
                "asus-armoury mini_led_mode possible_values duplicate value: {raw}"
            )));
        }
        values.push(value);
    }
    Ok(values)
}

/// Распарсить fresh `current_value`.
///
/// Kernel format (`sysfs_emit(buf, "%u\n", i)`): один десятичный индекс.
/// Malformed/пустое → `Internal`, никогда не default.
pub fn parse_current_value(input: &str) -> Result<MiniLedModeValue, ProviderError> {
    let trimmed = input.trim();
    let raw = trimmed.parse::<u32>().map_err(|error| {
        ProviderError::Internal(format!(
            "asus-armoury mini_led_mode current_value malformed {trimmed:?}: {error}"
        ))
    })?;
    Ok(MiniLedModeValue::new(raw))
}

/// Map a read error on the MiniLED attribute path.
///
/// Двухуровневая классификация (в отличие от Panel Overdrive, где отсутствие
/// атрибута flat-маппится в `Unsupported`):
/// - `NotFound` + отсутствующий backend root (`asus-armoury`) →
///   `BackendUnavailable` (backend не установлен);
/// - `NotFound` + существующий backend root → `Unsupported` (capability не
///   присутствует на этом устройстве);
/// - `PermissionDenied` сохраняется отдельно;
/// - остальные I/O ошибки → `Io` (probe классифицирует как Unknown/временные).
fn map_mini_led_error(path: &Path, error: std::io::Error, backend_root: &Path) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            if backend_root.exists() {
                ProviderError::Unsupported(format!(
                    "asus-armoury mini_led_mode attribute absent: {}",
                    path.display()
                ))
            } else {
                ProviderError::BackendUnavailable(format!(
                    "asus-armoury backend absent: {}",
                    backend_root.display()
                ))
            }
        }
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury mini_led_mode read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

/// Map a read error on the MiniLED `current_value` after `possible_values` was
/// already readable.
///
/// `current_value` mandatory по ABI; его отсутствие при наличии
/// `possible_values` — противоречивое/частичное evidence → `Internal`, а не
/// `Unsupported`/default.
fn map_mini_led_current_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Internal(format!(
            "asus-armoury mini_led_mode current_value отсутствует при наличии possible_values: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury mini_led_mode read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

/// Read-only MiniLED mode provider над kernel firmware-attributes ABI.
///
/// Корень sysfs инъектируется извне (production — `/sys`, тесты — временное
/// fixture-дерево). Конструктор не выполняет I/O.
///
/// Read algorithm (fresh, без кэша):
/// 1. прочитать authoritative `possible_values`;
/// 2. распарсить allowed set (fail-closed);
/// 3. прочитать fresh `current_value`;
/// 4. проверить consistency (current ∈ allowed, иначе `Internal`);
/// 5. вывести optional semantic label через [`interpret_mini_led_mode`];
/// 6. вернуть typed state.
pub struct AsusArmouryMiniLedModeProvider {
    sysfs_root: PathBuf,
}

impl AsusArmouryMiniLedModeProvider {
    /// Создать provider над заданным корнем sysfs.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    fn attribute_dir(&self) -> PathBuf {
        self.sysfs_root
            .join(ASUS_ARMOURY_MINI_LED_MODE_RELATIVE_DIR)
    }

    fn backend_root(&self) -> PathBuf {
        self.sysfs_root
            .join("class/firmware-attributes/asus-armoury")
    }
}

impl Default for AsusArmouryMiniLedModeProvider {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

impl Provider for AsusArmouryMiniLedModeProvider {
    fn id(&self) -> &'static str {
        "asus-armoury-mini-led-mode"
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
            "provider.asus-armoury-mini-led-mode",
            "read-only kernel asus-armoury mini_led_mode backend",
        )]
    }
}

#[async_trait]
impl MiniLedModeProvider for AsusArmouryMiniLedModeProvider {
    async fn mini_led_mode_state(&self) -> Result<MiniLedModeState, ProviderError> {
        let dir = self.attribute_dir();
        let possible_path = dir.join("possible_values");
        let current_path = dir.join("current_value");

        let possible_raw = std::fs::read_to_string(&possible_path)
            .map_err(|error| map_mini_led_error(&possible_path, error, &self.backend_root()))?;
        let allowed = parse_possible_values(&possible_raw)?;

        let current_raw = std::fs::read_to_string(&current_path)
            .map_err(|error| map_mini_led_current_error(&current_path, error))?;
        let current = parse_current_value(&current_raw)?;

        // Consistency: current вне authoritative allowed set — противоречивое
        // backend evidence. Честно rejected как Internal, а не представлен как
        // валидное состояние (и не как default).
        if !allowed.contains(&current) {
            return Err(ProviderError::Internal(format!(
                "asus-armoury mini_led_mode current value {} вне allowed set {:?}",
                current.raw(),
                allowed.iter().map(|v| v.raw()).collect::<Vec<_>>()
            )));
        }

        let semantics = interpret_mini_led_mode(&allowed, current);
        Ok(MiniLedModeState {
            allowed,
            current,
            semantics,
        })
    }
}

// ---------------------------------------------------------------------------
// Screen Auto Brightness (read-only)
// ---------------------------------------------------------------------------

/// Фиксированный production relative path kernel `asus-armoury` ABI.
///
/// Атрибут создаётся драйвером только если WMI devid
/// `ASUS_WMI_DEVID_SCREEN_AUTO_BRIGHTNESS` (0x0005002A) присутствует
/// (`armoury_has_devstate`). Upstream: commit `7725a2dc5863` «add screen
/// auto-brightness toggle», `ASUS_ATTR_GROUP_BOOL_RW(screen_auto_brightness,
/// ..., "Set the panel brightness to Off<0> or On<1>")`.
pub const ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH: &str =
    "class/firmware-attributes/asus-armoury/attributes/screen_auto_brightness/current_value";

/// Read-only Screen Auto Brightness provider над kernel firmware-attributes ABI.
///
/// Корень sysfs инъектируется извне (production — `/sys`, тесты — временное
/// fixture-дерево). Конструктор не выполняет I/O.
///
/// Read algorithm (fresh, без кэша):
/// 1. прочитать authoritative `current_value`;
/// 2. распарсить `0`/`1` (bool polarity документирована upstream);
/// 3. вернуть typed state.
///
/// `possible_values` для этого bool-атрибута статичен (`"0;1"` из
/// `ASUS_ATTR_GROUP_BOOL_RW`), поэтому consistency проверяется на уровне
/// парсера: любое значение вне `0`/`1` — malformed evidence → `Internal`,
/// а не fake default.
pub struct AsusArmouryScreenAutoBrightnessProvider {
    sysfs_root: PathBuf,
}

impl AsusArmouryScreenAutoBrightnessProvider {
    /// Создать provider над заданным корнем sysfs.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    fn current_value_path(&self) -> PathBuf {
        self.sysfs_root
            .join(ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH)
    }

    fn backend_root(&self) -> PathBuf {
        self.sysfs_root
            .join("class/firmware-attributes/asus-armoury")
    }
}

impl Default for AsusArmouryScreenAutoBrightnessProvider {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

impl Provider for AsusArmouryScreenAutoBrightnessProvider {
    fn id(&self) -> &'static str {
        "asus-armoury-screen-auto-brightness"
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
            "provider.asus-armoury-screen-auto-brightness",
            "read-only kernel asus-armoury screen_auto_brightness backend",
        )]
    }
}

/// Map a read error on the Screen Auto Brightness attribute path.
///
/// Двухуровневая классификация (как MiniLED):
/// - `NotFound` + отсутствующий backend root (`asus-armoury`) →
///   `BackendUnavailable` (backend не установлен);
/// - `NotFound` + существующий backend root → `Unsupported` (capability не
///   присутствует на этом устройстве);
/// - `PermissionDenied` сохраняется отдельно;
/// - остальные I/O ошибки → `Io`.
fn map_screen_auto_brightness_error(
    path: &Path,
    error: std::io::Error,
    backend_root: &Path,
) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            if backend_root.exists() {
                ProviderError::Unsupported(format!(
                    "asus-armoury screen_auto_brightness attribute absent: {}",
                    path.display()
                ))
            } else {
                ProviderError::BackendUnavailable(format!(
                    "asus-armoury backend absent: {}",
                    backend_root.display()
                ))
            }
        }
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury screen_auto_brightness read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

#[async_trait]
impl ScreenAutoBrightnessProvider for AsusArmouryScreenAutoBrightnessProvider {
    async fn screen_auto_brightness_state(
        &self,
    ) -> Result<ScreenAutoBrightnessState, ProviderError> {
        let path = self.current_value_path();
        let raw = std::fs::read_to_string(&path).map_err(|error| {
            map_screen_auto_brightness_error(&path, error, &self.backend_root())
        })?;

        match raw.trim() {
            "0" => Ok(ScreenAutoBrightnessState::Disabled),
            "1" => Ok(ScreenAutoBrightnessState::Enabled),
            other => Err(ProviderError::Internal(format!(
                "asus-armoury screen_auto_brightness malformed value: {other:?}"
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

    // -- MiniLED: parser -----------------------------------------------------

    fn values(raws: &[u32]) -> Vec<MiniLedModeValue> {
        raws.iter().copied().map(MiniLedModeValue::new).collect()
    }

    #[test]
    fn parse_possible_values_two_and_three_generation() {
        assert_eq!(parse_possible_values("0;1\n").unwrap(), values(&[0, 1]));
        assert_eq!(
            parse_possible_values("0;1;2\n").unwrap(),
            values(&[0, 1, 2])
        );
    }

    #[test]
    fn parse_possible_values_tolerates_whitespace_newlines() {
        assert_eq!(parse_possible_values("0;1").unwrap(), values(&[0, 1]));
        assert_eq!(parse_possible_values("0;1\n\n").unwrap(), values(&[0, 1]));
        assert_eq!(parse_possible_values(" 0 ; 1 \n").unwrap(), values(&[0, 1]));
        assert_eq!(
            parse_possible_values("0;1;2;3\n").unwrap(),
            values(&[0, 1, 2, 3])
        );
    }

    #[test]
    fn parse_possible_values_preserves_upstream_order() {
        // Сортировка не изобретается; upstream порядок сохраняется.
        assert_eq!(parse_possible_values("1;0\n").unwrap(), values(&[1, 0]));
    }

    #[test]
    fn parse_possible_values_rejects_malformed_token() {
        for malformed in ["0;x\n", "0;1;abc\n", "0;;1\n", "0;\n", "-1;0\n", "0;1\n0"] {
            let error = parse_possible_values(malformed).expect_err("malformed");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "malformed {malformed:?} must be Internal, got {error:?}"
            );
        }
    }

    #[test]
    fn parse_possible_values_rejects_empty_list() {
        for empty in ["", "\n", "   \n"] {
            let error = parse_possible_values(empty).expect_err("empty");
            assert!(matches!(error, ProviderError::Internal(_)));
        }
    }

    #[test]
    fn parse_possible_values_rejects_duplicates_deterministically() {
        for duplicated in ["0;0\n", "0;1;1\n", "0;1;0\n"] {
            let error = parse_possible_values(duplicated).expect_err("duplicate");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "duplicate {duplicated:?} must be Internal, got {error:?}"
            );
        }
    }

    #[test]
    fn parse_current_value_accepts_known_and_future_values() {
        assert_eq!(
            parse_current_value("0\n").unwrap(),
            MiniLedModeValue::new(0)
        );
        assert_eq!(
            parse_current_value("1\n").unwrap(),
            MiniLedModeValue::new(1)
        );
        assert_eq!(
            parse_current_value("2\n").unwrap(),
            MiniLedModeValue::new(2)
        );
        assert_eq!(
            parse_current_value("3\n").unwrap(),
            MiniLedModeValue::new(3)
        );
        assert_eq!(
            parse_current_value(" 1 \n").unwrap(),
            MiniLedModeValue::new(1)
        );
    }

    #[test]
    fn parse_current_value_rejects_malformed() {
        for malformed in ["abc\n", "\n", "", "0 1\n", "-1\n", "0;1\n"] {
            let error = parse_current_value(malformed).expect_err("malformed");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "malformed {malformed:?} must be Internal, got {error:?}"
            );
        }
    }

    // -- MiniLED: provider ---------------------------------------------------

    fn mini_led_fixture(
        possible: &str,
        current: &str,
    ) -> (PathBuf, AsusArmouryMiniLedModeProvider) {
        let dir = fixture_root();
        let attr_dir = dir.join(ASUS_ARMOURY_MINI_LED_MODE_RELATIVE_DIR);
        fs::create_dir_all(&attr_dir).unwrap();
        fs::write(attr_dir.join("possible_values"), possible).unwrap();
        fs::write(attr_dir.join("current_value"), current).unwrap();
        let provider = AsusArmouryMiniLedModeProvider::new(&dir);
        (dir, provider)
    }

    fn mini_led_state(
        allowed: &[u32],
        current: u32,
        semantics: Option<orbis_core::display::MiniLedModeKind>,
    ) -> MiniLedModeState {
        MiniLedModeState {
            allowed: values(allowed),
            current: MiniLedModeValue::new(current),
            semantics,
        }
    }

    #[tokio::test]
    async fn mini_led_current_0_allowed_0_1() {
        let (dir, provider) = mini_led_fixture("0;1\n", "0\n");
        assert_eq!(
            provider.mini_led_mode_state().await.unwrap(),
            mini_led_state(&[0, 1], 0, Some(orbis_core::display::MiniLedModeKind::Off))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_current_1_allowed_0_1() {
        let (dir, provider) = mini_led_fixture("0;1\n", "1\n");
        assert_eq!(
            provider.mini_led_mode_state().await.unwrap(),
            mini_led_state(&[0, 1], 1, Some(orbis_core::display::MiniLedModeKind::On))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_current_0_allowed_0_1_2() {
        let (dir, provider) = mini_led_fixture("0;1;2\n", "0\n");
        assert_eq!(
            provider.mini_led_mode_state().await.unwrap(),
            mini_led_state(
                &[0, 1, 2],
                0,
                Some(orbis_core::display::MiniLedModeKind::Off)
            )
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_current_1_allowed_0_1_2() {
        let (dir, provider) = mini_led_fixture("0;1;2\n", "1\n");
        assert_eq!(
            provider.mini_led_mode_state().await.unwrap(),
            mini_led_state(
                &[0, 1, 2],
                1,
                Some(orbis_core::display::MiniLedModeKind::Weak)
            )
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_current_2_allowed_0_1_2() {
        let (dir, provider) = mini_led_fixture("0;1;2\n", "2\n");
        assert_eq!(
            provider.mini_led_mode_state().await.unwrap(),
            mini_led_state(
                &[0, 1, 2],
                2,
                Some(orbis_core::display::MiniLedModeKind::Strong)
            )
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_unknown_future_value_round_trips() {
        // Будущий enum (possible_values 0;1;2;3) не ломает read: raw 3
        // сохраняется, семантика честно отсутствует (None).
        let (dir, provider) = mini_led_fixture("0;1;2;3\n", "3\n");
        assert_eq!(
            provider.mini_led_mode_state().await.unwrap(),
            mini_led_state(&[0, 1, 2, 3], 3, None)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_zero_is_not_absent() {
        // `0` не становится None/absent: current = MiniLedModeValue(0).
        let (dir, provider) = mini_led_fixture("0;1\n", "0\n");
        let state = provider.mini_led_mode_state().await.unwrap();
        assert_eq!(state.current, MiniLedModeValue::new(0));
        assert_eq!(u32::from(state.current), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_missing_attribute_is_unsupported() {
        // Backend (asus-armoury) присутствует, но атрибут mini_led_mode нет.
        let dir = fixture_root();
        let backend_root = dir.join("class/firmware-attributes/asus-armoury");
        fs::create_dir_all(backend_root.join("attributes")).unwrap();
        let provider = AsusArmouryMiniLedModeProvider::new(&dir);
        let error = provider
            .mini_led_mode_state()
            .await
            .expect_err("attribute absent");
        assert!(
            matches!(error, ProviderError::Unsupported(_)),
            "missing attribute must be Unsupported, got {error:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_backend_missing_is_backend_unavailable() {
        // Backend root (asus-armoury) отсутствует полностью.
        let dir = fixture_root();
        let provider = AsusArmouryMiniLedModeProvider::new(&dir);
        let error = provider
            .mini_led_mode_state()
            .await
            .expect_err("backend absent");
        assert!(
            matches!(error, ProviderError::BackendUnavailable(_)),
            "backend missing must be BackendUnavailable, got {error:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mini_led_permission_denied_is_preserved() {
        let dir = fixture_root();
        let provider = AsusArmouryMiniLedModeProvider::new(&dir);
        let path = provider.attribute_dir().join("possible_values");
        let backend_root = provider.backend_root();
        let error = map_mini_led_error(
            &path,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            &backend_root,
        );
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
        assert!(!matches!(error, ProviderError::Unsupported(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mini_led_transient_io_error_is_preserved() {
        let dir = fixture_root();
        let provider = AsusArmouryMiniLedModeProvider::new(&dir);
        let path = provider.attribute_dir().join("possible_values");
        let backend_root = provider.backend_root();
        let error = map_mini_led_error(&path, std::io::Error::other("temporary"), &backend_root);
        assert!(matches!(error, ProviderError::Io(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_malformed_current_is_internal_not_unsupported() {
        for malformed in ["abc\n", "\n", "0 1\n"] {
            let (dir, provider) = mini_led_fixture("0;1\n", malformed);
            let error = provider
                .mini_led_mode_state()
                .await
                .expect_err("malformed current");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "malformed current {malformed:?} must be Internal, got {error:?}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn mini_led_malformed_possible_values_is_internal_not_unsupported() {
        for malformed in ["0;x\n", "0;;1\n", "0;0;1\n", "\n"] {
            let (dir, provider) = mini_led_fixture(malformed, "0\n");
            let error = provider
                .mini_led_mode_state()
                .await
                .expect_err("malformed possible_values");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "malformed possible_values {malformed:?} must be Internal, got {error:?}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn mini_led_current_outside_allowed_set_is_rejected_explicitly() {
        let (dir, provider) = mini_led_fixture("0;1\n", "2\n");
        let error = provider
            .mini_led_mode_state()
            .await
            .expect_err("current outside allowed");
        assert!(
            matches!(error, ProviderError::Internal(_)),
            "current outside allowed set must be explicit Internal, got {error:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_current_missing_after_possible_values_is_internal() {
        let dir = fixture_root();
        let attr_dir = dir.join(ASUS_ARMOURY_MINI_LED_MODE_RELATIVE_DIR);
        fs::create_dir_all(&attr_dir).unwrap();
        fs::write(attr_dir.join("possible_values"), "0;1\n").unwrap();
        let provider = AsusArmouryMiniLedModeProvider::new(&dir);
        let error = provider
            .mini_led_mode_state()
            .await
            .expect_err("current_value absent");
        assert!(
            matches!(error, ProviderError::Internal(_)),
            "partial attribute must be Internal, got {error:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mini_led_failure_does_not_corrupt_panel_overdrive() {
        // Один malformed ASUS firmware attribute не ломает соседние
        // capabilities: mini_led отсутствует/сломан, panel_overdrive читается.
        let dir = fixture_root();
        // MiniLED сломан (malformed current).
        let attr_dir = dir.join(ASUS_ARMOURY_MINI_LED_MODE_RELATIVE_DIR);
        fs::create_dir_all(&attr_dir).unwrap();
        fs::write(attr_dir.join("possible_values"), "0;1\n").unwrap();
        fs::write(attr_dir.join("current_value"), "abc\n").unwrap();
        // Panel Overdrive валиден.
        let panel_dir = dir.join(ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH);
        fs::create_dir_all(panel_dir.parent().unwrap()).unwrap();
        fs::write(&panel_dir, "1\n").unwrap();

        let mini = AsusArmouryMiniLedModeProvider::new(&dir);
        let panel = AsusArmouryPanelOverdriveProvider::new(&dir);

        assert!(matches!(
            mini.mini_led_mode_state().await,
            Err(ProviderError::Internal(_))
        ));
        assert_eq!(
            panel.panel_overdrive_state().await.unwrap(),
            PanelOverdriveState::Enabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    // -- Screen Auto Brightness ---------------------------------------------

    fn sab_fixture(current: &str) -> (PathBuf, AsusArmouryScreenAutoBrightnessProvider) {
        let dir = fixture_root();
        let path = dir.join(ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, current).unwrap();
        let provider = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        (dir, provider)
    }

    #[tokio::test]
    async fn sab_valid_disabled_state() {
        let (dir, provider) = sab_fixture("0\n");
        assert_eq!(
            provider.screen_auto_brightness_state().await.unwrap(),
            ScreenAutoBrightnessState::Disabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_valid_enabled_state() {
        let (dir, provider) = sab_fixture("1\n");
        assert_eq!(
            provider.screen_auto_brightness_state().await.unwrap(),
            ScreenAutoBrightnessState::Enabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_false_is_not_none() {
        // `Some(false)`-аналог: Disabled не становится Unknown/absent.
        let (dir, provider) = sab_fixture("0\n");
        let state = provider.screen_auto_brightness_state().await.unwrap();
        assert_eq!(state, ScreenAutoBrightnessState::Disabled);
        assert_ne!(state, ScreenAutoBrightnessState::Unknown);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_reads_are_fresh_not_cached() {
        let dir = fixture_root();
        let path = dir.join(ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "0\n").unwrap();
        let provider = AsusArmouryScreenAutoBrightnessProvider::new(&dir);

        assert_eq!(
            provider.screen_auto_brightness_state().await.unwrap(),
            ScreenAutoBrightnessState::Disabled
        );
        fs::write(&path, "1\n").unwrap();
        assert_eq!(
            provider.screen_auto_brightness_state().await.unwrap(),
            ScreenAutoBrightnessState::Enabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_attribute_missing_is_unsupported() {
        // Backend (asus-armoury) присутствует, но атрибут отсутствует.
        let dir = fixture_root();
        let backend_root = dir.join("class/firmware-attributes/asus-armoury");
        fs::create_dir_all(backend_root.join("attributes")).unwrap();
        let provider = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        let error = provider
            .screen_auto_brightness_state()
            .await
            .expect_err("attribute absent");
        assert!(
            matches!(error, ProviderError::Unsupported(_)),
            "missing attribute must be Unsupported, got {error:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_backend_missing_is_backend_unavailable() {
        let dir = fixture_root();
        let provider = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        let error = provider
            .screen_auto_brightness_state()
            .await
            .expect_err("backend absent");
        assert!(
            matches!(error, ProviderError::BackendUnavailable(_)),
            "backend missing must be BackendUnavailable, got {error:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sab_permission_denied_is_preserved() {
        let dir = fixture_root();
        let provider = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        let path = provider.current_value_path();
        let backend_root = provider.backend_root();
        let error = map_screen_auto_brightness_error(
            &path,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            &backend_root,
        );
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
        assert!(!matches!(error, ProviderError::Unsupported(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sab_transient_io_error_is_preserved() {
        let dir = fixture_root();
        let provider = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        let path = provider.current_value_path();
        let backend_root = provider.backend_root();
        let error = map_screen_auto_brightness_error(
            &path,
            std::io::Error::other("temporary"),
            &backend_root,
        );
        assert!(matches!(error, ProviderError::Io(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_malformed_current_is_internal_not_unsupported() {
        for malformed in ["2\n", "true\n", "abc\n", "0 1\n", "-1\n"] {
            let (dir, provider) = sab_fixture(malformed);
            let error = provider
                .screen_auto_brightness_state()
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
    async fn sab_empty_current_is_internal_not_false() {
        for empty in ["\n", "", "   \n"] {
            let (dir, provider) = sab_fixture(empty);
            let error = provider
                .screen_auto_brightness_state()
                .await
                .expect_err("empty");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "empty {empty:?} must be Internal, got {error:?}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn sab_unexpected_numeric_value_is_rejected() {
        // Upstream bool contract — только 0/1; 2 и выше не допускается.
        for unexpected in ["2\n", "3\n", "255\n"] {
            let (dir, provider) = sab_fixture(unexpected);
            let error = provider
                .screen_auto_brightness_state()
                .await
                .expect_err("unexpected numeric");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "unexpected {unexpected:?} must be Internal, got {error:?}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn sab_failure_does_not_corrupt_mini_led() {
        // Screen Auto Brightness сломан (malformed), MiniLED валиден.
        let dir = fixture_root();
        let sab_path = dir.join(ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH);
        fs::create_dir_all(sab_path.parent().unwrap()).unwrap();
        fs::write(&sab_path, "abc\n").unwrap();
        let mini_dir = dir.join(ASUS_ARMOURY_MINI_LED_MODE_RELATIVE_DIR);
        fs::create_dir_all(&mini_dir).unwrap();
        fs::write(mini_dir.join("possible_values"), "0;1\n").unwrap();
        fs::write(mini_dir.join("current_value"), "1\n").unwrap();

        let sab = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        let mini = AsusArmouryMiniLedModeProvider::new(&dir);

        assert!(matches!(
            sab.screen_auto_brightness_state().await,
            Err(ProviderError::Internal(_))
        ));
        assert_eq!(
            mini.mini_led_mode_state().await.unwrap().current,
            MiniLedModeValue::new(1)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_failure_does_not_corrupt_panel_overdrive() {
        let dir = fixture_root();
        let sab_path = dir.join(ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH);
        fs::create_dir_all(sab_path.parent().unwrap()).unwrap();
        fs::write(&sab_path, "abc\n").unwrap();
        let panel_path = dir.join(ASUS_ARMOURY_PANEL_OVERDRIVE_RELATIVE_PATH);
        fs::create_dir_all(panel_path.parent().unwrap()).unwrap();
        fs::write(&panel_path, "1\n").unwrap();

        let sab = AsusArmouryScreenAutoBrightnessProvider::new(&dir);
        let panel = AsusArmouryPanelOverdriveProvider::new(&dir);

        assert!(matches!(
            sab.screen_auto_brightness_state().await,
            Err(ProviderError::Internal(_))
        ));
        assert_eq!(
            panel.panel_overdrive_state().await.unwrap(),
            PanelOverdriveState::Enabled
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn sab_production_relative_path_is_fixed() {
        assert_eq!(
            ASUS_ARMOURY_SCREEN_AUTO_BRIGHTNESS_RELATIVE_PATH,
            "class/firmware-attributes/asus-armoury/attributes/screen_auto_brightness/current_value"
        );
    }
}
