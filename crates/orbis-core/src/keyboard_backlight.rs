//! Domain model для keyboard backlight brightness.
//!
//! Состояние описывает hardware level яркости клавиатуры (не percentage).
//! Max level определяется динамически из `max_brightness` sysfs файла —
//! не hardcode-ится.

use serde::{Deserialize, Serialize};

/// Hardware brightness level клавиатуры.
///
/// Сырой hardware index из kernel LED class (`/sys/class/leds/*/brightness`).
/// Не percentage и не Percent newtype: конкретный hardware-dependent level.
/// Диапазон определяется `max_brightness` из sysfs, не фиксируется в типе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyboardBrightnessLevel(u8);

impl KeyboardBrightnessLevel {
    /// Создать level из raw значения.
    pub fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// Доступ к сырому значению.
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for KeyboardBrightnessLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Текущее состояние keyboard backlight brightness.
///
/// Содержит текущий level и максимальный level (оба читаются из sysfs).
/// Не содержит capability status — это отдельный уровень (probe/provider).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardBacklightState {
    /// Текущий brightness level.
    pub current: KeyboardBrightnessLevel,
    /// Максимальный brightness level (из `max_brightness` sysfs).
    pub max: KeyboardBrightnessLevel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_roundtrip() {
        let level = KeyboardBrightnessLevel::new(3);
        assert_eq!(level.get(), 3);
    }

    #[test]
    fn level_zero_is_valid() {
        let level = KeyboardBrightnessLevel::new(0);
        assert_eq!(level.get(), 0);
    }

    #[test]
    fn state_serialization() {
        let state = KeyboardBacklightState {
            current: KeyboardBrightnessLevel::new(2),
            max: KeyboardBrightnessLevel::new(3),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: KeyboardBacklightState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }
}
