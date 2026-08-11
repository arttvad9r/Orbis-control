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

use std::path::PathBuf;
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
    /// Effective kernel end threshold, если отдельный source смог его прочитать.
    pub effective_end_threshold: Option<u8>,
}

/// Testable источник UPower charge limit snapshot.
#[async_trait]
pub trait UPowerChargeLimitSource: Send + Sync {
    /// Прочитать snapshot (authoritative, без кэша).
    async fn read_charge_limit(&self) -> Result<UPowerChargeLimitSnapshot, ProviderError>;
}

/// Узкий read-only source effective kernel battery threshold.
#[async_trait]
pub trait BatteryEffectiveSource: Send + Sync {
    /// Прочитать effective end threshold свежим чтением.
    async fn read_effective_end_threshold(&self) -> Result<u8, ProviderError>;
}

/// Read-only sysfs source для уже обнаруженного power-supply.
pub struct SysfsBatteryEndThresholdSource {
    path: PathBuf,
}

impl SysfsBatteryEndThresholdSource {
    /// Создать source из одного validated native power-supply name.
    pub fn from_native_path(native_path: &str) -> Result<Self, ProviderError> {
        let name = std::path::Path::new(native_path);
        if native_path.is_empty()
            || name.components().count() != 1
            || name.file_name().and_then(|v| v.to_str()) != Some(native_path)
        {
            return Err(ProviderError::InvalidRequest(format!(
                "invalid power-supply native path: {native_path}"
            )));
        }
        Ok(Self {
            path: PathBuf::from("/sys/class/power_supply")
                .join(native_path)
                .join("charge_control_end_threshold"),
        })
    }

    #[cfg(test)]
    fn new_for_test(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl BatteryEffectiveSource for SysfsBatteryEndThresholdSource {
    async fn read_effective_end_threshold(&self) -> Result<u8, ProviderError> {
        let raw = std::fs::read_to_string(&self.path).map_err(ProviderError::Io)?;
        let value = raw.trim().parse::<u16>().map_err(|e| {
            ProviderError::Internal(format!(
                "battery effective threshold is not an integer: {e}"
            ))
        })?;
        if value > 100 {
            Err(ProviderError::Internal(format!(
                "battery effective threshold outside percent range: {value}"
            )))
        } else {
            Ok(value as u8)
        }
    }
}

/// Объединяет UPower configured/enabled reads и kernel effective read.
pub struct CombinedChargeLimitSource<U, E> {
    upower: U,
    effective: E,
}

impl<U, E> CombinedChargeLimitSource<U, E> {
    /// Создать composite read-only source.
    pub fn new(upower: U, effective: E) -> Self {
        Self { upower, effective }
    }
}

#[async_trait]
impl<U, E> UPowerChargeLimitSource for CombinedChargeLimitSource<U, E>
where
    U: UPowerChargeLimitSource,
    E: BatteryEffectiveSource,
{
    async fn read_charge_limit(&self) -> Result<UPowerChargeLimitSnapshot, ProviderError> {
        let mut snapshot = self.upower.read_charge_limit().await?;
        snapshot.effective_end_threshold =
            Some(self.effective.read_effective_end_threshold().await?);
        Ok(snapshot)
    }
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
            effective_end_threshold: None,
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

        let configured = Some(Percent::new(percent_u8).map_err(|e| {
            ProviderError::Internal(format!("UPower: невалидный configured threshold: {e}"))
        })?);
        let effective = snapshot
            .effective_end_threshold
            .map(|value| {
                Percent::new(value).map_err(|e| {
                    ProviderError::Internal(format!("kernel: невалидный effective threshold: {e}"))
                })
            })
            .transpose()?;
        ChargeLimit::new(snapshot.enabled, configured, effective, None)
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
    use tempfile::tempdir;

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
            effective_end_threshold: None,
        }));
        let limit = p.charge_limit().await.expect("charge limit");
        assert!(limit.enabled);
        assert_eq!(limit.configured_percent.map(|x| x.get()), Some(80));
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
            effective_end_threshold: None,
        }));
        let limit = p.charge_limit().await.expect("charge limit");
        assert!(!limit.enabled);
        // Выключенная функция не отменяет известный порог.
        assert_eq!(limit.configured_percent.map(|x| x.get()), Some(80));
        // UPower не сообщает hardware min/max/step: bounds неизвестны.
        assert!(limit.bounds.is_none());
    }

    #[tokio::test]
    async fn unsupported_threshold_is_reported() {
        let p = provider(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
            supported: false,
            enabled: false,
            end_threshold: 0,
            effective_end_threshold: None,
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
            effective_end_threshold: None,
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
            effective_end_threshold: None,
        }));
        let limit = p_zero.charge_limit().await.expect("0 accepted");
        assert_eq!(limit.configured_percent.map(|x| x.get()), Some(0));
        assert!(limit.bounds.is_none());
    }

    #[tokio::test]
    async fn sysfs_effective_source_reads_fresh_and_rejects_malformed() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("charge_control_end_threshold");
        std::fs::write(&path, "100\n").expect("write fixture");
        let source = SysfsBatteryEndThresholdSource::new_for_test(path.clone());
        assert_eq!(source.read_effective_end_threshold().await.unwrap(), 100);

        std::fs::write(&path, "not-a-number\n").expect("write malformed fixture");
        assert!(matches!(
            source.read_effective_end_threshold().await,
            Err(ProviderError::Internal(_))
        ));
        std::fs::write(&path, "101\n").expect("write out-of-range fixture");
        assert!(matches!(
            source.read_effective_end_threshold().await,
            Err(ProviderError::Internal(_))
        ));
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
            effective_end_threshold: None,
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
            effective_end_threshold: None,
        }));
        assert!(!matches!(
            p.validate_charge_limit(80),
            ValidationResult::Valid
        ));
        assert_eq!(p.source.reads(), 0);
    }
}
