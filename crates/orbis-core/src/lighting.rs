//! Режимы подсветки.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Режим подсветки (клавиатура/Aura).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingMode {
    /// Выключено.
    Off,
    /// Статичный цвет.
    Static,
    /// Дыхание.
    Breathing,
    /// Строб.
    Strobing,
    /// Циклическая смена цвета.
    ColorCycle,
    /// Радуга.
    Rainbow,
    /// Специфичный для модели режим.
    Custom(String),
}

impl LightingMode {
    /// Разбор из строки (CLI/конфиг/тесты).
    pub fn parse(s: &str) -> std::result::Result<Self, CoreError> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "static" => Ok(Self::Static),
            "breathing" => Ok(Self::Breathing),
            "strobing" => Ok(Self::Strobing),
            "color-cycle" | "colorcycle" => Ok(Self::ColorCycle),
            "rainbow" => Ok(Self::Rainbow),
            other => Ok(Self::Custom(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        assert_eq!(LightingMode::parse("off").unwrap(), LightingMode::Off);
        assert_eq!(
            LightingMode::parse("breathing").unwrap(),
            LightingMode::Breathing
        );
        assert_eq!(
            LightingMode::parse("rainbow").unwrap(),
            LightingMode::Rainbow
        );
    }

    #[test]
    fn unknown_becomes_custom() {
        assert_eq!(
            LightingMode::parse("my-effect").unwrap(),
            LightingMode::Custom("my-effect".into())
        );
    }
}
