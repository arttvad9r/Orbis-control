//! Состояние дисплея.

use serde::{Deserialize, Serialize};

use crate::newtypes::RefreshHz;

/// Режим обновления экрана.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshMode {
    /// Частота, Гц.
    pub hz: RefreshHz,
}

impl RefreshMode {
    /// Конструктор.
    pub fn new(hz: RefreshHz) -> Self {
        Self { hz }
    }
}

/// Состояние Panel Overdrive (бинарная ASUS firmware-настройка).
///
/// Backend-семантика (kernel `asus-armoury`, sysfs
/// `panel_overdrive/current_value`): `0` = выключено, `1` = включено.
/// Отсутствие чтения не подменяется значением `false`: неизвестное
/// состояние представлено отдельным вариантом [`PanelOverdriveState::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelOverdriveState {
    /// Overdrive выключен (0).
    Disabled,
    /// Overdrive включён (1).
    Enabled,
    /// Backend не предоставил определённое состояние (например, malformed value).
    Unknown,
}

/// Состояние HDR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrState {
    /// Неизвестно.
    Unknown,
    /// HDR поддерживается экраном.
    Supported,
    /// HDR выключен.
    Disabled,
    /// HDR включён (функции, несовместимые с HDR, отключаются).
    Enabled,
}

/// Состояние дисплея, агрегированное для UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayMode {
    /// Текущая частота внутреннего дисплея (если известна).
    pub current_hz: Option<RefreshHz>,
    /// Доступные частоты.
    pub modes: Vec<RefreshMode>,
    /// Panel Overdrive (asusd/asus-armoury), если доступен.
    pub overdrive: PanelOverdriveState,
    /// Состояние HDR.
    pub hdr: HdrState,
}

impl Default for DisplayMode {
    fn default() -> Self {
        Self {
            current_hz: None,
            modes: Vec::new(),
            overdrive: PanelOverdriveState::Unknown,
            hdr: HdrState::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_default() {
        let d = DisplayMode::default();
        assert!(d.current_hz.is_none());
        assert!(d.modes.is_empty());
        assert_eq!(d.overdrive, PanelOverdriveState::Unknown);
        assert_eq!(d.hdr, HdrState::Unknown);
    }

    #[test]
    fn panel_overdrive_state_preserves_disabled_unknown_distinction() {
        assert_ne!(PanelOverdriveState::Disabled, PanelOverdriveState::Unknown);
        assert_ne!(PanelOverdriveState::Enabled, PanelOverdriveState::Unknown);

        let json = serde_json::to_string(&PanelOverdriveState::Disabled).unwrap();
        assert_eq!(json, "\"disabled\"");
        let back: PanelOverdriveState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, PanelOverdriveState::Disabled);
    }

    #[test]
    fn display_with_modes() {
        let d = DisplayMode {
            current_hz: Some(RefreshHz::new(165).unwrap()),
            modes: vec![
                RefreshMode::new(RefreshHz::new(60).unwrap()),
                RefreshMode::new(RefreshHz::new(165).unwrap()),
            ],
            overdrive: PanelOverdriveState::Enabled,
            hdr: HdrState::Disabled,
        };
        assert_eq!(d.modes.len(), 2);
    }
}
