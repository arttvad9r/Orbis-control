//! Ошибки и вспомогательные типы провайдеров.

use thiserror::Error;

use orbis_capabilities::probe::{
    ProbeClassification, ProbeContext, ProbeError, ProbeOperationResult, classify_dbus_detail,
};

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

impl ProviderError {
    /// Classify a provider error for a future capability probe.
    ///
    /// This adapter deliberately lives in `orbis-providers`: the provider crate
    /// already depends on the capability contract, while `orbis-capabilities`
    /// remains independent from provider implementations.
    pub fn into_probe_result(
        &self,
        context: ProbeContext,
    ) -> Result<ProbeOperationResult, ProbeError> {
        let result = match self {
            Self::BackendUnavailable(detail) => ProbeOperationResult::with_detail(
                match context {
                    ProbeContext::BackendDiscovery => ProbeClassification::BackendMissing,
                    ProbeContext::EstablishedBackend => ProbeClassification::TemporarilyUnavailable,
                },
                detail.clone(),
            ),
            Self::Unsupported(detail) => {
                ProbeOperationResult::with_detail(ProbeClassification::Unsupported, detail.clone())
            }
            Self::PermissionDenied(detail) => ProbeOperationResult::with_detail(
                ProbeClassification::PermissionDenied,
                detail.clone(),
            ),
            Self::Timeout(detail) => ProbeOperationResult::with_detail(
                ProbeClassification::TemporarilyUnavailable,
                detail.clone(),
            ),
            Self::Dbus(detail) => ProbeOperationResult::with_detail(
                classify_dbus_detail(detail, context),
                detail.clone(),
            ),
            Self::Io(error) => {
                ProbeOperationResult::with_detail(ProbeClassification::Unknown, error.to_string())
            }
            Self::InvalidRequest(detail) => {
                return Err(ProbeError::ContractViolation(detail.clone()));
            }
            Self::Internal(detail) => return Err(ProbeError::Internal(detail.clone())),
        };
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::probe::{ProbeClassification, ProbeContext};

    #[test]
    fn operation_id_unique() {
        assert_ne!(OperationId::new(), OperationId::new());
    }

    #[test]
    fn validation_helpers() {
        assert!(ValidationResult::ok().into_result().is_ok());
        assert!(ValidationResult::invalid("nope").into_result().is_err());
    }

    #[test]
    fn provider_errors_have_normative_probe_classification() {
        assert_eq!(
            ProviderError::Unsupported("not implemented".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::Unsupported
        );
        assert_eq!(
            ProviderError::PermissionDenied("polkit".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::PermissionDenied
        );
        assert_eq!(
            ProviderError::BackendUnavailable("service absent".into())
                .into_probe_result(ProbeContext::BackendDiscovery)
                .unwrap()
                .classification,
            ProbeClassification::BackendMissing
        );
        assert_eq!(
            ProviderError::BackendUnavailable("backend restarted".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::TemporarilyUnavailable
        );
        assert_eq!(
            ProviderError::Timeout("no reply".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::TemporarilyUnavailable
        );
    }

    #[test]
    fn provider_dbus_error_mapping_preserves_distinctions() {
        assert_eq!(
            ProviderError::Dbus("org.freedesktop.DBus.Error.ServiceUnknown".into())
                .into_probe_result(ProbeContext::BackendDiscovery)
                .unwrap()
                .classification,
            ProbeClassification::BackendMissing
        );
        assert_eq!(
            ProviderError::Dbus("org.freedesktop.DBus.Error.AccessDenied".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::PermissionDenied
        );
        assert_eq!(
            ProviderError::Dbus("org.freedesktop.DBus.Error.UnknownMethod".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::Unsupported
        );
        assert_eq!(
            ProviderError::Dbus("unexpected D-Bus failure".into())
                .into_probe_result(ProbeContext::EstablishedBackend)
                .unwrap()
                .classification,
            ProbeClassification::Unknown
        );
    }

    #[test]
    fn provider_internal_and_invalid_request_are_probe_errors() {
        assert!(matches!(
            ProviderError::Internal("invariant".into())
                .into_probe_result(ProbeContext::EstablishedBackend),
            Err(ProbeError::Internal(_))
        ));
        assert!(matches!(
            ProviderError::InvalidRequest("bad request".into())
                .into_probe_result(ProbeContext::EstablishedBackend),
            Err(ProbeError::ContractViolation(_))
        ));
    }
}
