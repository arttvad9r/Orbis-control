//! Client-side abstraction для вызова привилегированного `orbis-hardwared`
//! (system bus, интерфейс `io.github.orbiscontrol.Hardware1`).
//!
//! - узкий contract: только `SetPerformanceProfile(y) -> y`;
//! - sessiond НЕ получает sysfs/root доступа и НЕ дублирует writer logic;
//! - каждый вызов — новый authoritative D-Bus call (cache отсутствует);
//! - wire decode/confirmed-проверка выполняются вызывающим (SessionService).

use async_trait::async_trait;
use orbis_hardwared::{DBUS_OBJECT_PATH, Hardware1Proxy};
use orbis_providers::error::ProviderError;
use zbus::proxy::CacheProperties;

/// Узкий hardware Performance client contract (инъектируемый для тестов).
#[async_trait]
pub trait HardwarePerformanceClient: Send + Sync {
    /// Вызвать `SetPerformanceProfile(wire)` на system bus; вернуть
    /// подтверждённый hardware wire profile или честную ошибку.
    async fn set_performance_profile(&self, profile: u8) -> Result<u8, ProviderError>;
}

/// Реальная zbus-реализация над готовой system-bus Connection.
///
/// I/O начинается только в `set_performance_profile().await`; конструктор не
/// выполняет D-Bus вызовов и не создаёт proxy.
pub struct ZbusHardwarePerformanceClient {
    connection: zbus::Connection,
}

impl ZbusHardwarePerformanceClient {
    /// Создать client над готовой system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl HardwarePerformanceClient for ZbusHardwarePerformanceClient {
    async fn set_performance_profile(&self, profile: u8) -> Result<u8, ProviderError> {
        let proxy = Hardware1Proxy::builder(&self.connection)
            .path(DBUS_OBJECT_PATH)
            .expect("valid object path")
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy
            .set_performance_profile(profile)
            .await
            .map_err(zbus_error_to_provider)
    }
}

/// Преобразовать `zbus::Error` в `ProviderError` (детерминированный mapping,
/// как в session-client).
fn zbus_error_to_provider(error: zbus::Error) -> ProviderError {
    if let zbus::Error::FDO(boxed) = &error {
        return match &**boxed {
            zbus::fdo::Error::NotSupported(msg) => ProviderError::Unsupported(msg.clone()),
            zbus::fdo::Error::AccessDenied(msg) => ProviderError::PermissionDenied(msg.clone()),
            zbus::fdo::Error::InvalidArgs(msg) => ProviderError::InvalidRequest(msg.clone()),
            _ => ProviderError::Dbus(error.to_string()),
        };
    }
    ProviderError::Dbus(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Fake client для тестов: заранее заданный результат/счётчик вызовов.
    #[allow(dead_code)]
    pub struct ScriptedHardwareClient {
        result: Mutex<Result<u8, ProviderError>>,
        calls: AtomicUsize,
    }

    #[allow(dead_code)]
    impl ScriptedHardwareClient {
        pub fn new(result: Result<u8, ProviderError>) -> Self {
            Self {
                result: Mutex::new(result),
                calls: AtomicUsize::new(0),
            }
        }

        pub fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl HardwarePerformanceClient for ScriptedHardwareClient {
        async fn set_performance_profile(&self, _profile: u8) -> Result<u8, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &*self.result.lock().unwrap() {
                Ok(v) => Ok(*v),
                Err(e) => Err(clone_provider_error(e)),
            }
        }
    }

    /// Клонировать `ProviderError` (нет `Clone`): воссоздаёт эквивалент по классу.
    #[allow(dead_code)]
    fn clone_provider_error(e: &ProviderError) -> ProviderError {
        match e {
            ProviderError::BackendUnavailable(m) => ProviderError::BackendUnavailable(m.clone()),
            ProviderError::Unsupported(m) => ProviderError::Unsupported(m.clone()),
            ProviderError::PermissionDenied(m) => ProviderError::PermissionDenied(m.clone()),
            ProviderError::InvalidRequest(m) => ProviderError::InvalidRequest(m.clone()),
            ProviderError::Timeout(m) => ProviderError::Timeout(m.clone()),
            ProviderError::Io(_) => ProviderError::Io(std::io::Error::other("simulated io")),
            ProviderError::Dbus(m) => ProviderError::Dbus(m.clone()),
            ProviderError::Internal(m) => ProviderError::Internal(m.clone()),
        }
    }
}
