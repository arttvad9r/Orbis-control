//! Предупреждения.

use serde::{Deserialize, Serialize};

/// Уровень серьёзности предупреждения/записи диагностики.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    /// Информация.
    Info,
    /// Предупреждение.
    Warning,
    /// Ошибка.
    Error,
}

/// Предупреждение для UI (например, конфликт владельца профилей).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// Уровень.
    pub severity: WarningSeverity,
    /// Стабильный код (для маппинга на локализацию).
    pub code: String,
    /// Сообщение.
    pub message: String,
    /// Детали (backend, errno и т.п.).
    pub details: Option<String>,
}

impl Warning {
    /// Информационное предупреждение.
    pub fn info(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: WarningSeverity::Info,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Предупреждение.
    pub fn warn(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: WarningSeverity::Warning,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Ошибка.
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: WarningSeverity::Error,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors() {
        assert_eq!(Warning::info("a", "x").severity, WarningSeverity::Info);
        assert_eq!(Warning::warn("b", "x").severity, WarningSeverity::Warning);
        assert_eq!(Warning::error("c", "x").severity, WarningSeverity::Error);
    }
}
