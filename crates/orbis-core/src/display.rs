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
    pub overdrive: Option<bool>,
    /// Состояние HDR.
    pub hdr: HdrState,
}

impl Default for DisplayMode {
    fn default() -> Self {
        Self {
            current_hz: None,
            modes: Vec::new(),
            overdrive: None,
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
        assert_eq!(d.hdr, HdrState::Unknown);
    }

    #[test]
    fn display_with_modes() {
        let d = DisplayMode {
            current_hz: Some(RefreshHz::new(165).unwrap()),
            modes: vec![
                RefreshMode::new(RefreshHz::new(60).unwrap()),
                RefreshMode::new(RefreshHz::new(165).unwrap()),
            ],
            overdrive: Some(true),
            hdr: HdrState::Disabled,
        };
        assert_eq!(d.modes.len(), 2);
    }
}
