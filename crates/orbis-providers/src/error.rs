//! Ошибки и вспомогательные типы провайдеров.

use thiserror::Error;

/// Идентификатор операции (для журналирования и отмены устаревших запросов).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct OperationId(pub String);

impl OperationId {
    /// Новый идентификатор.
    pub fn new() -> Self {
        Self(format!("op-{}", uuid_like()))
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Простой генератор уникального суффикса (без внешних зависимостей).
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{}", std::process::id())
}

/// Результат валидации запроса перед записью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationResult {
    /// Запрос допустим.
    Valid,
    /// Запрос недопустим (с человекочитаемой причиной).
    Invalid(String),
}

impl ValidationResult {
    /// Удобный конструктор.
    pub fn ok() -> Self {
        Self::Valid
    }

    /// Удобный конструктор ошибки.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// Удобно: Ok(()) или Err(ProviderError::InvalidRequest).
    pub fn into_result(self) -> Result<(), ProviderError> {
        match self {
            Self::Valid => Ok(()),
            Self::Invalid(msg) => Err(ProviderError::InvalidRequest(msg)),
        }
    }
}

/// Ошибки провайдеров.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Backend недоступен (сервис не запущен, D-Bus нет).
    #[error("backend недоступен: {0}")]
    BackendUnavailable(String),
    /// Нет поддержки функции.
    #[error("функция не поддерживается: {0}")]
    Unsupported(String),
    /// Недостаточно прав.
    #[error("недостаточно прав: {0}")]
    PermissionDenied(String),
    /// Запрос не прошёл валидацию.
    #[error("невалидный запрос: {0}")]
    InvalidRequest(String),
    /// Таймаут операции.
    #[error("таймаут операции: {0}")]
    Timeout(String),
    /// Ошибка ввода-вывода.
    #[error("io: {0}")]
    Io(#[source] std::io::Error),
    /// Ошибка D-Bus.
    #[error("dbus: {0}")]
    Dbus(String),
    /// Внутренняя ошибка.
    #[error("внутренняя ошибка: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_id_unique() {
        assert_ne!(OperationId::new(), OperationId::new());
    }

    #[test]
    fn validation_helpers() {
        assert!(ValidationResult::ok().into_result().is_ok());
        assert!(ValidationResult::invalid("nope").into_result().is_err());
    }
}
