//! # orbis-session-client
//!
//! Client-side read-only Battery Charge Limit provider поверх D-Bus protocol
//! `orbis-session-protocol` (generated `Session1Proxy`).
//!
//! - source получает готовую `zbus::Connection` извне; crate не открывает
//!   session/system bus, не создаёт runtime и не выполняет hardware access;
//! - mutation-методы `BatteryProvider` возвращают `ProviderError::Unsupported`;
//! - каждый вызов `charge_limit()` выполняет новый authoritative property Get
//!   (кэш отсутствует).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::{ChargeLimit, ChargeLimitBounds};
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::Percent;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, Provider, ProviderHealth,
};
use orbis_session_protocol::{ChargeLimitInfo, Session1Proxy, gpu_access, gpu_mux, gpu_power};
use zbus::proxy::CacheProperties;

/// Testable источник wire DTO через session protocol.
#[async_trait]
pub trait SessionChargeLimitSource: Send + Sync {
    /// Прочитать authoritative `ChargeLimitInfo` (wire DTO, без domain
    /// conversion); ошибка не превращается в bool/None.
    async fn read_charge_limit(&self) -> Result<ChargeLimitInfo, ProviderError>;
}

/// Реальный zbus источник через generated `Session1Proxy`.
///
/// Хранит переданную извне готовую `Connection`; I/O начинается только в
/// `read_charge_limit().await`.
pub struct ZbusSessionChargeLimitSource {
    connection: zbus::Connection,
}

impl ZbusSessionChargeLimitSource {
    /// Создать источник с готовой Connection.
    ///
    /// Конструктор не выполняет I/O, не открывает session/system bus, не
    /// проверяет service, не создаёт proxy и runtime; cache отсутствует.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl SessionChargeLimitSource for ZbusSessionChargeLimitSource {
    async fn read_charge_limit(&self) -> Result<ChargeLimitInfo, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        let info = proxy.charge_limit().await.map_err(zbus_error_to_provider)?;
        Ok(info)
    }
}

/// Преобразовать `zbus::Error` в `ProviderError`.
///
/// Remote FDO errors отображаются детерминированно; остальные (Failed,
/// transport/authentication/disconnect/protocol) — в `ProviderError::Dbus`.
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

/// Wire→domain boundary между недоверенным D-Bus payload и доменной моделью.
///
/// - canonical invariant: `percent_present == false` и `percent != 0` — ошибка
///   `Internal` (несогласованный payload не превращается молча в `None`);
/// - malformed bounds/percent/step → `Internal` (remote service нарушил
///   contract — это не ошибка пользовательского запроса);
/// - `enabled` сохраняется без преобразования.
pub fn charge_limit_from_wire(info: ChargeLimitInfo) -> Result<ChargeLimit, ProviderError> {
    if !info.percent_present && info.percent != 0 {
        return Err(ProviderError::Internal(format!(
            "session protocol: percent_present=false, но percent={} (нарушен canonical invariant)",
            info.percent
        )));
    }

    if !info.bounds_present
        && (info.min_percent != 0 || info.max_percent != 0 || info.step_percent != 0)
    {
        return Err(ProviderError::Internal(
            "session protocol: bounds_present=false, но min/max/step ненулевые (нарушен canonical invariant)"
                .to_string(),
        ));
    }

    let percent = if info.percent_present {
        Some(Percent::new(info.percent).map_err(|e| {
            ProviderError::Internal(format!("session protocol: невалидный percent: {e}"))
        })?)
    } else {
        None
    };

    let bounds = if info.bounds_present {
        Some(
            ChargeLimitBounds::new(
                Percent::new(info.min_percent).map_err(|e| {
                    ProviderError::Internal(format!(
                        "session protocol: невалидный min_percent: {e}"
                    ))
                })?,
                Percent::new(info.max_percent).map_err(|e| {
                    ProviderError::Internal(format!(
                        "session protocol: невалидный max_percent: {e}"
                    ))
                })?,
                info.step_percent,
            )
            .map_err(|e| {
                ProviderError::Internal(format!("session protocol: невалидные bounds: {e}"))
            })?,
        )
    } else {
        None
    };

    ChargeLimit::new(info.enabled, percent, bounds).map_err(|e| {
        ProviderError::Internal(format!(
            "session protocol: несогласованный wire payload: {e}"
        ))
    })
}

/// Read-only Battery Charge Limit provider над session protocol source.
///
/// `S` — source (реальный zbus или scripted в тестах); source передаётся в
/// конструкторе, который не выполняет I/O. Bounds приходят по protocol (не
/// хранятся отдельно); cache/last state отсутствуют.
pub struct SessionChargeLimitProvider<S> {
    source: S,
}

impl<S> SessionChargeLimitProvider<S> {
    /// Создать provider над source.
    ///
    /// Не выполняет D-Bus чтение и не открывает Connection.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SessionChargeLimitProvider<S>
where
    S: SessionChargeLimitSource,
{
    fn id(&self) -> &'static str {
        "session-charge-limit"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-charge-limit")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session protocol read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-charge-limit",
            "read-only session protocol charge limit backend",
        )]
    }
}

#[async_trait]
impl<S> BatteryProvider for SessionChargeLimitProvider<S>
where
    S: SessionChargeLimitSource,
{
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        let info = self.source.read_charge_limit().await?;
        charge_limit_from_wire(info)
    }

    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session protocol read-only: set_charge_limit недоступна".into(),
        ))
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session protocol read-only: one_shot_full_charge недоступна".into(),
        ))
    }

    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid(
            "session protocol read-only: запись charge limit не поддерживается",
        )
    }
}

// ---------------------------------------------------------------------------
// Read-only GPU capabilities (power / MUX / access) через Session1
// ---------------------------------------------------------------------------

/// Преобразовать wire `u8` в domain `GpuPowerState`.
///
/// Strict decode: неизвестный wire value → `Internal` (remote service нарушил
/// contract). Semantic `Unknown` передаётся как допустимое wire значение.
pub fn gpu_power_from_wire(raw: u8) -> Result<GpuPowerState, ProviderError> {
    match raw {
        gpu_power::ACTIVE => Ok(GpuPowerState::Active),
        gpu_power::SUSPENDED => Ok(GpuPowerState::Suspended),
        gpu_power::OFF => Ok(GpuPowerState::Off),
        gpu_power::STALE => Ok(GpuPowerState::Stale),
        gpu_power::UNKNOWN => Ok(GpuPowerState::Unknown),
        other => Err(ProviderError::Internal(format!(
            "session protocol: неизвестный gpu_power wire value {other}"
        ))),
    }
}

/// Преобразовать wire `u8` в domain `GpuMuxState`.
pub fn gpu_mux_from_wire(raw: u8) -> Result<GpuMuxState, ProviderError> {
    match raw {
        gpu_mux::INTEGRATED => Ok(GpuMuxState::Integrated),
        gpu_mux::DISCRETE => Ok(GpuMuxState::Discrete),
        gpu_mux::UNKNOWN => Ok(GpuMuxState::Unknown),
        other => Err(ProviderError::Internal(format!(
            "session protocol: неизвестный gpu_mux wire value {other}"
        ))),
    }
}

/// Преобразовать wire `u8` в domain `GpuAccessPolicy`.
pub fn gpu_access_from_wire(raw: u8) -> Result<GpuAccessPolicy, ProviderError> {
    match raw {
        gpu_access::UNBLOCKED => Ok(GpuAccessPolicy::Unblocked),
        gpu_access::BLOCKED => Ok(GpuAccessPolicy::Blocked),
        gpu_access::PENDING => Ok(GpuAccessPolicy::Pending),
        gpu_access::UNKNOWN => Ok(GpuAccessPolicy::Unknown),
        other => Err(ProviderError::Internal(format!(
            "session protocol: неизвестный gpu_access wire value {other}"
        ))),
    }
}

/// Testable источник raw GPU power wire value через session protocol.
#[async_trait]
pub trait SessionGpuPowerSource: Send + Sync {
    /// Прочитать authoritative wire `gpu_power` (u8).
    async fn read_gpu_power(&self) -> Result<u8, ProviderError>;
}

/// Testable источник raw GPU MUX wire value через session protocol.
#[async_trait]
pub trait SessionGpuMuxSource: Send + Sync {
    /// Прочитать authoritative wire `gpu_mux` (u8).
    async fn read_gpu_mux(&self) -> Result<u8, ProviderError>;
}

/// Testable источник raw GPU access wire value через session protocol.
#[async_trait]
pub trait SessionGpuAccessSource: Send + Sync {
    /// Прочитать authoritative wire `gpu_access` (u8).
    async fn read_gpu_access(&self) -> Result<u8, ProviderError>;
}

/// Реальный zbus источник для всех трёх GPU capability wire reads.
pub struct ZbusSessionGpuSource {
    connection: zbus::Connection,
}

impl ZbusSessionGpuSource {
    /// Создать источник с готовой Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl SessionGpuPowerSource for ZbusSessionGpuSource {
    async fn read_gpu_power(&self) -> Result<u8, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy.gpu_power().await.map_err(zbus_error_to_provider)
    }
}

#[async_trait]
impl SessionGpuMuxSource for ZbusSessionGpuSource {
    async fn read_gpu_mux(&self) -> Result<u8, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy.gpu_mux().await.map_err(zbus_error_to_provider)
    }
}

#[async_trait]
impl SessionGpuAccessSource for ZbusSessionGpuSource {
    async fn read_gpu_access(&self) -> Result<u8, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy.gpu_access().await.map_err(zbus_error_to_provider)
    }
}

/// Read-only GPU power provider над session protocol source.
pub struct SessionGpuPowerProvider<S> {
    source: S,
}

impl<S> SessionGpuPowerProvider<S> {
    /// Создать provider над source.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SessionGpuPowerProvider<S>
where
    S: SessionGpuPowerSource,
{
    fn id(&self) -> &'static str {
        "session-gpu-power"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-gpu-power")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session protocol read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-gpu-power",
            "read-only session protocol GPU power backend",
        )]
    }
}

#[async_trait]
impl<S> GpuPowerProvider for SessionGpuPowerProvider<S>
where
    S: SessionGpuPowerSource,
{
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        let raw = self.source.read_gpu_power().await?;
        gpu_power_from_wire(raw)
    }
}

/// Read-only GPU MUX provider над session protocol source.
pub struct SessionGpuMuxProvider<S> {
    source: S,
}

impl<S> SessionGpuMuxProvider<S> {
    /// Создать provider над source.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SessionGpuMuxProvider<S>
where
    S: SessionGpuMuxSource,
{
    fn id(&self) -> &'static str {
        "session-gpu-mux"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-gpu-mux")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session protocol read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-gpu-mux",
            "read-only session protocol GPU MUX backend",
        )]
    }
}

#[async_trait]
impl<S> GpuMuxProvider for SessionGpuMuxProvider<S>
where
    S: SessionGpuMuxSource,
{
    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
        let raw = self.source.read_gpu_mux().await?;
        gpu_mux_from_wire(raw)
    }
}

/// Read-only GPU access provider над session protocol source.
pub struct SessionGpuAccessProvider<S> {
    source: S,
}

impl<S> SessionGpuAccessProvider<S> {
    /// Создать provider над source.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SessionGpuAccessProvider<S>
where
    S: SessionGpuAccessSource,
{
    fn id(&self) -> &'static str {
        "session-gpu-access"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-gpu-access")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session protocol read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-gpu-access",
            "read-only session protocol GPU access backend",
        )]
    }
}

#[async_trait]
impl<S> GpuAccessProvider for SessionGpuAccessProvider<S>
where
    S: SessionGpuAccessSource,
{
    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
        let raw = self.source.read_gpu_access().await?;
        gpu_access_from_wire(raw)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Исход теста ScriptedSource.
    #[derive(Debug, Clone, Copy)]
    enum ScriptedRead {
        Info(ChargeLimitInfo),
        Dbus,
    }

    /// Тестовый источник: очередь заранее заданных wire DTO/ошибок, счётчик.
    struct ScriptedSource {
        results: Mutex<VecDeque<ScriptedRead>>,
        reads: AtomicUsize,
    }

    impl ScriptedSource {
        fn new(reads: Vec<ScriptedRead>) -> Self {
            Self {
                results: Mutex::new(reads.into()),
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SessionChargeLimitSource for ScriptedSource {
        async fn read_charge_limit(&self) -> Result<ChargeLimitInfo, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            match self
                .results
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted source: очередь результатов исчерпана")
            {
                ScriptedRead::Info(info) => Ok(info),
                ScriptedRead::Dbus => Err(ProviderError::Dbus("scripted dbus error".into())),
            }
        }
    }

    #[test]
    fn maps_wire_with_percent_to_domain() {
        let info = ChargeLimitInfo::with_percent(true, 80, 40, 100, 5);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(limit.enabled);
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
        let b = limit.bounds.expect("known bounds");
        assert_eq!(b.min.get(), 40);
        assert_eq!(b.max.get(), 100);
        assert_eq!(b.step, 5);
    }

    #[test]
    fn maps_wire_without_percent_to_domain() {
        let info = ChargeLimitInfo::without_percent(false, 40, 100, 5);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(!limit.enabled);
        assert_eq!(limit.percent, None);
        let b = limit.bounds.expect("known bounds");
        assert_eq!(b.min.get(), 40);
        assert_eq!(b.max.get(), 100);
        assert_eq!(b.step, 5);
    }

    #[test]
    fn preserves_disabled_state_with_known_percent() {
        let info = ChargeLimitInfo {
            enabled: false,
            percent_present: true,
            percent: 80,
            bounds_present: true,
            min_percent: 40,
            max_percent: 100,
            step_percent: 5,
        };
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(!limit.enabled);
        // Выключенная функция не означает отсутствие известного threshold.
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
    }

    #[test]
    fn rejects_noncanonical_missing_percent() {
        let info = ChargeLimitInfo {
            enabled: true,
            percent_present: false,
            percent: 80,
            bounds_present: true,
            min_percent: 40,
            max_percent: 100,
            step_percent: 5,
        };
        let err = charge_limit_from_wire(info).expect_err("noncanonical");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn rejects_invalid_remote_bounds() {
        // min > max
        let bad_range = ChargeLimitInfo::with_percent(true, 80, 100, 40, 5);
        assert!(matches!(
            charge_limit_from_wire(bad_range).expect_err("min > max"),
            ProviderError::Internal(_)
        ));
        // percent вне [min, max]
        let bad_percent = ChargeLimitInfo::with_percent(true, 30, 40, 100, 5);
        assert!(matches!(
            charge_limit_from_wire(bad_percent).expect_err("percent < min"),
            ProviderError::Internal(_)
        ));
        // step == 0
        let bad_step = ChargeLimitInfo::with_percent(true, 80, 40, 100, 0);
        assert!(matches!(
            charge_limit_from_wire(bad_step).expect_err("step == 0"),
            ProviderError::Internal(_)
        ));
    }

    #[tokio::test]
    async fn provider_reads_source_once() {
        let source = ScriptedSource::new(vec![ScriptedRead::Info(ChargeLimitInfo::with_percent(
            true, 80, 40, 100, 5,
        ))]);
        let provider = SessionChargeLimitProvider::new(source);
        let limit = provider.charge_limit().await.expect("charge limit");
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
        assert_eq!(provider.source.reads(), 1);
    }

    #[tokio::test]
    async fn provider_does_not_cache_wire_state() {
        let source = ScriptedSource::new(vec![
            ScriptedRead::Info(ChargeLimitInfo::with_percent(true, 80, 40, 100, 5)),
            ScriptedRead::Info(ChargeLimitInfo::with_percent(true, 60, 40, 100, 5)),
        ]);
        let provider = SessionChargeLimitProvider::new(source);
        let first = provider.charge_limit().await.expect("read1");
        let second = provider.charge_limit().await.expect("read2");
        assert_eq!(first.percent.map(|p| p.get()), Some(80));
        assert_eq!(second.percent.map(|p| p.get()), Some(60));
        assert_eq!(provider.source.reads(), 2);
    }

    #[tokio::test]
    async fn source_error_is_preserved() {
        let source = ScriptedSource::new(vec![ScriptedRead::Dbus]);
        let provider = SessionChargeLimitProvider::new(source);
        let err = provider.charge_limit().await.expect_err("dbus error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn mutations_are_unsupported_without_source_access() {
        let source = ScriptedSource::new(vec![ScriptedRead::Info(ChargeLimitInfo::with_percent(
            true, 80, 40, 100, 5,
        ))]);
        let provider = SessionChargeLimitProvider::new(source);
        assert!(matches!(
            provider
                .set_charge_limit(40)
                .await
                .expect_err("set unsupported"),
            ProviderError::Unsupported(_)
        ));
        assert!(matches!(
            provider
                .one_shot_full_charge()
                .await
                .expect_err("oneshot unsupported"),
            ProviderError::Unsupported(_)
        ));
        assert_eq!(provider.source.reads(), 0);
    }

    #[test]
    fn validation_does_not_claim_write_support() {
        let source = ScriptedSource::new(vec![ScriptedRead::Info(ChargeLimitInfo::with_percent(
            true, 80, 40, 100, 5,
        ))]);
        let provider = SessionChargeLimitProvider::new(source);
        assert!(!matches!(
            provider.validate_charge_limit(80),
            ValidationResult::Valid
        ));
        assert_eq!(provider.source.reads(), 0);
    }

    #[test]
    fn remote_fdo_errors_map_to_provider_classes() {
        let unsupported = zbus_error_to_provider(zbus::Error::FDO(Box::new(
            zbus::fdo::Error::NotSupported("no".into()),
        )));
        assert!(matches!(unsupported, ProviderError::Unsupported(_)));

        let denied = zbus_error_to_provider(zbus::Error::FDO(Box::new(
            zbus::fdo::Error::AccessDenied("no".into()),
        )));
        assert!(matches!(denied, ProviderError::PermissionDenied(_)));

        let invalid = zbus_error_to_provider(zbus::Error::FDO(Box::new(
            zbus::fdo::Error::InvalidArgs("no".into()),
        )));
        assert!(matches!(invalid, ProviderError::InvalidRequest(_)));

        let failed = zbus_error_to_provider(zbus::Error::FDO(Box::new(zbus::fdo::Error::Failed(
            "boom".into(),
        ))));
        assert!(matches!(failed, ProviderError::Dbus(_)));
    }

    #[test]
    fn generic_zbus_error_maps_to_dbus() {
        let err = zbus_error_to_provider(zbus::Error::Failure("transport".into()));
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[test]
    fn maps_wire_unknown_bounds_to_domain() {
        // current percent допустим при bounds=None.
        let info = ChargeLimitInfo::with_percent_unknown_bounds(true, 80);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(limit.enabled);
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
        assert!(limit.bounds.is_none());
    }

    #[test]
    fn rejects_noncanonical_absent_bounds() {
        // bounds_present=false с ненулевыми min/max/step нарушает canonical form.
        let info = ChargeLimitInfo {
            enabled: true,
            percent_present: true,
            percent: 80,
            bounds_present: false,
            min_percent: 40,
            max_percent: 0,
            step_percent: 0,
        };
        let err = charge_limit_from_wire(info).expect_err("noncanonical absent bounds");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn unknown_bounds_roundtrip_preserves_none() {
        let info = ChargeLimitInfo::with_percent_unknown_bounds(true, 60);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert_eq!(limit.percent.map(|p| p.get()), Some(60));
        assert!(limit.bounds.is_none());
    }

    // -----------------------------------------------------------------------
    // GPU capability wire decode
    // -----------------------------------------------------------------------

    #[test]
    fn gpu_power_wire_decode_maps_all_values() {
        assert_eq!(
            gpu_power_from_wire(gpu_power::ACTIVE).unwrap(),
            GpuPowerState::Active
        );
        assert_eq!(
            gpu_power_from_wire(gpu_power::SUSPENDED).unwrap(),
            GpuPowerState::Suspended
        );
        assert_eq!(
            gpu_power_from_wire(gpu_power::OFF).unwrap(),
            GpuPowerState::Off
        );
        assert_eq!(
            gpu_power_from_wire(gpu_power::STALE).unwrap(),
            GpuPowerState::Stale
        );
        assert_eq!(
            gpu_power_from_wire(gpu_power::UNKNOWN).unwrap(),
            GpuPowerState::Unknown
        );
    }

    #[test]
    fn gpu_mux_wire_decode_maps_all_values() {
        use orbis_session_protocol::gpu_mux;
        assert_eq!(
            gpu_mux_from_wire(gpu_mux::INTEGRATED).unwrap(),
            GpuMuxState::Integrated
        );
        assert_eq!(
            gpu_mux_from_wire(gpu_mux::DISCRETE).unwrap(),
            GpuMuxState::Discrete
        );
        assert_eq!(
            gpu_mux_from_wire(gpu_mux::UNKNOWN).unwrap(),
            GpuMuxState::Unknown
        );
    }

    #[test]
    fn gpu_access_wire_decode_maps_all_values() {
        use orbis_session_protocol::gpu_access;
        assert_eq!(
            gpu_access_from_wire(gpu_access::UNBLOCKED).unwrap(),
            GpuAccessPolicy::Unblocked
        );
        assert_eq!(
            gpu_access_from_wire(gpu_access::BLOCKED).unwrap(),
            GpuAccessPolicy::Blocked
        );
        assert_eq!(
            gpu_access_from_wire(gpu_access::PENDING).unwrap(),
            GpuAccessPolicy::Pending
        );
        assert_eq!(
            gpu_access_from_wire(gpu_access::UNKNOWN).unwrap(),
            GpuAccessPolicy::Unknown
        );
    }

    #[test]
    fn gpu_wire_unknown_value_is_internal_error() {
        assert!(matches!(
            gpu_power_from_wire(99),
            Err(ProviderError::Internal(_))
        ));
        assert!(matches!(
            gpu_mux_from_wire(99),
            Err(ProviderError::Internal(_))
        ));
        assert!(matches!(
            gpu_access_from_wire(99),
            Err(ProviderError::Internal(_))
        ));
    }
}
