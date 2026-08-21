//! # orbis-session-client
//!
//! Client-side Battery Charge Limit providers поверх D-Bus protocol
//! `orbis-session-protocol` (generated `Session1Proxy`).
//!
//! - source получает готовую `zbus::Connection` извне; crate не открывает
//!   session/system bus, не создаёт runtime и не выполняет hardware access;
//! - read path идёт через `Session1`, mutation path — напрямую через `Hardware1`;
//! - каждый вызов `charge_limit()` выполняет новый authoritative property Get
//!   (кэш отсутствует).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::{
    BatteryThresholdConfidence, BatteryThresholdEvidence, BatteryThresholdEvidenceState,
    BatteryThresholdFreshness, BatteryThresholdObservation, BatteryThresholdSource, ChargeLimit,
    ChargeLimitBounds,
};
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::fan::{FanCurve, FanCurvePoint, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::{FanPwm, Percent, TemperatureC};
use orbis_core::profile::{AsusdFanProfile, PerformanceProfile};
use orbis_hardwared::battery::validate_charge_limit;
use orbis_hardwared::fans::{FanCurveWire, fan_profile_from_wire};
use orbis_hardwared::{DBUS_OBJECT_PATH, Hardware1Proxy};
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{
    BatteryProvider, FanCurveMutationProvider, FanCurvePoints, FanProvider, GpuAccessProvider,
    GpuMuxProvider, GpuPowerProvider, PerformanceProvider, Provider, ProviderHealth,
};
use orbis_session_protocol::{
    BatteryThresholdEvidenceTuple, ChargeLimitInfo, PerformanceInfo, Session1Proxy,
    battery_threshold_confidence, battery_threshold_freshness, battery_threshold_source,
    battery_threshold_state, gpu_access, gpu_mux, gpu_power, performance,
};
use zbus::proxy::CacheProperties;

/// Testable источник wire DTO через session protocol.
#[async_trait]
pub trait SessionChargeLimitSource: Send + Sync {
    /// Прочитать authoritative `ChargeLimitInfo` (wire DTO, без domain
    /// conversion); ошибка не превращается в bool/None.
    async fn read_charge_limit(&self) -> Result<ChargeLimitInfo, ProviderError>;

    /// Прочитать source-labelled threshold evidence.
    async fn read_threshold_evidence(
        &self,
    ) -> Result<BatteryThresholdEvidenceTuple, ProviderError> {
        Err(ProviderError::Unsupported(
            "session protocol: battery threshold evidence unavailable".into(),
        ))
    }
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

    async fn read_threshold_evidence(
        &self,
    ) -> Result<BatteryThresholdEvidenceTuple, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy
            .battery_threshold_evidence()
            .await
            .map_err(zbus_error_to_provider)
    }
}

/// Convert untrusted Session1 threshold evidence into the domain model.
pub fn battery_threshold_evidence_from_wire(
    wire: BatteryThresholdEvidenceTuple,
) -> Result<BatteryThresholdEvidence, ProviderError> {
    let state = match wire.0 {
        battery_threshold_state::OBSERVED => BatteryThresholdEvidenceState::Observed,
        battery_threshold_state::CONFIRMED => BatteryThresholdEvidenceState::Confirmed,
        battery_threshold_state::CONFLICT => BatteryThresholdEvidenceState::Conflict,
        battery_threshold_state::UNKNOWN => BatteryThresholdEvidenceState::Unknown,
        other => {
            return Err(ProviderError::Internal(format!(
                "session protocol: unknown battery evidence state {other}"
            )));
        }
    };
    let mut observations = Vec::with_capacity(wire.1.len());
    for (source, value, observed_at_ms, freshness, confidence) in wire.1 {
        let source = match source {
            battery_threshold_source::UPOWER => BatteryThresholdSource::UPower,
            battery_threshold_source::ASUS_BACKEND => BatteryThresholdSource::AsusBackend,
            battery_threshold_source::SYSFS => BatteryThresholdSource::Sysfs,
            other => {
                return Err(ProviderError::Internal(format!(
                    "session protocol: unknown battery evidence source {other}"
                )));
            }
        };
        let freshness = match freshness {
            battery_threshold_freshness::FRESH => BatteryThresholdFreshness::Fresh,
            battery_threshold_freshness::STALE => BatteryThresholdFreshness::Stale,
            battery_threshold_freshness::UNKNOWN => BatteryThresholdFreshness::Unknown,
            other => {
                return Err(ProviderError::Internal(format!(
                    "session protocol: unknown battery evidence freshness {other}"
                )));
            }
        };
        let confidence = match confidence {
            battery_threshold_confidence::LOW => BatteryThresholdConfidence::Low,
            battery_threshold_confidence::MEDIUM => BatteryThresholdConfidence::Medium,
            battery_threshold_confidence::HIGH => BatteryThresholdConfidence::High,
            other => {
                return Err(ProviderError::Internal(format!(
                    "session protocol: unknown battery evidence confidence {other}"
                )));
            }
        };
        observations.push(BatteryThresholdObservation {
            source,
            value: Percent::new(value).map_err(|error| {
                ProviderError::Internal(format!("session protocol: invalid threshold: {error}"))
            })?,
            observed_at: SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_millis(observed_at_ms))
                .ok_or_else(|| {
                    ProviderError::Internal("session protocol: timestamp overflow".into())
                })?,
            freshness,
            confidence,
        });
    }
    let evidence = BatteryThresholdEvidence::from_observations(observations);
    if evidence.state != state {
        return Err(ProviderError::Internal(
            "session protocol: battery evidence state does not match observations".into(),
        ));
    }
    Ok(evidence)
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
            zbus::fdo::Error::Failed(message)
                if message
                    .starts_with(orbis_session_protocol::BATTERY_THRESHOLD_CONFLICT_PREFIX) =>
            {
                ProviderError::Conflict(
                    message
                        .trim_start_matches(
                            orbis_session_protocol::BATTERY_THRESHOLD_CONFLICT_PREFIX,
                        )
                        .to_string(),
                )
            }
            _ => ProviderError::Dbus(error.to_string()),
        };
    }
    ProviderError::Dbus(error.to_string())
}

/// Wire→domain boundary между недоверенным D-Bus payload и доменной моделью.
///
/// - canonical invariants для configured/effective absent fields проверяются
///   отдельно; несогласованный payload — ошибка
///   `Internal` (несогласованный payload не превращается молча в `None`);
/// - malformed bounds/percent/step → `Internal` (remote service нарушил
///   contract — это не ошибка пользовательского запроса);
/// - `enabled` сохраняется без преобразования.
pub fn charge_limit_from_wire(info: ChargeLimitInfo) -> Result<ChargeLimit, ProviderError> {
    if !info.configured_percent_present && info.configured_percent != 0 {
        return Err(ProviderError::Internal(format!(
            "session protocol: configured_percent_present=false, но configured_percent={} (нарушен canonical invariant)",
            info.configured_percent
        )));
    }

    if !info.effective_percent_present && info.effective_percent != 0 {
        return Err(ProviderError::Internal(format!(
            "session protocol: effective_percent_present=false, но effective_percent={} (нарушен canonical invariant)",
            info.effective_percent
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

    let configured = if info.configured_percent_present {
        Some(Percent::new(info.configured_percent).map_err(|e| {
            ProviderError::Internal(format!(
                "session protocol: невалидный configured percent: {e}"
            ))
        })?)
    } else {
        None
    };
    let effective = if info.effective_percent_present {
        Some(Percent::new(info.effective_percent).map_err(|e| {
            ProviderError::Internal(format!(
                "session protocol: невалидный effective percent: {e}"
            ))
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

    ChargeLimit::new(info.enabled, configured, effective, bounds).map_err(|e| {
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

    async fn threshold_evidence(&self) -> Result<BatteryThresholdEvidence, ProviderError> {
        battery_threshold_evidence_from_wire(self.source.read_threshold_evidence().await?)
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

// ---------------------------------------------------------------------------
// Read-only Performance Mode через Session1
// ---------------------------------------------------------------------------

/// Преобразовать wire `u8` в domain `PerformanceProfile`.
///
/// Strict decode: неизвестный wire value → `Internal` (remote service нарушил
/// contract).
pub fn performance_current_from_wire(raw: u8) -> Result<PerformanceProfile, ProviderError> {
    match raw {
        performance::SILENT => Ok(PerformanceProfile::Silent),
        performance::BALANCED => Ok(PerformanceProfile::Balanced),
        performance::TURBO => Ok(PerformanceProfile::Turbo),
        other => Err(ProviderError::Internal(format!(
            "session protocol: неизвестный performance current wire value {other}"
        ))),
    }
}

/// Преобразовать domain Performance profile в wire value.
fn performance_profile_to_wire(profile: PerformanceProfile) -> u8 {
    match profile {
        PerformanceProfile::Silent => performance::SILENT,
        PerformanceProfile::Balanced => performance::BALANCED,
        PerformanceProfile::Turbo => performance::TURBO,
    }
}

/// Преобразовать wire mask в список доступных `PerformanceProfile`.
///
/// Strict decode: установленный бит вне (bit0..=bit2) → `Internal`. Порядок
/// результата фиксирован порядком enum (`PerformanceProfile::ALL`), так как
/// маска не сохраняет порядок, отданный backend-ом; UI использует только set
/// доступности.
pub fn performance_mask_from_wire(mask: u8) -> Result<Vec<PerformanceProfile>, ProviderError> {
    let allowed: u8 = performance::SILENT_BIT | performance::BALANCED_BIT | performance::TURBO_BIT;
    if mask & !allowed != 0 {
        return Err(ProviderError::Internal(format!(
            "session protocol: performance available mask содержит неизвестные биты (raw={mask})"
        )));
    }
    let mut out = Vec::new();
    if mask & performance::SILENT_BIT != 0 {
        out.push(PerformanceProfile::Silent);
    }
    if mask & performance::BALANCED_BIT != 0 {
        out.push(PerformanceProfile::Balanced);
    }
    if mask & performance::TURBO_BIT != 0 {
        out.push(PerformanceProfile::Turbo);
    }
    Ok(out)
}

/// Testable источник Performance wire DTO через session protocol.
#[async_trait]
pub trait SessionPerformanceSource: Send + Sync {
    /// Прочитать authoritative `PerformanceInfo` (wire DTO, без domain
    /// conversion); ошибка не превращается в default.
    async fn read_performance(&self) -> Result<PerformanceInfo, ProviderError>;
}

/// Testable direct system-bus source для Performance mutation через Hardware1.
#[async_trait]
pub trait HardwarePerformanceSource: Send + Sync {
    /// Установить profile и вернуть подтверждённый hardware wire value.
    async fn set_performance(&self, profile: u8) -> Result<u8, ProviderError>;
}

/// Testable direct system-bus source для Battery mutation через Hardware1.
#[async_trait]
pub trait HardwareBatterySource: Send + Sync {
    /// Установить threshold и вернуть подтверждённый wire value.
    async fn set_charge_limit(&self, percent: u8) -> Result<u8, ProviderError>;
}

/// Testable direct system-bus source для fan curve mutation через Hardware1.
#[async_trait]
pub trait HardwareFanCurveSource: Send + Sync {
    /// Установить одну fan curve; вернуть подтверждённый profile wire value.
    async fn set_fan_curve(
        &self,
        profile: u32,
        fan: u8,
        curve: orbis_hardwared::fans::FanCurveWire,
    ) -> Result<u32, ProviderError>;
}

/// Реальный direct system-bus источник fan curve mutation через Hardware1.
///
/// Connection создаётся и хранится в application/GUI process; sessiond в этот
/// путь не входит и sender Hardware1 остаётся исходным caller process.
pub struct ZbusHardwareFanCurveSource {
    connection: zbus::Connection,
}

impl ZbusHardwareFanCurveSource {
    /// Создать источник над готовой system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl HardwareFanCurveSource for ZbusHardwareFanCurveSource {
    async fn set_fan_curve(
        &self,
        profile: u32,
        fan: u8,
        curve: orbis_hardwared::fans::FanCurveWire,
    ) -> Result<u32, ProviderError> {
        let proxy = Hardware1Proxy::builder(&self.connection)
            .path(DBUS_OBJECT_PATH)
            .expect("valid hardware object path")
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy
            .set_fan_curve(profile, fan, curve)
            .await
            .map_err(zbus_error_to_provider)
    }
}

/// Реальный direct system-bus источник Battery mutation через Hardware1.
///
/// Connection создаётся и хранится в application/GUI process; sessiond в этот
/// путь не входит и sender Hardware1 остаётся исходным caller process.
pub struct ZbusHardwareBatterySource {
    connection: zbus::Connection,
}

impl ZbusHardwareBatterySource {
    /// Создать источник над готовой system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl HardwareBatterySource for ZbusHardwareBatterySource {
    async fn set_charge_limit(&self, percent: u8) -> Result<u8, ProviderError> {
        let proxy = Hardware1Proxy::builder(&self.connection)
            .path(DBUS_OBJECT_PATH)
            .expect("valid hardware object path")
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy
            .set_charge_limit(percent)
            .await
            .map_err(zbus_error_to_provider)
    }
}

/// Decode the Hardware1 `BatteryMutationStatus` wire value into the canonical
/// domain capability status.
///
/// Unknown wire values and the explicit `Unknown` evidence both map to
/// `CapabilityStatus::Unknown` — never to a guessed known state.
pub fn battery_mutation_status_from_wire(raw: u8) -> orbis_core::capability::CapabilityStatus {
    use orbis_core::capability::CapabilityStatus;
    use orbis_hardwared::battery::{BatteryMutationStatus, battery_mutation_wire};
    match battery_mutation_wire::from_wire(raw) {
        Some(BatteryMutationStatus::Supported) => CapabilityStatus::Supported,
        Some(BatteryMutationStatus::Unsupported) => CapabilityStatus::Unsupported,
        Some(BatteryMutationStatus::TemporarilyUnavailable) => {
            CapabilityStatus::TemporarilyUnavailable
        }
        Some(BatteryMutationStatus::PermissionDenied) => CapabilityStatus::PermissionDenied,
        Some(BatteryMutationStatus::Unknown) | None => CapabilityStatus::Unknown,
    }
}

/// Read-only typed Battery mutation backend status from the production
/// Hardware1 daemon.
///
/// This performs no mutation and requires no authorization. When the daemon is
/// absent, the method is not exposed, or the reply is malformed, the result is
/// `CapabilityStatus::Unknown` — the honest no-evidence state, never a guessed
/// `Supported`.
pub async fn hardware1_battery_mutation_status(
    connection: &zbus::Connection,
) -> orbis_core::capability::CapabilityStatus {
    let proxy = match Hardware1Proxy::builder(connection)
        .path(DBUS_OBJECT_PATH)
        .expect("valid hardware object path")
        .cache_properties(CacheProperties::No)
        .build()
        .await
    {
        Ok(proxy) => proxy,
        Err(_) => return orbis_core::capability::CapabilityStatus::Unknown,
    };
    match proxy.battery_mutation_status().await {
        Ok(raw) => battery_mutation_status_from_wire(raw),
        Err(_) => orbis_core::capability::CapabilityStatus::Unknown,
    }
}

/// Decode the Hardware1 `PerformanceMutationStatus` wire value into the
/// canonical domain capability status.
///
/// Unknown wire values and the explicit `Unknown` evidence both map to
/// `CapabilityStatus::Unknown` — never to a guessed known state.
pub fn performance_mutation_status_from_wire(raw: u8) -> orbis_core::capability::CapabilityStatus {
    use orbis_core::capability::CapabilityStatus;
    use orbis_hardwared::{PerformanceMutationStatus, performance_mutation_wire};
    match performance_mutation_wire::from_wire(raw) {
        Some(PerformanceMutationStatus::Supported) => CapabilityStatus::Supported,
        Some(PerformanceMutationStatus::Unsupported) => CapabilityStatus::Unsupported,
        Some(PerformanceMutationStatus::TemporarilyUnavailable) => {
            CapabilityStatus::TemporarilyUnavailable
        }
        Some(PerformanceMutationStatus::PermissionDenied) => CapabilityStatus::PermissionDenied,
        Some(PerformanceMutationStatus::Unknown) | None => CapabilityStatus::Unknown,
    }
}

/// Read-only typed Performance mutation backend status from the production
/// Hardware1 daemon.
///
/// This performs no mutation and requires no authorization. When the daemon is
/// absent, the method is not exposed, or the reply is malformed, the result is
/// `CapabilityStatus::Unknown` — the honest no-evidence state, never a guessed
/// `Supported`.
pub async fn hardware1_performance_mutation_status(
    connection: &zbus::Connection,
) -> orbis_core::capability::CapabilityStatus {
    let proxy = match Hardware1Proxy::builder(connection)
        .path(DBUS_OBJECT_PATH)
        .expect("valid hardware object path")
        .cache_properties(CacheProperties::No)
        .build()
        .await
    {
        Ok(proxy) => proxy,
        Err(_) => return orbis_core::capability::CapabilityStatus::Unknown,
    };
    match proxy.performance_mutation_status().await {
        Ok(raw) => performance_mutation_status_from_wire(raw),
        Err(_) => orbis_core::capability::CapabilityStatus::Unknown,
    }
}

/// Decode the Hardware1 `FanMutationStatus` wire value into the canonical
/// domain capability status.
///
/// Unknown wire values and the explicit `Unknown` evidence both map to
/// `CapabilityStatus::Unknown` — never to a guessed known state.
pub fn fan_mutation_status_from_wire(raw: u8) -> orbis_core::capability::CapabilityStatus {
    use orbis_core::capability::CapabilityStatus;
    use orbis_hardwared::fans::{FanMutationStatus, fan_mutation_wire};
    match fan_mutation_wire::from_wire(raw) {
        Some(FanMutationStatus::Supported) => CapabilityStatus::Supported,
        Some(FanMutationStatus::Unsupported) => CapabilityStatus::Unsupported,
        Some(FanMutationStatus::TemporarilyUnavailable) => CapabilityStatus::TemporarilyUnavailable,
        Some(FanMutationStatus::PermissionDenied) => CapabilityStatus::PermissionDenied,
        Some(FanMutationStatus::BackendMissing) => CapabilityStatus::BackendMissing,
        Some(FanMutationStatus::Unknown) | None => CapabilityStatus::Unknown,
    }
}

/// Read-only typed fan curve mutation backend status from the production
/// Hardware1 daemon.
///
/// This performs no mutation and requires no authorization. When the daemon is
/// absent, the method is not exposed, or the reply is malformed, the result is
/// `CapabilityStatus::Unknown` — the honest no-evidence state, never a guessed
/// `Supported`.
pub async fn hardware1_fan_mutation_status(
    connection: &zbus::Connection,
) -> orbis_core::capability::CapabilityStatus {
    let proxy = match Hardware1Proxy::builder(connection)
        .path(DBUS_OBJECT_PATH)
        .expect("valid hardware object path")
        .cache_properties(CacheProperties::No)
        .build()
        .await
    {
        Ok(proxy) => proxy,
        Err(_) => return orbis_core::capability::CapabilityStatus::Unknown,
    };
    match proxy.fan_mutation_status().await {
        Ok(raw) => fan_mutation_status_from_wire(raw),
        Err(_) => orbis_core::capability::CapabilityStatus::Unknown,
    }
}

/// Реальный zbus источник Performance через generated `Session1Proxy`.
///
/// Хранит переданную извне готовую `Connection`; I/O начинается только в
/// `read_performance().await`.
pub struct ZbusSessionPerformanceSource {
    connection: zbus::Connection,
}

impl ZbusSessionPerformanceSource {
    /// Создать источник с готовой Connection.
    ///
    /// Конструктор не выполняет I/O, не открывает session/system bus, не
    /// проверяет service, не создаёт proxy и runtime; cache отсутствует.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl SessionPerformanceSource for ZbusSessionPerformanceSource {
    async fn read_performance(&self) -> Result<PerformanceInfo, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy.performance().await.map_err(zbus_error_to_provider)
    }
}

/// Реальный direct system-bus источник Hardware1 для Performance mutation.
///
/// Connection создаётся и хранится в application/GUI process; sessiond в этот
/// путь не входит и sender Hardware1 остаётся исходным caller process.
pub struct ZbusHardwarePerformanceSource {
    connection: zbus::Connection,
}

impl ZbusHardwarePerformanceSource {
    /// Создать источник над готовой system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl HardwarePerformanceSource for ZbusHardwarePerformanceSource {
    async fn set_performance(&self, profile: u8) -> Result<u8, ProviderError> {
        let proxy = Hardware1Proxy::builder(&self.connection)
            .path(DBUS_OBJECT_PATH)
            .expect("valid hardware object path")
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

/// Performance Mode provider над session protocol source.
///
/// `S` — read-only Session1 source (реальный zbus или scripted в тестах).
/// Mutation-методы этого provider честно возвращают `Unsupported`.
pub struct SessionPerformanceProvider<S> {
    source: S,
}

impl<S> SessionPerformanceProvider<S> {
    /// Создать provider над source.
    ///
    /// Не выполняет D-Bus чтение и не открывает Connection.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SessionPerformanceProvider<S>
where
    S: SessionPerformanceSource,
{
    fn id(&self) -> &'static str {
        "session-performance"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-performance")
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
            "provider.session-performance",
            "read-only session protocol Performance Mode backend",
        )]
    }
}

#[async_trait]
impl<S> PerformanceProvider for SessionPerformanceProvider<S>
where
    S: SessionPerformanceSource,
{
    async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
        let info = self.source.read_performance().await?;
        performance_mask_from_wire(info.available_mask)
    }

    async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
        let info = self.source.read_performance().await?;
        performance_current_from_wire(info.current)
    }

    async fn set_profile(
        &self,
        _profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session protocol read-only: set_profile недоступна".into(),
        ))
    }

    async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        Ok(None)
    }

    async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        Ok(None)
    }

    fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
        ValidationResult::invalid("session protocol read-only: запись profile не поддерживается")
    }
}

/// Composed Performance provider: authoritative reads через Session1, mutation
/// напрямую через Hardware1 из application/GUI process.
pub struct SessionHardwarePerformanceProvider<S, H> {
    session: S,
    hardware: H,
}

/// Composed Battery provider: authoritative reads через Session1, mutation
/// напрямую через Hardware1 из application/GUI process.
pub struct SessionHardwareBatteryProvider<S, H> {
    session: S,
    hardware: H,
}

impl<S, H> SessionHardwareBatteryProvider<S, H> {
    /// Создать composed provider без I/O на construction.
    pub fn new(session: S, hardware: H) -> Self {
        Self { session, hardware }
    }
}

impl<S, H> Provider for SessionHardwareBatteryProvider<S, H>
where
    S: BatteryProvider,
    H: HardwareBatterySource,
{
    fn id(&self) -> &'static str {
        "session-hardware-battery"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-hardware-battery")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session + hardware Battery backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-hardware-battery",
            "Session1 read + direct Hardware1 Battery backend",
        )]
    }
}

#[async_trait]
impl<S, H> BatteryProvider for SessionHardwareBatteryProvider<S, H>
where
    S: BatteryProvider,
    H: HardwareBatterySource,
{
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        self.session.charge_limit().await
    }

    async fn set_charge_limit(&self, percent: u8) -> Result<ApplyResult, ProviderError> {
        validate_charge_limit(percent)?;
        let confirmed = self.hardware.set_charge_limit(percent).await?;
        if confirmed != percent {
            return Err(ProviderError::Internal(format!(
                "hardware protocol: подтверждён другой Battery threshold: requested={percent}, confirmed={confirmed}"
            )));
        }
        Ok(ApplyResult::Applied)
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "Hardware1 Battery provider: one_shot_full_charge недоступна".into(),
        ))
    }

    fn validate_charge_limit(&self, percent: u8) -> ValidationResult {
        match validate_charge_limit(percent) {
            Ok(()) => ValidationResult::ok(),
            Err(ProviderError::InvalidRequest(message)) => ValidationResult::invalid(message),
            Err(error) => ValidationResult::invalid(error.to_string()),
        }
    }
}

impl<S, H> SessionHardwarePerformanceProvider<S, H> {
    /// Создать composed provider без I/O на construction.
    pub fn new(session: S, hardware: H) -> Self {
        Self { session, hardware }
    }
}

impl<S, H> Provider for SessionHardwarePerformanceProvider<S, H>
where
    S: SessionPerformanceSource,
    H: HardwarePerformanceSource,
{
    fn id(&self) -> &'static str {
        "session-hardware-performance"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-hardware-performance")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session + hardware Performance backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-hardware-performance",
            "Session1 read + direct Hardware1 Performance backend",
        )]
    }
}

#[async_trait]
impl<S, H> PerformanceProvider for SessionHardwarePerformanceProvider<S, H>
where
    S: SessionPerformanceSource,
    H: HardwarePerformanceSource,
{
    async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
        let info = self.session.read_performance().await?;
        performance_mask_from_wire(info.available_mask)
    }

    async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
        let info = self.session.read_performance().await?;
        performance_current_from_wire(info.current)
    }

    async fn set_profile(&self, profile: PerformanceProfile) -> Result<ApplyResult, ProviderError> {
        let requested = performance_profile_to_wire(profile);
        tracing::debug!(
            requested_profile = ?profile,
            requested_wire = requested,
            "performance hardware mutation request"
        );
        let confirmed = match self.hardware.set_performance(requested).await {
            Ok(confirmed) => {
                tracing::debug!(
                    requested_wire = requested,
                    confirmed_wire = confirmed,
                    "performance hardware mutation reply"
                );
                confirmed
            }
            Err(error) => {
                tracing::debug!(
                    requested_wire = requested,
                    error = ?error,
                    "performance hardware mutation error"
                );
                return Err(error);
            }
        };
        let confirmed_profile = performance_current_from_wire(confirmed)?;
        if confirmed_profile != profile {
            return Err(ProviderError::Conflict(format!(
                "hardware protocol: подтверждён другой profile: requested={profile:?}, confirmed={confirmed_profile:?}"
            )));
        }
        Ok(ApplyResult::Applied)
    }

    async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        Ok(None)
    }

    async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        Ok(None)
    }

    fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
        ValidationResult::ok()
    }
}

// ---------------------------------------------------------------------------
// Read-only profile-specific fan curve read через Session1
// ---------------------------------------------------------------------------

/// Testable источник wire `FanCurveInfo` через session protocol.
///
/// Read-only: возвращает сохранённую кривую профиля (asusd), НЕ активную
/// системную кривую. Ошибка не превращается в bool/None/default.
#[async_trait]
pub trait SessionFanCurveSource: Send + Sync {
    /// Прочитать authoritative `FanCurveInfo` для профиля и вентилятора
    /// (wire DTO, без domain conversion).
    async fn read_fan_curve(
        &self,
        profile: u32,
        fan: u8,
    ) -> Result<orbis_session_protocol::FanCurveInfo, ProviderError>;
}

/// Реальный zbus источник через generated `Session1Proxy`.
///
/// Хранит переданную извне готовую `Connection`; I/O начинается только в
/// `read_fan_curve().await`. Каждый вызов — новый authoritative method call
/// (кэш отсутствует).
pub struct ZbusSessionFanCurveSource {
    connection: zbus::Connection,
}

impl ZbusSessionFanCurveSource {
    /// Создать источник с готовой Connection.
    ///
    /// Конструктор не выполняет I/O, не открывает session/system bus, не
    /// проверяет service, не создаёт proxy и runtime; cache отсутствует.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl SessionFanCurveSource for ZbusSessionFanCurveSource {
    async fn read_fan_curve(
        &self,
        profile: u32,
        fan: u8,
    ) -> Result<orbis_session_protocol::FanCurveInfo, ProviderError> {
        let proxy = Session1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)?;
        proxy
            .fan_curve(profile, fan)
            .await
            .map_err(zbus_error_to_provider)
    }
}

/// Преобразовать wire `u8` → `FanId` (0=CPU, 1=GPU).
///
/// Strict decode: неизвестный wire value → `Internal` (remote service нарушил
/// contract).
pub fn fan_id_from_wire_protocol(raw: u8) -> Result<FanId, ProviderError> {
    use orbis_session_protocol::fan_id;
    match raw {
        fan_id::CPU => Ok(FanId::Cpu),
        fan_id::GPU => Ok(FanId::Gpu),
        other => Err(ProviderError::Internal(format!(
            "session protocol: неизвестный fan wire value {other}"
        ))),
    }
}

/// Преобразовать wire `u32` → lossless `AsusdFanProfile` (0..3).
///
/// Strict decode: неизвестный profile wire → `Internal`; Silent не выводится
/// автоматически в Quiet/LowPower (wire lossless 0..3).
pub fn fan_profile_from_wire_protocol(raw: u32) -> Result<AsusdFanProfile, ProviderError> {
    use orbis_session_protocol::fan_profile;
    match raw {
        fan_profile::BALANCED => Ok(AsusdFanProfile::Balanced),
        fan_profile::PERFORMANCE => Ok(AsusdFanProfile::Performance),
        fan_profile::QUIET => Ok(AsusdFanProfile::Quiet),
        fan_profile::LOW_POWER => Ok(AsusdFanProfile::LowPower),
        other => Err(ProviderError::Internal(format!(
            "session protocol: неизвестный fan profile wire value {other}"
        ))),
    }
}

/// Wire→domain boundary между недоверенным D-Bus payload и доменной моделью.
///
/// - wire содержит ровно 8 точек (массивы одинаковой длины); malformed →
///   `Internal` (не превращаются в синтетическую кривую);
/// - температу/raw PWM переносятся lossless (raw PWM 0..255, НЕ процент);
/// - returned `profile` из wire проверяется на соответствие запрошенному —
///   расхождение → `Internal` (remote service нарушил contract);
/// - возвращает доменную `FanCurve` с lossless `AsusdFanProfile` в `profile`
///   (домом-бится через `PerformanceProfile::from`).
pub fn fan_curve_from_wire(
    wire: orbis_session_protocol::FanCurveInfo,
    requested_profile: AsusdFanProfile,
) -> Result<FanCurve, ProviderError> {
    let wire_profile = fan_profile_from_wire_protocol(wire.profile)?;
    if wire_profile != requested_profile {
        return Err(ProviderError::Internal(format!(
            "session protocol: ответ profile {} не совпадает с запрошенным {:?}",
            wire.profile, requested_profile
        )));
    }
    if wire.temps.len() != 8 || wire.pwms.len() != 8 {
        return Err(ProviderError::Internal(format!(
            "session protocol: кривая содержит {} temp / {} pwm, ожидается 8/8",
            wire.temps.len(),
            wire.pwms.len()
        )));
    }
    let fan = fan_id_from_wire_protocol(wire.fan)?;
    let mut points = Vec::with_capacity(8);
    for (t, p) in wire.temps.iter().zip(wire.pwms.iter()) {
        let temp = TemperatureC::new(i16::from(*t)).map_err(|e| {
            ProviderError::Internal(format!("session protocol: температура вне диапазона: {e}"))
        })?;
        let pwm = FanPwm::new(*p).map_err(|e| {
            ProviderError::Internal(format!("session protocol: PWM вне диапазона: {e}"))
        })?;
        points.push(FanCurvePoint::new(temp, pwm));
    }
    Ok(FanCurve {
        profile: PerformanceProfile::from(wire_profile),
        fan,
        enabled: Some(wire.enabled),
        points,
    })
}

/// Composed read-only FanProvider: profile-specific curves через Session1
/// (sessiond → asusd), активная кривая — через existing sysfs source.
///
/// `S` — profile read через Session1 (`SessionFanCurveSource`), `A` — active
/// curve источник (существующий sysfs `FanCurveSource`). Профиль-специничные
/// чтения НЕ смешиваются с активной кривой; `fan_curve(PerformanceProfile)`
/// (не порт профильной кривой) честно возвращает `Unsupported`.
pub struct SessionProfileFanCurveProvider<S, A> {
    session: S,
    active: A,
}

impl<S, A> SessionProfileFanCurveProvider<S, A> {
    /// Создать composed read provider без I/O на construction.
    pub fn new(session: S, active: A) -> Self {
        Self { session, active }
    }
}

impl<S, A> Provider for SessionProfileFanCurveProvider<S, A>
where
    S: SessionFanCurveSource,
    A: FanProvider,
{
    fn id(&self) -> &'static str {
        "session-profile-fan-curve"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-profile-fan-curve")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session + active fan curve backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-profile-fan-curve",
            "Session1 profile-specific fan curve read + active sysfs curve backend",
        )]
    }
}

#[async_trait]
impl<S, A> FanProvider for SessionProfileFanCurveProvider<S, A>
where
    S: SessionFanCurveSource,
    A: FanProvider,
{
    async fn fan_ids(&self) -> Result<Vec<FanId>, ProviderError> {
        self.active.fan_ids().await
    }

    async fn fan_rpms(&self) -> Result<Vec<(FanId, orbis_core::newtypes::Rpm)>, ProviderError> {
        self.active.fan_rpms().await
    }

    async fn fan_curve(
        &self,
        profile: PerformanceProfile,
        fan: &FanId,
    ) -> Result<FanCurve, ProviderError> {
        // PerformanceProfile-based profile read не сохраняет lossless
        // AsusdFanProfile (Quiet vs LowPower); используйте
        // `fan_curve_for_profile` для profile-specific read.
        self.active.fan_curve(profile, fan).await
    }

    async fn fan_curve_for_profile(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
    ) -> Result<FanCurve, ProviderError> {
        let info = self
            .session
            .read_fan_curve(profile.wire(), fan_wire_from_id(fan)?)
            .await?;
        fan_curve_from_wire(info, profile)
    }

    async fn active_curve(&self, fan: &FanId) -> Result<FanCurve, ProviderError> {
        self.active.active_curve(fan).await
    }

    async fn set_fan_curve(&self, _curve: &FanCurve) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session-profile-fan-curve: read-only, запись не поддерживается".into(),
        ))
    }

    async fn set_curves_to_defaults(
        &self,
        _profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "session-profile-fan-curve: defaults/reset не поддерживается (Hardware1 mutation)"
                .into(),
        ))
    }

    fn curve_point_count(&self) -> usize {
        self.active.curve_point_count()
    }

    fn allow_decreasing(&self) -> bool {
        self.active.allow_decreasing()
    }

    fn validate_curve(&self, curve: &FanCurve) -> ValidationResult {
        self.active.validate_curve(curve)
    }
}

/// Composed fan curve mutation provider: read через `FanProvider`
/// (sessiond read-only backend), mutation напрямую через Hardware1
/// (`HardwareFanCurveSource`).
///
/// Mutation использует lossless `AsusdFanProfile` (не `PerformanceProfile`),
/// чтобы Quiet/LowPower оставались различимыми. Original caller сохраняется:
/// connection живёт в application/GUI process, sessiond в путь не входит.
pub struct SessionHardwareFanCurveProvider<S, H> {
    session: S,
    hardware: H,
}

impl<S, H> SessionHardwareFanCurveProvider<S, H> {
    /// Создать composed provider без I/O на construction.
    pub fn new(session: S, hardware: H) -> Self {
        Self { session, hardware }
    }
}

impl<S, H> Provider for SessionHardwareFanCurveProvider<S, H>
where
    S: FanProvider,
    H: HardwareFanCurveSource,
{
    fn id(&self) -> &'static str {
        "session-hardware-fan-curve"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("session-hardware-fan-curve")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("session + hardware fan curve backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.session-hardware-fan-curve",
            "composed session read + hardware mutation fan curve backend",
        )]
    }
}

#[async_trait]
impl<S, H> FanProvider for SessionHardwareFanCurveProvider<S, H>
where
    S: FanProvider,
    H: HardwareFanCurveSource,
{
    async fn fan_ids(&self) -> Result<Vec<FanId>, ProviderError> {
        self.session.fan_ids().await
    }

    async fn fan_rpms(&self) -> Result<Vec<(FanId, orbis_core::newtypes::Rpm)>, ProviderError> {
        self.session.fan_rpms().await
    }

    async fn fan_curve(
        &self,
        profile: PerformanceProfile,
        fan: &FanId,
    ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
        self.session.fan_curve(profile, fan).await
    }

    async fn fan_curve_for_profile(
        &self,
        profile: orbis_core::profile::AsusdFanProfile,
        fan: &FanId,
    ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
        self.session.fan_curve_for_profile(profile, fan).await
    }

    async fn active_curve(&self, fan: &FanId) -> Result<orbis_core::fan::FanCurve, ProviderError> {
        self.session.active_curve(fan).await
    }

    async fn set_fan_curve(
        &self,
        _curve: &orbis_core::fan::FanCurve,
    ) -> Result<ApplyResult, ProviderError> {
        // PerformanceProfile-based mutation не поддерживается: используйте
        // `FanCurveMutationProvider::set_fan_curve` (lossless AsusdFanProfile).
        Err(ProviderError::Unsupported(
            "fan curve mutation: используйте typed FanCurveMutationProvider (AsusdFanProfile), не PerformanceProfile-based set_fan_curve".into(),
        ))
    }

    async fn set_curves_to_defaults(
        &self,
        _profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "fan curve defaults/reset не поддерживается".into(),
        ))
    }

    fn curve_point_count(&self) -> usize {
        self.session.curve_point_count()
    }

    fn allow_decreasing(&self) -> bool {
        self.session.allow_decreasing()
    }

    fn validate_curve(&self, curve: &orbis_core::fan::FanCurve) -> ValidationResult {
        self.session.validate_curve(curve)
    }
}

#[async_trait]
impl<S, H> FanCurveMutationProvider for SessionHardwareFanCurveProvider<S, H>
where
    S: FanProvider,
    H: HardwareFanCurveSource,
{
    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
        curve: &FanCurvePoints,
    ) -> Result<ApplyResult, ProviderError> {
        let requested_profile = profile.wire();
        let requested_fan = fan_wire_from_id(fan)?;
        let mut temps = Vec::with_capacity(curve.temps.len());
        for temp in &curve.temps {
            let raw = temp.get();
            let raw_u8 = u8::try_from(raw).map_err(|_| {
                ProviderError::InvalidRequest(format!(
                    "hardware fan curve: температура {raw} вне wire диапазона 0..=255"
                ))
            })?;
            temps.push(raw_u8);
        }
        let wire = FanCurveWire {
            temps,
            pwms: curve.pwms.iter().map(|p| p.get()).collect(),
        };
        tracing::debug!(
            requested_profile,
            requested_fan,
            temps = ?wire.temps,
            pwms = ?wire.pwms,
            "fan curve hardware mutation request"
        );
        let confirmed = match self
            .hardware
            .set_fan_curve(requested_profile, requested_fan, wire)
            .await
        {
            Ok(confirmed) => {
                tracing::debug!(
                    requested_profile,
                    confirmed_profile = confirmed,
                    "fan curve hardware mutation reply"
                );
                confirmed
            }
            Err(error) => {
                tracing::debug!(
                    requested_profile,
                    error = ?error,
                    "fan curve hardware mutation error"
                );
                return Err(error);
            }
        };
        let confirmed_profile = fan_profile_from_wire(confirmed).map_err(|err| match err {
            ProviderError::InvalidRequest(msg) => ProviderError::Internal(format!(
                "hardware protocol: malformed fan profile confirmation: {msg}"
            )),
            other => other,
        })?;
        if confirmed_profile != profile {
            return Err(ProviderError::Internal(format!(
                "hardware protocol: подтверждён другой fan profile: requested={profile:?}, confirmed={confirmed_profile:?}"
            )));
        }
        Ok(ApplyResult::Applied)
    }
}

/// Strict mapping `FanId` → wire `u8` (0=CPU, 1=GPU) для Hardware1.
///
/// Hardware1 поддерживает только CPU/GPU; остальные `FanId` — `InvalidRequest`.
fn fan_wire_from_id(fan: &FanId) -> Result<u8, ProviderError> {
    match fan {
        FanId::Cpu => Ok(0),
        FanId::Gpu => Ok(1),
        other => Err(ProviderError::InvalidRequest(format!(
            "hardware fan curve: неподдерживаемый fan {other:?} (только CPU/GPU)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use orbis_core::fan::{FanCurve, FanCurvePoint};
    use orbis_core::newtypes::{FanPwm, TemperatureC};

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
        let info = ChargeLimitInfo::with_percent(true, 80, 80, 40, 100, 5);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(limit.enabled);
        assert_eq!(limit.configured_percent.map(|p| p.get()), Some(80));
        assert_eq!(limit.effective_percent.map(|p| p.get()), Some(80));
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
        assert_eq!(limit.configured_percent, None);
        assert_eq!(limit.effective_percent, None);
        let b = limit.bounds.expect("known bounds");
        assert_eq!(b.min.get(), 40);
        assert_eq!(b.max.get(), 100);
        assert_eq!(b.step, 5);
    }

    #[test]
    fn preserves_disabled_state_with_known_percent() {
        let info = ChargeLimitInfo {
            enabled: false,
            configured_percent_present: true,
            configured_percent: 80,
            effective_percent_present: true,
            effective_percent: 100,
            bounds_present: true,
            min_percent: 40,
            max_percent: 100,
            step_percent: 5,
        };
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(!limit.enabled);
        // Выключенная функция не означает отсутствие известного threshold.
        assert_eq!(limit.configured_percent.map(|p| p.get()), Some(80));
        assert_eq!(limit.effective_percent.map(|p| p.get()), Some(100));
    }

    #[test]
    fn rejects_noncanonical_missing_percent() {
        let info = ChargeLimitInfo {
            enabled: true,
            configured_percent_present: false,
            configured_percent: 80,
            effective_percent_present: false,
            effective_percent: 0,
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
        let bad_range = ChargeLimitInfo::with_percent(true, 80, 80, 100, 40, 5);
        assert!(matches!(
            charge_limit_from_wire(bad_range).expect_err("min > max"),
            ProviderError::Internal(_)
        ));
        // percent вне [min, max]
        let bad_percent = ChargeLimitInfo::with_percent(true, 30, 30, 40, 100, 5);
        assert!(matches!(
            charge_limit_from_wire(bad_percent).expect_err("percent < min"),
            ProviderError::Internal(_)
        ));
        // step == 0
        let bad_step = ChargeLimitInfo::with_percent(true, 80, 80, 40, 100, 0);
        assert!(matches!(
            charge_limit_from_wire(bad_step).expect_err("step == 0"),
            ProviderError::Internal(_)
        ));
    }

    #[tokio::test]
    async fn provider_reads_source_once() {
        let source = ScriptedSource::new(vec![ScriptedRead::Info(ChargeLimitInfo::with_percent(
            true, 80, 80, 40, 100, 5,
        ))]);
        let provider = SessionChargeLimitProvider::new(source);
        let limit = provider.charge_limit().await.expect("charge limit");
        assert_eq!(limit.configured_percent.map(|p| p.get()), Some(80));
        assert_eq!(provider.source.reads(), 1);
    }

    #[tokio::test]
    async fn provider_does_not_cache_wire_state() {
        let source = ScriptedSource::new(vec![
            ScriptedRead::Info(ChargeLimitInfo::with_percent(true, 80, 80, 40, 100, 5)),
            ScriptedRead::Info(ChargeLimitInfo::with_percent(true, 60, 60, 40, 100, 5)),
        ]);
        let provider = SessionChargeLimitProvider::new(source);
        let first = provider.charge_limit().await.expect("read1");
        let second = provider.charge_limit().await.expect("read2");
        assert_eq!(first.configured_percent.map(|p| p.get()), Some(80));
        assert_eq!(second.configured_percent.map(|p| p.get()), Some(60));
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
            true, 80, 80, 40, 100, 5,
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
            true, 80, 80, 40, 100, 5,
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
    fn battery_mutation_status_wire_maps_to_capability_status() {
        use orbis_core::capability::CapabilityStatus;
        use orbis_hardwared::battery::battery_mutation_wire;

        assert_eq!(
            battery_mutation_status_from_wire(battery_mutation_wire::SUPPORTED),
            CapabilityStatus::Supported
        );
        assert_eq!(
            battery_mutation_status_from_wire(battery_mutation_wire::UNSUPPORTED),
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            battery_mutation_status_from_wire(battery_mutation_wire::TEMPORARILY_UNAVAILABLE),
            CapabilityStatus::TemporarilyUnavailable
        );
        assert_eq!(
            battery_mutation_status_from_wire(battery_mutation_wire::PERMISSION_DENIED),
            CapabilityStatus::PermissionDenied
        );
        // Explicit unknown evidence and malformed wire both map to Unknown —
        // never to a guessed known state.
        assert_eq!(
            battery_mutation_status_from_wire(battery_mutation_wire::UNKNOWN),
            CapabilityStatus::Unknown
        );
        assert_eq!(
            battery_mutation_status_from_wire(99),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn performance_mutation_status_wire_maps_to_capability_status() {
        use orbis_core::capability::CapabilityStatus;
        use orbis_hardwared::performance_mutation_wire;

        assert_eq!(
            performance_mutation_status_from_wire(performance_mutation_wire::SUPPORTED),
            CapabilityStatus::Supported
        );
        assert_eq!(
            performance_mutation_status_from_wire(performance_mutation_wire::UNSUPPORTED),
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            performance_mutation_status_from_wire(
                performance_mutation_wire::TEMPORARILY_UNAVAILABLE
            ),
            CapabilityStatus::TemporarilyUnavailable
        );
        assert_eq!(
            performance_mutation_status_from_wire(performance_mutation_wire::PERMISSION_DENIED),
            CapabilityStatus::PermissionDenied
        );
        // Explicit unknown evidence and malformed wire both map to Unknown —
        // never to a guessed known state.
        assert_eq!(
            performance_mutation_status_from_wire(performance_mutation_wire::UNKNOWN),
            CapabilityStatus::Unknown
        );
        assert_eq!(
            performance_mutation_status_from_wire(99),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn fan_mutation_status_wire_maps_to_capability_status() {
        use orbis_core::capability::CapabilityStatus;
        use orbis_hardwared::fans::fan_mutation_wire;

        assert_eq!(
            fan_mutation_status_from_wire(fan_mutation_wire::SUPPORTED),
            CapabilityStatus::Supported
        );
        assert_eq!(
            fan_mutation_status_from_wire(fan_mutation_wire::UNSUPPORTED),
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            fan_mutation_status_from_wire(fan_mutation_wire::TEMPORARILY_UNAVAILABLE),
            CapabilityStatus::TemporarilyUnavailable
        );
        assert_eq!(
            fan_mutation_status_from_wire(fan_mutation_wire::PERMISSION_DENIED),
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            fan_mutation_status_from_wire(fan_mutation_wire::BACKEND_MISSING),
            CapabilityStatus::BackendMissing
        );
        // Explicit unknown evidence and malformed wire both map to Unknown —
        // never to a guessed known state.
        assert_eq!(
            fan_mutation_status_from_wire(fan_mutation_wire::UNKNOWN),
            CapabilityStatus::Unknown
        );
        assert_eq!(fan_mutation_status_from_wire(99), CapabilityStatus::Unknown);
    }

    #[test]
    fn maps_wire_unknown_bounds_to_domain() {
        // current percent допустим при bounds=None.
        let info = ChargeLimitInfo::with_percent_unknown_bounds(true, 80, 80);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert!(limit.enabled);
        assert_eq!(limit.configured_percent.map(|p| p.get()), Some(80));
        assert!(limit.bounds.is_none());
    }

    #[test]
    fn rejects_noncanonical_absent_bounds() {
        // bounds_present=false с ненулевыми min/max/step нарушает canonical form.
        let info = ChargeLimitInfo {
            enabled: true,
            configured_percent_present: true,
            configured_percent: 80,
            effective_percent_present: true,
            effective_percent: 80,
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
        let info = ChargeLimitInfo::with_percent_unknown_bounds(true, 60, 60);
        let limit = charge_limit_from_wire(info).expect("valid");
        assert_eq!(limit.configured_percent.map(|p| p.get()), Some(60));
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

    // -----------------------------------------------------------------------
    // Performance wire decode
    // -----------------------------------------------------------------------

    #[test]
    fn performance_current_wire_decode_maps_all_values() {
        use orbis_session_protocol::performance;
        assert_eq!(
            performance_current_from_wire(performance::SILENT).unwrap(),
            PerformanceProfile::Silent
        );
        assert_eq!(
            performance_current_from_wire(performance::BALANCED).unwrap(),
            PerformanceProfile::Balanced
        );
        assert_eq!(
            performance_current_from_wire(performance::TURBO).unwrap(),
            PerformanceProfile::Turbo
        );
    }

    #[test]
    fn performance_current_unknown_wire_is_internal_error() {
        assert!(matches!(
            performance_current_from_wire(7),
            Err(ProviderError::Internal(_))
        ));
    }

    #[test]
    fn performance_mask_wire_decode_maps_all_values() {
        use orbis_session_protocol::performance;
        assert_eq!(
            performance_mask_from_wire(0b111).unwrap(),
            vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ]
        );
        assert_eq!(
            performance_mask_from_wire(performance::BALANCED_BIT | performance::TURBO_BIT).unwrap(),
            vec![PerformanceProfile::Balanced, PerformanceProfile::Turbo]
        );
        assert_eq!(
            performance_mask_from_wire(0).unwrap(),
            Vec::<PerformanceProfile>::new()
        );
    }

    #[test]
    fn performance_mask_unknown_bit_is_internal_error() {
        assert!(matches!(
            performance_mask_from_wire(0b1000),
            Err(ProviderError::Internal(_))
        ));
        assert!(matches!(
            performance_mask_from_wire(0b1111),
            Err(ProviderError::Internal(_))
        ));
    }

    /// Тестовый источник Performance: очередь заранее заданных wire DTO.
    #[derive(Debug, Clone, Copy)]
    enum ScriptedPerfRead {
        Info(PerformanceInfo),
        Dbus,
    }

    struct ScriptedPerfSource {
        results: Mutex<VecDeque<ScriptedPerfRead>>,
        reads: AtomicUsize,
    }

    impl ScriptedPerfSource {
        fn new(reads: Vec<ScriptedPerfRead>) -> Self {
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
    impl SessionPerformanceSource for ScriptedPerfSource {
        async fn read_performance(&self) -> Result<PerformanceInfo, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            match self
                .results
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted performance source: очередь результатов исчерпана")
            {
                ScriptedPerfRead::Info(info) => Ok(info),
                ScriptedPerfRead::Dbus => Err(ProviderError::Dbus("scripted dbus error".into())),
            }
        }
    }

    struct ScriptedHardwareSource {
        result: Mutex<Option<Result<u8, ProviderError>>>,
        requests: Mutex<Vec<u8>>,
    }

    impl ScriptedHardwareSource {
        fn new(result: Result<u8, ProviderError>) -> Self {
            Self {
                result: Mutex::new(Some(result)),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl HardwarePerformanceSource for ScriptedHardwareSource {
        async fn set_performance(&self, profile: u8) -> Result<u8, ProviderError> {
            self.requests.lock().unwrap().push(profile);
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("scripted hardware source: one mutation expected")
        }
    }

    fn perf_info(current: u8, mask: u8) -> PerformanceInfo {
        PerformanceInfo {
            current,
            available_mask: mask,
        }
    }

    #[tokio::test]
    async fn performance_provider_reads_source_once() {
        let source = ScriptedPerfSource::new(vec![ScriptedPerfRead::Info(perf_info(1, 0b111))]);
        let provider = SessionPerformanceProvider::new(source);
        assert_eq!(
            provider.current_profile().await.expect("current"),
            PerformanceProfile::Balanced
        );
        assert_eq!(provider.source.reads(), 1);
    }

    #[tokio::test]
    async fn performance_provider_does_not_cache_wire_state() {
        let source = ScriptedPerfSource::new(vec![
            ScriptedPerfRead::Info(perf_info(performance::SILENT, 0b111)),
            ScriptedPerfRead::Info(perf_info(performance::TURBO, 0b111)),
        ]);
        let provider = SessionPerformanceProvider::new(source);
        let first = provider.current_profile().await.expect("read1");
        let second = provider.current_profile().await.expect("read2");
        assert_eq!(first, PerformanceProfile::Silent);
        assert_eq!(second, PerformanceProfile::Turbo);
        assert_eq!(provider.source.reads(), 2);
    }

    #[tokio::test]
    async fn performance_provider_error_is_preserved() {
        let source = ScriptedPerfSource::new(vec![ScriptedPerfRead::Dbus]);
        let provider = SessionPerformanceProvider::new(source);
        let err = provider.current_profile().await.expect_err("dbus error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn read_only_performance_mutation_is_unsupported() {
        let source = ScriptedPerfSource::new(vec![ScriptedPerfRead::Info(perf_info(1, 0b111))]);
        let provider = SessionPerformanceProvider::new(source);
        assert!(matches!(
            provider.set_profile(PerformanceProfile::Turbo).await,
            Err(ProviderError::Unsupported(_))
        ));
        assert!(matches!(
            provider.validate_set_profile(PerformanceProfile::Turbo),
            ValidationResult::Invalid(_)
        ));
        assert_eq!(provider.source.reads(), 0);
    }

    #[tokio::test]
    async fn composed_provider_writes_direct_hardware_and_confirms_all_profiles() {
        for (profile, wire) in [
            (PerformanceProfile::Silent, performance::SILENT),
            (PerformanceProfile::Balanced, performance::BALANCED),
            (PerformanceProfile::Turbo, performance::TURBO),
        ] {
            let session = ScriptedPerfSource::new(Vec::new());
            let hardware = ScriptedHardwareSource::new(Ok(wire));
            let provider = SessionHardwarePerformanceProvider::new(session, hardware);
            assert_eq!(
                provider.set_profile(profile).await.expect("set"),
                ApplyResult::Applied
            );
            assert_eq!(
                provider.hardware.requests.lock().unwrap().as_slice(),
                &[wire]
            );
        }
    }

    #[tokio::test]
    async fn composed_provider_reads_through_session_source() {
        let session = ScriptedPerfSource::new(vec![
            ScriptedPerfRead::Info(perf_info(performance::TURBO, 0b111)),
            ScriptedPerfRead::Info(perf_info(performance::TURBO, 0b111)),
        ]);
        let hardware = ScriptedHardwareSource::new(Ok(performance::SILENT));
        let provider = SessionHardwarePerformanceProvider::new(session, hardware);

        assert_eq!(
            provider.current_profile().await.expect("current"),
            PerformanceProfile::Turbo
        );
        assert_eq!(
            provider.profiles().await.expect("profiles"),
            vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ]
        );
        assert_eq!(provider.session.reads(), 2);
    }

    #[tokio::test]
    async fn composed_provider_rejects_mismatched_or_unknown_confirmation() {
        let provider = SessionHardwarePerformanceProvider::new(
            ScriptedPerfSource::new(Vec::new()),
            ScriptedHardwareSource::new(Ok(performance::SILENT)),
        );
        assert!(matches!(
            provider.set_profile(PerformanceProfile::Turbo).await,
            Err(ProviderError::Conflict(_))
        ));

        let provider = SessionHardwarePerformanceProvider::new(
            ScriptedPerfSource::new(Vec::new()),
            ScriptedHardwareSource::new(Ok(255)),
        );
        assert!(matches!(
            provider.set_profile(PerformanceProfile::Turbo).await,
            Err(ProviderError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn composed_provider_preserves_hardware_error() {
        let provider = SessionHardwarePerformanceProvider::new(
            ScriptedPerfSource::new(Vec::new()),
            ScriptedHardwareSource::new(Err(ProviderError::PermissionDenied("denied".into()))),
        );
        assert!(matches!(
            provider.set_profile(PerformanceProfile::Turbo).await,
            Err(ProviderError::PermissionDenied(_))
        ));
    }

    #[tokio::test]
    async fn composed_provider_preserves_backend_unavailable_error() {
        let provider = SessionHardwarePerformanceProvider::new(
            ScriptedPerfSource::new(Vec::new()),
            ScriptedHardwareSource::new(Err(ProviderError::BackendUnavailable(
                "hardwared unavailable".into(),
            ))),
        );
        assert!(matches!(
            provider.set_profile(PerformanceProfile::Balanced).await,
            Err(ProviderError::BackendUnavailable(_))
        ));
    }

    #[tokio::test]
    async fn performance_ac_battery_profiles_are_none() {
        let source = ScriptedPerfSource::new(vec![ScriptedPerfRead::Info(perf_info(1, 0b111))]);
        let provider = SessionPerformanceProvider::new(source);
        assert_eq!(provider.profile_on_ac().await.expect("ac"), None);
        assert_eq!(provider.profile_on_battery().await.expect("battery"), None);
    }

    /// Mock read-only FanProvider (delegates nothing; only used for read).
    struct ScriptedFanReadProvider {
        active: FanCurve,
    }

    impl ScriptedFanReadProvider {
        fn new(active: FanCurve) -> Self {
            Self { active }
        }
    }

    impl Provider for ScriptedFanReadProvider {
        fn id(&self) -> &'static str {
            "scripted-fan-read"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-fan-read")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted fan read: функция '{feature}' недоступна")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl FanProvider for ScriptedFanReadProvider {
        async fn fan_ids(&self) -> Result<Vec<FanId>, ProviderError> {
            Ok(vec![FanId::Cpu, FanId::Gpu])
        }

        async fn fan_rpms(&self) -> Result<Vec<(FanId, orbis_core::newtypes::Rpm)>, ProviderError> {
            Err(ProviderError::Unsupported("no rpm".into()))
        }

        async fn fan_curve(
            &self,
            _profile: PerformanceProfile,
            _fan: &FanId,
        ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
            Err(ProviderError::Unsupported("no profile curve".into()))
        }

        async fn fan_curve_for_profile(
            &self,
            _profile: orbis_core::profile::AsusdFanProfile,
            _fan: &FanId,
        ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
            Err(ProviderError::Unsupported("no profile curve".into()))
        }

        async fn active_curve(
            &self,
            _fan: &FanId,
        ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
            Ok(self.active.clone())
        }

        async fn set_fan_curve(
            &self,
            _curve: &orbis_core::fan::FanCurve,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }

        async fn set_curves_to_defaults(
            &self,
            _profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }

        fn curve_point_count(&self) -> usize {
            8
        }

        fn allow_decreasing(&self) -> bool {
            false
        }

        fn validate_curve(&self, curve: &orbis_core::fan::FanCurve) -> ValidationResult {
            match curve.validate(self.curve_point_count(), self.allow_decreasing()) {
                Ok(()) => ValidationResult::ok(),
                Err(e) => ValidationResult::invalid(e.to_string()),
            }
        }
    }

    /// Mock HardwareFanCurveSource: записывает запрос, возвращает scripted результат.
    struct ScriptedFanHardwareSource {
        result: Mutex<Option<Result<u32, ProviderError>>>,
        requests: Mutex<Vec<(u32, u8, FanCurveWire)>>,
    }

    impl ScriptedFanHardwareSource {
        fn new(result: Result<u32, ProviderError>) -> Self {
            Self {
                result: Mutex::new(Some(result)),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl HardwareFanCurveSource for ScriptedFanHardwareSource {
        async fn set_fan_curve(
            &self,
            profile: u32,
            fan: u8,
            curve: FanCurveWire,
        ) -> Result<u32, ProviderError> {
            self.requests.lock().unwrap().push((profile, fan, curve));
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("scripted fan hardware source: one mutation expected")
        }
    }

    fn fan_curve_points() -> FanCurvePoints {
        let mut temps = [TemperatureC::new(0).expect("temp"); 8];
        let mut pwms = [FanPwm::new(0).expect("pwm"); 8];
        for (i, slot) in temps.iter_mut().enumerate() {
            *slot = TemperatureC::new(40 + i as i16 * 10).expect("temp");
        }
        for (i, slot) in pwms.iter_mut().enumerate() {
            *slot = FanPwm::new(20 + i as u8 * 10).expect("pwm");
        }
        FanCurvePoints { temps, pwms }
    }

    #[tokio::test]
    async fn fan_mutation_writes_direct_hardware_and_confirms_profile() {
        let session = ScriptedFanReadProvider::new(active_curve_fixture());
        let hardware = ScriptedFanHardwareSource::new(Ok(AsusdFanProfile::Quiet.wire()));
        let provider = SessionHardwareFanCurveProvider::new(session, hardware);
        let points = fan_curve_points();

        assert_eq!(
            orbis_providers::traits::FanCurveMutationProvider::set_fan_curve(
                &provider,
                AsusdFanProfile::Quiet,
                &FanId::Cpu,
                &points
            )
            .await
            .expect("set"),
            ApplyResult::Applied
        );
        let (profile, fan, curve) = &provider.hardware.requests.lock().unwrap()[0];
        assert_eq!(*profile, AsusdFanProfile::Quiet.wire());
        assert_eq!(*fan, 0);
        assert_eq!(curve.temps.len(), 8);
        assert_eq!(curve.pwms.len(), 8);
        assert_eq!(curve.temps[0], 40);
        assert_eq!(curve.pwms[0], 20);
    }

    #[tokio::test]
    async fn fan_mutation_rejects_mismatched_confirmation() {
        let session = ScriptedFanReadProvider::new(active_curve_fixture());
        let hardware = ScriptedFanHardwareSource::new(Ok(AsusdFanProfile::Balanced.wire()));
        let provider = SessionHardwareFanCurveProvider::new(session, hardware);
        let points = fan_curve_points();

        assert!(matches!(
            orbis_providers::traits::FanCurveMutationProvider::set_fan_curve(
                &provider,
                AsusdFanProfile::Quiet,
                &FanId::Cpu,
                &points
            )
            .await,
            Err(ProviderError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn fan_mutation_rejects_unknown_confirmation_wire() {
        let session = ScriptedFanReadProvider::new(active_curve_fixture());
        let hardware = ScriptedFanHardwareSource::new(Ok(99));
        let provider = SessionHardwareFanCurveProvider::new(session, hardware);
        let points = fan_curve_points();

        // Unknown confirmation from backend is a protocol violation → Internal,
        // not InvalidRequest (which is reserved for user input errors).
        assert!(matches!(
            orbis_providers::traits::FanCurveMutationProvider::set_fan_curve(
                &provider,
                AsusdFanProfile::Quiet,
                &FanId::Cpu,
                &points
            )
            .await,
            Err(ProviderError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn fan_mutation_preserves_hardware_error() {
        let session = ScriptedFanReadProvider::new(active_curve_fixture());
        let hardware =
            ScriptedFanHardwareSource::new(Err(ProviderError::PermissionDenied("denied".into())));
        let provider = SessionHardwareFanCurveProvider::new(session, hardware);
        let points = fan_curve_points();

        assert!(matches!(
            orbis_providers::traits::FanCurveMutationProvider::set_fan_curve(
                &provider,
                AsusdFanProfile::Quiet,
                &FanId::Cpu,
                &points
            )
            .await,
            Err(ProviderError::PermissionDenied(_))
        ));
    }

    #[tokio::test]
    async fn fan_mutation_rejects_non_cpu_gpu_fan() {
        let session = ScriptedFanReadProvider::new(active_curve_fixture());
        let hardware = ScriptedFanHardwareSource::new(Ok(AsusdFanProfile::Quiet.wire()));
        let provider = SessionHardwareFanCurveProvider::new(session, hardware);
        let points = fan_curve_points();

        assert!(matches!(
            orbis_providers::traits::FanCurveMutationProvider::set_fan_curve(
                &provider,
                AsusdFanProfile::Quiet,
                &FanId::Mid,
                &points
            )
            .await,
            Err(ProviderError::InvalidRequest(_))
        ));
        assert!(provider.hardware.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fan_read_delegates_to_session_provider() {
        let session = ScriptedFanReadProvider::new(active_curve_fixture());
        let hardware = ScriptedFanHardwareSource::new(Ok(AsusdFanProfile::Quiet.wire()));
        let provider = SessionHardwareFanCurveProvider::new(session, hardware);

        let curve = provider.active_curve(&FanId::Cpu).await.expect("active");
        assert_eq!(curve.points.len(), 8);
        assert!(matches!(
            orbis_providers::traits::FanProvider::set_fan_curve(&provider, &curve).await,
            Err(ProviderError::Unsupported(_))
        ));
    }

    fn active_curve_fixture() -> FanCurve {
        let mut points = Vec::new();
        for i in 0..8 {
            points.push(FanCurvePoint::new(
                TemperatureC::new(40 + i as i16 * 10).expect("temp"),
                FanPwm::new(20 + i as u8 * 10).expect("pwm"),
            ));
        }
        FanCurve {
            profile: PerformanceProfile::Balanced,
            fan: FanId::Cpu,
            enabled: None,
            points,
        }
    }

    // -----------------------------------------------------------------------
    // Wire→domain fan curve decoding (Session1 read side)
    // -----------------------------------------------------------------------

    fn sentinel_wire(
        profile: u32,
        fan: u8,
        temps: [u8; 8],
        pwms: [u8; 8],
    ) -> orbis_session_protocol::FanCurveInfo {
        sentinel_wire_with_enabled(profile, fan, temps, pwms, true)
    }

    fn sentinel_wire_with_enabled(
        profile: u32,
        fan: u8,
        temps: [u8; 8],
        pwms: [u8; 8],
        enabled: bool,
    ) -> orbis_session_protocol::FanCurveInfo {
        use orbis_session_protocol::FanCurveInfo;
        FanCurveInfo {
            profile,
            fan,
            temps: temps.to_vec(),
            pwms: pwms.to_vec(),
            enabled,
        }
    }

    const CURVE_A_TEMPS: [u8; 8] = [45, 49, 54, 68, 74, 79, 84, 89];
    const CURVE_A_PWMS: [u8; 8] = [5, 22, 38, 45, 56, 63, 81, 94];
    const CURVE_B_TEMPS: [u8; 8] = [40, 44, 50, 60, 70, 76, 82, 90];
    const CURVE_B_PWMS: [u8; 8] = [3, 18, 35, 42, 50, 58, 70, 99];
    const CURVE_C_TEMPS: [u8; 8] = [42, 46, 55, 64, 73, 80, 86, 92];
    const CURVE_C_PWMS: [u8; 8] = [2, 12, 28, 38, 46, 54, 66, 88];

    #[test]
    fn fan_curve_wire_is_lossless_roundtrip() {
        let curve = fan_curve_from_wire(
            sentinel_wire(
                orbis_session_protocol::fan_profile::BALANCED,
                orbis_session_protocol::fan_id::CPU,
                CURVE_B_TEMPS,
                CURVE_B_PWMS,
            ),
            AsusdFanProfile::Balanced,
        )
        .expect("decoded");
        assert_eq!(curve.fan, FanId::Cpu);
        assert_eq!(curve.points.len(), 8);
        assert_eq!(curve.points[0].temp.get(), 40);
        assert_eq!(curve.points[7].pwm.get(), 99);
    }

    #[test]
    fn fan_curve_enabled_state_survives_session1_conversion() {
        let curve = fan_curve_from_wire(
            sentinel_wire_with_enabled(
                orbis_session_protocol::fan_profile::BALANCED,
                orbis_session_protocol::fan_id::GPU,
                CURVE_A_TEMPS,
                CURVE_A_PWMS,
                false,
            ),
            AsusdFanProfile::Balanced,
        )
        .expect("decoded");

        assert_eq!(curve.fan, FanId::Gpu);
        assert_eq!(curve.enabled, Some(false));
    }

    #[test]
    fn fan_curve_wire_preserves_lossless_profile() {
        // Quiet (wire 2) и LowPower (wire 3) различимы — Silent не выводится
        // автоматически ни в один из них.
        let quiet = fan_curve_from_wire(
            sentinel_wire(
                orbis_session_protocol::fan_profile::QUIET,
                orbis_session_protocol::fan_id::CPU,
                CURVE_C_TEMPS,
                CURVE_C_PWMS,
            ),
            AsusdFanProfile::Quiet,
        )
        .expect("quiet decoded");
        assert_eq!(quiet.profile, PerformanceProfile::Silent);

        let low_power = fan_curve_from_wire(
            sentinel_wire(
                orbis_session_protocol::fan_profile::LOW_POWER,
                orbis_session_protocol::fan_id::CPU,
                CURVE_B_TEMPS,
                CURVE_B_PWMS,
            ),
            AsusdFanProfile::LowPower,
        )
        .expect("low power decoded");
        assert_eq!(low_power.profile, PerformanceProfile::Silent);
        // Профиль на wire lossless: запрошенный LowPower не превращается в Quiet.
        assert_eq!(quiet.fan, FanId::Cpu);
    }

    #[test]
    fn fan_curve_wire_rejects_unknown_profile() {
        let err = fan_curve_from_wire(
            sentinel_wire(
                42,
                orbis_session_protocol::fan_id::CPU,
                CURVE_A_TEMPS,
                CURVE_A_PWMS,
            ),
            AsusdFanProfile::Balanced,
        )
        .expect_err("unknown profile wire");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn fan_curve_wire_rejects_profile_mismatch() {
        let err = fan_curve_from_wire(
            sentinel_wire(
                orbis_session_protocol::fan_profile::QUIET,
                orbis_session_protocol::fan_id::CPU,
                CURVE_A_TEMPS,
                CURVE_A_PWMS,
            ),
            AsusdFanProfile::Balanced,
        )
        .expect_err("profile mismatch");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn fan_curve_wire_rejects_unknown_fan() {
        let err = fan_curve_from_wire(
            sentinel_wire(
                orbis_session_protocol::fan_profile::BALANCED,
                7,
                CURVE_A_TEMPS,
                CURVE_A_PWMS,
            ),
            AsusdFanProfile::Balanced,
        )
        .expect_err("unknown fan wire");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn fan_curve_wire_rejects_wrong_point_count() {
        use orbis_session_protocol::FanCurveInfo;
        let err = fan_curve_from_wire(
            FanCurveInfo {
                profile: orbis_session_protocol::fan_profile::BALANCED,
                fan: orbis_session_protocol::fan_id::CPU,
                temps: vec![45, 49],
                pwms: vec![5, 22],
                enabled: true,
            },
            AsusdFanProfile::Balanced,
        )
        .expect_err("wrong point count");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn fan_curve_wire_rejects_unknown_profile_probe() {
        let err = fan_profile_from_wire_protocol(9);
        assert!(matches!(err, Err(ProviderError::Internal(_))));
        let err = fan_id_from_wire_protocol(9);
        assert!(matches!(err, Err(ProviderError::Internal(_))));
    }

    // -----------------------------------------------------------------------
    // Profile-specific read через Session1-composed provider
    // -----------------------------------------------------------------------

    /// Scripted Session1 profile source: returns sentinel curves per profile.
    struct ScriptedSessionFanSource {
        cpu_by_profile:
            std::collections::HashMap<AsusdFanProfile, orbis_session_protocol::FanCurveInfo>,
        reads: AtomicUsize,
    }

    impl ScriptedSessionFanSource {
        fn new(
            cpu_by_profile: std::collections::HashMap<
                AsusdFanProfile,
                orbis_session_protocol::FanCurveInfo,
            >,
        ) -> Self {
            Self {
                cpu_by_profile,
                reads: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl SessionFanCurveSource for ScriptedSessionFanSource {
        async fn read_fan_curve(
            &self,
            profile: u32,
            fan: u8,
        ) -> Result<orbis_session_protocol::FanCurveInfo, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let profile = super::fan_profile_from_wire_protocol(profile)?;
            // CPU/GPU не смешиваются: для GPU возвращаем явно отличный sentinel.
            if fan == orbis_session_protocol::fan_id::GPU {
                return Ok(sentinel_wire(
                    profile.wire(),
                    orbis_session_protocol::fan_id::GPU,
                    [30, 33, 36, 40, 45, 50, 55, 60],
                    [0, 0, 0, 0, 0, 0, 0, 0],
                ));
            }
            self.cpu_by_profile
                .get(&profile)
                .cloned()
                .ok_or_else(|| ProviderError::Unsupported("sentinel profile недоступен".into()))
        }
    }

    /// Scripted sysfs active provider: returns curve A for CPU.
    struct ScriptedActiveFanProvider;

    impl Provider for ScriptedActiveFanProvider {
        fn id(&self) -> &'static str {
            "scripted-active-fan"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-active-fan")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-active-fan: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl FanProvider for ScriptedActiveFanProvider {
        async fn fan_ids(&self) -> Result<Vec<FanId>, ProviderError> {
            Ok(vec![FanId::Cpu, FanId::Gpu])
        }
        async fn fan_rpms(&self) -> Result<Vec<(FanId, orbis_core::newtypes::Rpm)>, ProviderError> {
            Err(ProviderError::Unsupported("no rpms".into()))
        }
        async fn fan_curve(
            &self,
            _profile: PerformanceProfile,
            _fan: &FanId,
        ) -> Result<FanCurve, ProviderError> {
            Err(ProviderError::Unsupported(
                "profile via sysfs не предоставляется".into(),
            ))
        }
        async fn fan_curve_for_profile(
            &self,
            _profile: AsusdFanProfile,
            _fan: &FanId,
        ) -> Result<FanCurve, ProviderError> {
            Err(ProviderError::Unsupported(
                "active source не предоставляет profile curves".into(),
            ))
        }
        async fn active_curve(&self, fan: &FanId) -> Result<FanCurve, ProviderError> {
            assert_eq!(fan, &FanId::Cpu);
            let mut points = Vec::new();
            for i in 0..8 {
                points.push(FanCurvePoint::new(
                    TemperatureC::new(CURVE_A_TEMPS[i] as i16).expect("temp"),
                    FanPwm::new(CURVE_A_PWMS[i]).expect("pwm"),
                ));
            }
            Ok(FanCurve {
                profile: PerformanceProfile::Balanced,
                fan: FanId::Cpu,
                enabled: None,
                points,
            })
        }
        async fn set_fan_curve(&self, _curve: &FanCurve) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }
        async fn set_curves_to_defaults(
            &self,
            _profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }
        fn curve_point_count(&self) -> usize {
            8
        }
        fn allow_decreasing(&self) -> bool {
            false
        }
        fn validate_curve(&self, _curve: &FanCurve) -> ValidationResult {
            ValidationResult::ok()
        }
    }

    fn sentinel_map()
    -> std::collections::HashMap<AsusdFanProfile, orbis_session_protocol::FanCurveInfo> {
        let mut map = std::collections::HashMap::new();
        map.insert(
            AsusdFanProfile::Balanced,
            sentinel_wire(
                orbis_session_protocol::fan_profile::BALANCED,
                orbis_session_protocol::fan_id::CPU,
                CURVE_B_TEMPS,
                CURVE_B_PWMS,
            ),
        );
        map.insert(
            AsusdFanProfile::Quiet,
            sentinel_wire(
                orbis_session_protocol::fan_profile::QUIET,
                orbis_session_protocol::fan_id::CPU,
                CURVE_C_TEMPS,
                CURVE_C_PWMS,
            ),
        );
        map
    }

    #[tokio::test]
    async fn listener_sentinel_regression_capability_probe_sees_active_curve_a() {
        // 1. capability probe (active_curve) видит активную sysfs кривую A.
        let session = ScriptedSessionFanSource::new(sentinel_map());
        let composed = SessionProfileFanCurveProvider::new(session, ScriptedActiveFanProvider);

        let probe = composed.active_curve(&FanId::Cpu).await.expect("active");
        assert_eq!(probe.points[0].temp.get(), CURVE_A_TEMPS[0] as i16);
        assert_eq!(probe.points[7].pwm.get(), CURVE_A_PWMS[7]);
    }

    #[tokio::test]
    async fn listener_sentinel_regression_refresh_balanced_cpu_returns_b() {
        // 2. RefreshFanCurve(Balanced, CPU) → кривая B (sessiond → asusd).
        let session = ScriptedSessionFanSource::new(sentinel_map());
        let composed = SessionProfileFanCurveProvider::new(session, ScriptedActiveFanProvider);

        let curve = composed
            .fan_curve_for_profile(AsusdFanProfile::Balanced, &FanId::Cpu)
            .await
            .expect("balanced");
        assert_eq!(curve.points[0].temp.get(), CURVE_B_TEMPS[0] as i16);
        assert_eq!(curve.points[7].pwm.get(), CURVE_B_PWMS[7]);
    }

    #[tokio::test]
    async fn listener_sentinel_regression_refresh_quiet_cpu_returns_c() {
        // 3. RefreshFanCurve(Quiet, CPU) → C.
        let session = ScriptedSessionFanSource::new(sentinel_map());
        let composed = SessionProfileFanCurveProvider::new(session, ScriptedActiveFanProvider);

        let curve = composed
            .fan_curve_for_profile(AsusdFanProfile::Quiet, &FanId::Cpu)
            .await
            .expect("quiet");
        assert_eq!(curve.points[0].temp.get(), CURVE_C_TEMPS[0] as i16);
        assert_eq!(curve.points[7].pwm.get(), CURVE_C_PWMS[7]);
    }

    #[tokio::test]
    async fn listener_sentinel_regression_cpu_gpu_not_mixed() {
        // 4. CPU/GPU не смешиваются: GPU read возвращает только GPU curve
        // (distinct sentinel), не CPU-кривую.
        let session = ScriptedSessionFanSource::new(sentinel_map());
        let composed = SessionProfileFanCurveProvider::new(session, ScriptedActiveFanProvider);

        let gpu = composed
            .fan_curve_for_profile(AsusdFanProfile::Balanced, &FanId::Gpu)
            .await
            .expect("gpu");
        assert_eq!(gpu.fan, FanId::Gpu);
        assert_eq!(gpu.points[0].temp.get(), 30);
        assert_eq!(gpu.points[7].pwm.get(), 0);
    }
}
