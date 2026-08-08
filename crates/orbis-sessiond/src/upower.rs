//! Read-only UPower adapter для Battery Charge Limit.
//!
//! Реализует `BatteryProvider::charge_limit()` через read-only D-Bus property
//! чтение интерфейса `org.freedesktop.UPower.Device`.
//!
//! - `Connection` и object path батареи передаются извне; adapter сам не
//!   открывает system/session bus и не создаёт service;
//! - mutation-методы `BatteryProvider` возвращают `ProviderError::Unsupported`
//!   и не выполняют I/O;
//! - каждый вызов `charge_limit()` выполняет новое authoritative чтение source
//!   (кэш отсутствует).

use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::Percent;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
use zbus::zvariant::OwnedObjectPath;

/// Минимальный read-only proxy-контракт UPower Device.
///
/// Только три getter property; mutation/signals/enumeration отсутствуют.
#[zbus::proxy(
    interface = "org.freedesktop.UPower.Device",
    default_service = "org.freedesktop.UPower"
)]
trait UPowerDevice {
    /// Конечный порог заряда в процентах (`u32` из D-Bus).
    #[zbus(property)]
    fn charge_end_threshold(&self) -> zbus::Result<u32>;

    /// Поддерживает ли устройство порог заряда.
    #[zbus(property)]
    fn charge_threshold_supported(&self) -> zbus::Result<bool>;

    /// Активна ли функция порога заряда.
    #[zbus(property)]
    fn charge_threshold_enabled(&self) -> zbus::Result<bool>;
}

/// Raw snapshot UPower, независимый от domain.
///
/// Domain validation здесь не выполняется.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UPowerChargeLimitSnapshot {
    /// Поддерживает ли backend порог заряда.
    pub supported: bool,
    /// Активна ли функция порога.
    pub enabled: bool,
    /// Конечный порог в процентах (raw `u32`).
    pub end_threshold: u32,
}

/// Testable источник UPower charge limit snapshot.
#[async_trait]
pub trait UPowerChargeLimitSource: Send + Sync {
    /// Прочитать snapshot (authoritative, без кэша).
    async fn read_charge_limit(&self) -> Result<UPowerChargeLimitSnapshot, ProviderError>;
}

/// Реальный zbus источник.
///
/// Хранит переданную извне `Connection` и object path батареи; I/O начинается
/// только в `read_charge_limit().await`.
pub struct ZbusUPowerChargeLimitSource {
    connection: zbus::Connection,
    object_path: OwnedObjectPath,
}

impl ZbusUPowerChargeLimitSource {
    /// Создать источник с готовой Connection и object path батареи.
    ///
    /// Конструктор не выполняет I/O, не открывает bus и не проверяет
    /// существование объекта.
    pub fn new(connection: zbus::Connection, object_path: OwnedObjectPath) -> Self {
        Self {
            connection,
            object_path,
        }
    }
}

#[async_trait]
impl UPowerChargeLimitSource for ZbusUPowerChargeLimitSource {
    async fn read_charge_limit(&self) -> Result<UPowerChargeLimitSnapshot, ProviderError> {
        // Явно отключаем property cache: каждый getter выполняет прямой
        // отдельный D-Bus Get (без GetAll), строго последовательно, с
        // коротким замыканием после первой ошибки.
        let proxy = UPowerDeviceProxy::builder(&self.connection)
            .path(self.object_path.clone())
            .map_err(|e| ProviderError::Dbus(e.to_string()))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        let supported = proxy
            .charge_threshold_supported()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        let enabled = proxy
            .charge_threshold_enabled()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        let end_threshold = proxy
            .charge_end_threshold()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        Ok(UPowerChargeLimitSnapshot {
            supported,
            enabled,
            end_threshold,
        })
    }
}

/// Read-only Battery Charge Limit provider над UPower.
///
/// `S` — source (реальный zbus или scripted в тестах); источник передаётся в
/// конструкторе, который не выполняет I/O.
pub struct UPowerChargeLimitProvider<S> {
    source: S,
}

impl<S> UPowerChargeLimitProvider<S> {
    /// Создать provider над source.
    ///
    /// Не открывает D-Bus connection и не выполняет I/O; UPower не сообщает
    /// hardware min/max/step, поэтому provider не принимает injected bounds.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for UPowerChargeLimitProvider<S>
where
    S: UPowerChargeLimitSource,
{
    fn id(&self) -> &'static str {
        "upower-charge-limit"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("upower-charge-limit")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("upower read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.upower-charge-limit",
            "read-only UPower charge limit backend (Этап: без hardware writes)",
        )]
    }
}

#[async_trait]
impl<S> BatteryProvider for UPowerChargeLimitProvider<S>
where
    S: UPowerChargeLimitSource,
{
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        let snapshot = self.source.read_charge_limit().await?;
        if !snapshot.supported {
            return Err(ProviderError::Unsupported(
                "UPower: charge threshold не поддерживается устройством".into(),
            ));
        }

        let percent_u8 = u8::try_from(snapshot.end_threshold).map_err(|_| {
            ProviderError::Internal(format!(
                "UPower: end_threshold вне u8 диапазона: {}",
                snapshot.end_threshold
            ))
        })?;

        ChargeLimit::new(
            snapshot.enabled,
            Some(Percent::new(percent_u8).map_err(|e| {
                ProviderError::Internal(format!("UPower: невалидный threshold: {e}"))
            })?),
            None,
        )
        .map_err(|e| ProviderError::Internal(format!("UPower: threshold невалиден: {e}")))
    }

    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "upower read-only backend: set_charge_limit недоступна".into(),
        ))
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "upower read-only backend: one_shot_full_charge недоступна".into(),
        ))
    }

    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid("read-only backend: запись charge limit не поддерживается")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Исход теста ScriptedSource: snapshot либо сценарий ошибки.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ScriptedOutcome {
        Snapshot(UPowerChargeLimitSnapshot),
        Dbus,
    }

    /// Тестовый источник: возвращает заранее заданный исход, считает вызовы.
    struct ScriptedSource {
        outcome: ScriptedOutcome,
        reads: AtomicUsize,
    }

    impl ScriptedSource {
        fn new(outcome: ScriptedOutcome) -> Self {
            Self {
                outcome,
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl UPowerChargeLimitSource for ScriptedSource {
        async fn read_charge_limit(&self) -> Result<UPowerChargeLimitSnapshot, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            match self.outcome {
                ScriptedOutcome::Snapshot(s) => Ok(s),
                ScriptedOutcome::Dbus => Err(ProviderError::Dbus("scripted dbus error".into())),
            }
        }
    }

    fn provider(outcome: ScriptedOutcome) -> UPowerChargeLimitProvider<ScriptedSource> {
        UPowerChargeLimitProvider::new(ScriptedSource::new(outcome))
    }

    #[tokio::test]
    async fn maps_supported_enabled_charge_limit() {
        let p = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: true,
            enabled: true,
            end_threshold: 80,
        }));
        let limit = p.charge_limit().await.expect("charge limit");
        assert!(limit.enabled);
        assert_eq!(limit.percent.map(|x| x.get()), Some(80));
        // UPower не сообщает hardware min/max/step: bounds неизвестны.
        assert!(limit.bounds.is_none());
        assert_eq!(p.source.reads(), 1);
    }

    #[tokio::test]
    async fn preserves_disabled_state_with_known_threshold() {
        let p = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: true,
            enabled: false,
            end_threshold: 80,
        }));
        let limit = p.charge_limit().await.expect("charge limit");
        assert!(!limit.enabled);
        // Выключенная функция не отменяет известный порог.
        assert_eq!(limit.percent.map(|x| x.get()), Some(80));
        // UPower не сообщает hardware min/max/step: bounds неизвестны.
        assert!(limit.bounds.is_none());
    }

    #[tokio::test]
    async fn unsupported_threshold_is_reported() {
        let p = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: false,
            enabled: false,
            end_threshold: 0,
        }));
        let err = p.charge_limit().await.expect_err("unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));
    }

    #[tokio::test]
    async fn invalid_backend_threshold_is_rejected() {
        // >255: u8::try_from отбрасывает, усечения нет.
        let p_high = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: true,
            enabled: true,
            end_threshold: 300,
        }));
        assert!(matches!(
            p_high.charge_limit().await.expect_err("300 rejected"),
            ProviderError::Internal(_)
        ));

        // 0: при неизвестных bounds (None) допустимый current percent.
        let p_zero = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: true,
            enabled: true,
            end_threshold: 0,
        }));
        let limit = p_zero.charge_limit().await.expect("0 accepted");
        assert_eq!(limit.percent.map(|x| x.get()), Some(0));
        assert!(limit.bounds.is_none());
    }

    #[tokio::test]
    async fn source_error_is_preserved() {
        let p = provider(ScriptedOutcome::Dbus);
        let err = p.charge_limit().await.expect_err("dbus error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn mutations_are_unsupported_without_source_access() {
        let p = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: true,
            enabled: true,
            end_threshold: 80,
        }));
        assert!(matches!(
            p.set_charge_limit(40).await.expect_err("set unsupported"),
            ProviderError::Unsupported(_)
        ));
        assert!(matches!(
            p.one_shot_full_charge()
                .await
                .expect_err("oneshot unsupported"),
            ProviderError::Unsupported(_)
        ));
        // I/O не выполнялся.
        assert_eq!(p.source.reads(), 0);
    }

    #[test]
    fn validation_does_not_claim_write_support() {
        let p = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: true,
            enabled: true,
            end_threshold: 80,
        }));
        assert!(!matches!(
            p.validate_charge_limit(80),
            ValidationResult::Valid
        ));
        assert_eq!(p.source.reads(), 0);
    }
}
