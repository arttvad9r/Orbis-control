//! Read-only Battery adapter over UPower, asusd and kernel sysfs.
//!
//! Current battery state comes from UPower. Charge-limit configured/effective
//! state is accepted only when asusd and kernel sysfs agree; UPower's threshold
//! property remains diagnostic evidence and does not block that consensus.
//!
//! - `Connection` и object path батареи передаются извне; adapter сам не
//!   открывает system/session bus и не создаёт service;
//! - mutation-методы `BatteryProvider` возвращают `ProviderError::Unsupported`
//!   и не выполняют I/O;
//! - каждый вызов `charge_limit()` выполняет новое authoritative чтение source
//!   (кэш отсутствует).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::{
    BatteryThresholdConfidence, BatteryThresholdEvidence, BatteryThresholdFreshness,
    BatteryThresholdObservation, BatteryThresholdSource, ChargeLimit,
};
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

/// Read-only configured threshold source owned by asusd.
#[async_trait]
pub trait AsusdConfiguredSource: Send + Sync {
    /// Прочитать сохранённый configured threshold свежим getter-вызовом.
    async fn read_configured_threshold(&self) -> Result<u8, ProviderError>;
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
        if !snapshot.supported {
            return Ok(snapshot);
        }
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

/// Production configured source: UPower supplies battery state, while asusd
/// and kernel sysfs own the configured/effective threshold consensus.
pub struct AsusdBatteryChargeLimitProvider<U, A, E> {
    upower: U,
    asusd: A,
    effective: E,
}

impl<U, A, E> AsusdBatteryChargeLimitProvider<U, A, E> {
    /// Создать provider с раздельными authoritative sources.
    pub fn new(upower: U, asusd: A, effective: E) -> Self {
        Self {
            upower,
            asusd,
            effective,
        }
    }
}

impl<U, A, E> Provider for AsusdBatteryChargeLimitProvider<U, A, E>
where
    U: Send + Sync,
    A: Send + Sync,
    E: Send + Sync,
{
    fn id(&self) -> &'static str {
        "asusd-charge-limit"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asusd-charge-limit")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("asusd charge-limit backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asusd-charge-limit",
            "read-only UPower enabled + asusd configured + kernel effective battery backend",
        )]
    }
}

#[async_trait]
impl<U, A, E> BatteryProvider for AsusdBatteryChargeLimitProvider<U, A, E>
where
    U: UPowerChargeLimitSource,
    A: AsusdConfiguredSource,
    E: BatteryEffectiveSource,
{
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        let snapshot = self.upower.read_charge_limit().await?;
        if !snapshot.supported {
            return Err(ProviderError::Unsupported(
                "UPower: charge threshold не поддерживается устройством".into(),
            ));
        }
        let configured =
            Percent::new(self.asusd.read_configured_threshold().await?).map_err(|e| {
                ProviderError::Internal(format!("asusd: невалидный configured threshold: {e}"))
            })?;
        let upower_reported = u8::try_from(snapshot.end_threshold).map_err(|_| {
            ProviderError::Internal(format!(
                "UPower: end_threshold вне u8 диапазона: {}",
                snapshot.end_threshold
            ))
        })?;
        let effective = Percent::new(self.effective.read_effective_end_threshold().await?)
            .map_err(|e| {
                ProviderError::Internal(format!("kernel: невалидный effective threshold: {e}"))
            })?;
        if configured != effective {
            return Err(ProviderError::Conflict(format!(
                "battery thresholds disagree: ASUS={configured}%, sysfs={effective}% (UPower reports {upower_reported}% as diagnostic evidence)"
            )));
        }
        ChargeLimit::new(snapshot.enabled, Some(configured), Some(effective), None)
            .map_err(|e| ProviderError::Internal(format!("Battery threshold невалиден: {e}")))
    }

    async fn threshold_evidence(&self) -> Result<BatteryThresholdEvidence, ProviderError> {
        let snapshot = self.upower.read_charge_limit().await?;
        if !snapshot.supported {
            return Err(ProviderError::Unsupported(
                "UPower: charge threshold не поддерживается устройством".into(),
            ));
        }
        let asus = Percent::new(self.asusd.read_configured_threshold().await?).map_err(|e| {
            ProviderError::Internal(format!("asusd: невалидный configured threshold: {e}"))
        })?;
        let sysfs =
            Percent::new(self.effective.read_effective_end_threshold().await?).map_err(|e| {
                ProviderError::Internal(format!("kernel: невалидный effective threshold: {e}"))
            })?;
        let observed_at = SystemTime::now();
        Ok(BatteryThresholdEvidence::from_observations(vec![
            BatteryThresholdObservation {
                source: BatteryThresholdSource::AsusBackend,
                value: asus,
                observed_at,
                freshness: BatteryThresholdFreshness::Fresh,
                confidence: BatteryThresholdConfidence::High,
            },
            BatteryThresholdObservation {
                source: BatteryThresholdSource::Sysfs,
                value: sysfs,
                observed_at,
                freshness: BatteryThresholdFreshness::Fresh,
                confidence: BatteryThresholdConfidence::High,
            },
        ]))
    }

    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "sessiond Battery provider is read-only".into(),
        ))
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "sessiond Battery provider is read-only".into(),
        ))
    }

    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid("sessiond Battery provider is read-only")
    }
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

#[zbus::proxy(
    interface = "xyz.ljones.Platform",
    default_service = "xyz.ljones.Asusd",
    default_path = "/xyz/ljones"
)]
trait AsusdPlatformReadOnly {
    #[zbus(property)]
    fn charge_control_end_threshold(&self) -> zbus::Result<u8>;
}

/// Typed read-only client for the asusd configured threshold.
pub struct ZbusAsusdConfiguredSource {
    connection: zbus::Connection,
}

impl ZbusAsusdConfiguredSource {
    /// Создать client без D-Bus I/O.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl AsusdConfiguredSource for ZbusAsusdConfiguredSource {
    async fn read_configured_threshold(&self) -> Result<u8, ProviderError> {
        AsusdPlatformReadOnlyProxy::builder(&self.connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| ProviderError::Dbus(format!("asusd proxy: {e}")))?
            .charge_control_end_threshold()
            .await
            .map_err(|e| ProviderError::Dbus(format!("asusd configured read: {e}")))
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

// ---------------------------------------------------------------------------
// Lazy discovery Battery provider (UPower resilience)
// ---------------------------------------------------------------------------

/// Testable источник discovery системной батареи.
///
/// Отдельный trait от `crate::discovery::discover_battery` позволяет
/// инжектировать scripted discovery в unit/P2P тестах без реального UPower.
#[async_trait]
pub trait BatteryDiscoverySource: Send + Sync {
    /// Обнаружить системную батарею (authoritative, без кэша).
    async fn discover(&self) -> Result<crate::discovery::DiscoveredBattery, ProviderError>;
}

/// Реальный zbus discovery через UPower (`org.freedesktop.UPower`).
///
/// Хранит готовую `Connection`; I/O начинается только в `discover().await`.
pub struct ZbusBatteryDiscoverySource {
    connection: zbus::Connection,
}

impl ZbusBatteryDiscoverySource {
    /// Создать источник с готовой C-connection.
    ///
    /// Конструктор не выполняет I/O, не открывает bus, не проверяет service.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl BatteryDiscoverySource for ZbusBatteryDiscoverySource {
    async fn discover(&self) -> Result<crate::discovery::DiscoveredBattery, ProviderError> {
        crate::discovery::discover_battery(&self.connection).await
    }
}

/// Testable фабрика read-chain по обнаруженной батарее.
///
/// Отдельный trait позволяет инжектировать scripted read в unit/P2P-тестах
/// без реального UPower/asusd/sysfs.
#[async_trait]
pub trait BatteryReadFactory: Send + Sync {
    /// Построить read-only `BatteryProvider` по результату discovery.
    async fn build(
        &self,
        battery: &crate::discovery::DiscoveredBattery,
    ) -> Result<Arc<dyn BatteryProvider>, ProviderError>;
}

/// Production фабрика: UPower device + asusd configured + kernel effective
/// (тот же состав, что `build_upower_session_server_with_effective_source`).
pub struct AsusdBatteryReadFactory {
    connection: zbus::Connection,
}

impl AsusdBatteryReadFactory {
    /// Создать фабрику без I/O.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl BatteryReadFactory for AsusdBatteryReadFactory {
    async fn build(
        &self,
        battery: &crate::discovery::DiscoveredBattery,
    ) -> Result<Arc<dyn BatteryProvider>, ProviderError> {
        let effective = SysfsBatteryEndThresholdSource::from_native_path(&battery.native_path)?;
        let upower_source =
            ZbusUPowerChargeLimitSource::new(self.connection.clone(), battery.object_path.clone());
        let asusd_source = ZbusAsusdConfiguredSource::new(self.connection.clone());
        Ok(Arc::new(AsusdBatteryChargeLimitProvider::new(
            upower_source,
            asusd_source,
            effective,
        )))
    }
}

/// Lazy Battery Charge Limit provider: discovery выполняется при каждом read.
///
/// Создан для UPower resilience:
/// - Session1 стартует даже если UPower service / battery object недоступны
///   при startup (startup discovery отсутствует);
/// - каждый `charge_limit()` выполняет новый discovery + read; transient
///   failure возвращается честно и **не кэшируется** (следующий read повторяет
///   discovery без restart sessiond);
/// - если UPower/battery появляется позже или UPower перезапускается —
///   следующий read снова обнаружит батарею;
/// - реально неподдерживаемая battery capability → `Unsupported`
///   (непревращаемое в transient), permission → `PermissionDenied`,
///   transient/unavailable → `Dbus`/`BackendUnavailable`, malformed → closed;
/// - никаких synthetic/default charge limits.
pub struct LazyBatteryChargeLimitProvider<D, F> {
    discovery: D,
    read_factory: F,
}

impl<D, F> LazyBatteryChargeLimitProvider<D, F> {
    /// Создать provider без I/O.
    pub fn new(discovery: D, read_factory: F) -> Self {
        Self {
            discovery,
            read_factory,
        }
    }
}

impl<D, F> Provider for LazyBatteryChargeLimitProvider<D, F>
where
    D: BatteryDiscoverySource,
    F: BatteryReadFactory,
{
    fn id(&self) -> &'static str {
        "lazy-upower-charge-limit"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("lazy-upower-charge-limit")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("lazy UPower charge limit backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.lazy-upower-charge-limit",
            "lazy read-only UPower charge limit backend (retry discovery per read)",
        )]
    }
}

#[async_trait]
impl<D, F> BatteryProvider for LazyBatteryChargeLimitProvider<D, F>
where
    D: BatteryDiscoverySource,
    F: BatteryReadFactory,
{
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        let battery = self.discovery.discover().await?;
        let provider = self.read_factory.build(&battery).await?;
        provider.charge_limit().await
    }

    async fn threshold_evidence(&self) -> Result<BatteryThresholdEvidence, ProviderError> {
        let battery = self.discovery.discover().await?;
        let provider = self.read_factory.build(&battery).await?;
        provider.threshold_evidence().await
    }

    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "lazy-upower: set_charge_limit недоступна (read-only)".into(),
        ))
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "lazy-upower: one_shot_full_charge недоступна (read-only)".into(),
        ))
    }

    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid("read-only backend: запись charge limit не поддерживается")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use orbis_core::battery::BatteryThresholdEvidenceState;
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

    struct ScriptedAsusdSource {
        values: std::sync::Mutex<Vec<Result<u8, ProviderError>>>,
    }

    #[async_trait]
    impl AsusdConfiguredSource for ScriptedAsusdSource {
        async fn read_configured_threshold(&self) -> Result<u8, ProviderError> {
            self.values.lock().unwrap().remove(0)
        }
    }

    struct ScriptedEffectiveSource {
        values: std::sync::Mutex<Vec<u8>>,
    }

    #[async_trait]
    impl BatteryEffectiveSource for ScriptedEffectiveSource {
        async fn read_effective_end_threshold(&self) -> Result<u8, ProviderError> {
            Ok(self.values.lock().unwrap().remove(0))
        }
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

    fn asusd_provider(
        upower: UPowerChargeLimitSnapshot,
        configured: Vec<Result<u8, ProviderError>>,
        effective: u8,
    ) -> AsusdBatteryChargeLimitProvider<ScriptedSource, ScriptedAsusdSource, ScriptedEffectiveSource>
    {
        AsusdBatteryChargeLimitProvider::new(
            ScriptedSource::new(ScriptedOutcome::Snapshot(upower)),
            ScriptedAsusdSource {
                values: std::sync::Mutex::new(configured),
            },
            ScriptedEffectiveSource {
                values: std::sync::Mutex::new(vec![effective]),
            },
        )
    }

    #[tokio::test]
    async fn matching_asus_and_sysfs_thresholds_override_stale_upower_threshold() {
        let p = asusd_provider(
            UPowerChargeLimitSnapshot {
                supported: true,
                enabled: false,
                end_threshold: 80,
                effective_end_threshold: None,
            },
            vec![Ok(100)],
            100,
        );
        let limit = p.charge_limit().await.expect("ASUS and sysfs agree");
        assert_eq!(limit.configured_percent.map(|value| value.get()), Some(100));
        assert_eq!(limit.effective_percent.map(|value| value.get()), Some(100));
    }

    #[tokio::test]
    async fn threshold_evidence_retains_all_conflicting_sources() {
        let provider = asusd_provider(
            UPowerChargeLimitSnapshot {
                supported: true,
                enabled: true,
                end_threshold: 80,
                effective_end_threshold: None,
            },
            vec![Ok(100)],
            100,
        );

        let evidence = provider.threshold_evidence().await.expect("evidence");
        assert_eq!(evidence.state, BatteryThresholdEvidenceState::Confirmed);
        assert_eq!(evidence.observations.len(), 2);
        assert_eq!(
            evidence.observations[0].source,
            BatteryThresholdSource::AsusBackend
        );
        assert_eq!(evidence.observations[0].value.get(), 100);
        assert_eq!(
            evidence.observations[1].source,
            BatteryThresholdSource::Sysfs
        );
        assert_eq!(evidence.observations[1].value.get(), 100);
    }

    #[tokio::test]
    async fn asusd_configured_value_supports_mutation_and_rollback_reads() {
        let p = AsusdBatteryChargeLimitProvider::new(
            ScriptedSource::new(ScriptedOutcome::Snapshot(UPowerChargeLimitSnapshot {
                supported: true,
                enabled: true,
                end_threshold: 80,
                effective_end_threshold: None,
            })),
            ScriptedAsusdSource {
                values: std::sync::Mutex::new(vec![Ok(80), Ok(80)]),
            },
            ScriptedEffectiveSource {
                values: std::sync::Mutex::new(vec![80, 80]),
            },
        );
        let first = p.charge_limit().await.expect("80 read");
        assert_eq!(first.configured_percent.map(|x| x.get()), Some(80));
        assert_eq!(first.effective_percent.map(|x| x.get()), Some(80));
        let second = p.charge_limit().await.expect("100 read");
        assert_eq!(second.configured_percent.map(|x| x.get()), Some(80));
        assert_eq!(second.effective_percent.map(|x| x.get()), Some(80));
        assert!(second.enabled);
    }

    #[tokio::test]
    async fn asusd_configured_read_error_does_not_fallback_to_upower() {
        let p = asusd_provider(
            UPowerChargeLimitSnapshot {
                supported: true,
                enabled: false,
                end_threshold: 80,
                effective_end_threshold: None,
            },
            vec![Err(ProviderError::Dbus("asusd unavailable".into()))],
            100,
        );
        assert!(matches!(
            p.charge_limit().await,
            Err(ProviderError::Dbus(message)) if message == "asusd unavailable"
        ));
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

    // -----------------------------------------------------------------------
    // Lazy discovery Battery provider (UPower resilience)
    // -----------------------------------------------------------------------

    /// Тестовый discovery source: очередь заранее заданных результатов.
    struct ScriptedDiscovery {
        outcomes: std::sync::Mutex<
            std::collections::VecDeque<Result<crate::discovery::DiscoveredBattery, ProviderError>>,
        >,
        calls: AtomicUsize,
    }

    impl ScriptedDiscovery {
        fn new(outcomes: Vec<Result<crate::discovery::DiscoveredBattery, ProviderError>>) -> Self {
            Self {
                outcomes: std::sync::Mutex::new(outcomes.into()),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl BatteryDiscoverySource for ScriptedDiscovery {
        async fn discover(&self) -> Result<crate::discovery::DiscoveredBattery, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted discovery: очередь результатов исчерпана")
        }
    }

    #[async_trait]
    impl BatteryDiscoverySource for Arc<ScriptedDiscovery> {
        async fn discover(&self) -> Result<crate::discovery::DiscoveredBattery, ProviderError> {
            (**self).discover().await
        }
    }

    fn discovered_battery() -> crate::discovery::DiscoveredBattery {
        crate::discovery::DiscoveredBattery {
            object_path: "/org/freedesktop/UPower/devices/battery_BAT1"
                .try_into()
                .expect("valid path"),
            native_path: "BAT1".into(),
        }
    }

    /// Тестовая read-фабрика: возвращает scripted provider либо ошибку.
    struct ScriptedReadFactory {
        outcomes: std::sync::Mutex<std::collections::VecDeque<Result<ChargeLimit, ProviderError>>>,
    }

    impl ScriptedReadFactory {
        fn new(outcomes: Vec<Result<ChargeLimit, ProviderError>>) -> Self {
            Self {
                outcomes: std::sync::Mutex::new(outcomes.into()),
            }
        }
    }

    #[async_trait]
    impl BatteryReadFactory for ScriptedReadFactory {
        async fn build(
            &self,
            _battery: &crate::discovery::DiscoveredBattery,
        ) -> Result<Arc<dyn BatteryProvider>, ProviderError> {
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted read factory: очередь результатов исчерпана");
            Ok(Arc::new(ScriptedReadProvider { outcome }))
        }
    }

    /// Тестовый read-only BatteryProvider поверх готового результата.
    struct ScriptedReadProvider {
        outcome: Result<ChargeLimit, ProviderError>,
    }

    impl Provider for ScriptedReadProvider {
        fn id(&self) -> &'static str {
            "scripted-read"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-read")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-read: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl BatteryProvider for ScriptedReadProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            match &self.outcome {
                Ok(limit) => Ok(*limit),
                Err(e) => Err(match e {
                    ProviderError::Unsupported(m) => ProviderError::Unsupported(m.clone()),
                    ProviderError::PermissionDenied(m) => {
                        ProviderError::PermissionDenied(m.clone())
                    }
                    ProviderError::InvalidRequest(m) => ProviderError::InvalidRequest(m.clone()),
                    ProviderError::BackendUnavailable(m) => {
                        ProviderError::BackendUnavailable(m.clone())
                    }
                    ProviderError::Timeout(m) => ProviderError::Timeout(m.clone()),
                    ProviderError::Dbus(m) => ProviderError::Dbus(m.clone()),
                    ProviderError::Internal(m) => ProviderError::Internal(m.clone()),
                    ProviderError::Conflict(m) => ProviderError::Conflict(m.clone()),
                    ProviderError::Io(e) => ProviderError::Io(std::io::Error::other(e.to_string())),
                }),
            }
        }
        async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("scripted: read-only".into()))
        }
        async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("scripted: read-only".into()))
        }
        fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
            ValidationResult::invalid("scripted: read-only")
        }
    }

    fn scripted_limit(percent: u8) -> ChargeLimit {
        ChargeLimit::new(
            true,
            Some(Percent::new(percent).expect("percent")),
            Some(Percent::new(percent).expect("percent")),
            None,
        )
        .expect("valid")
    }

    fn lazy_provider(
        discovery: ScriptedDiscovery,
        read_factory: ScriptedReadFactory,
    ) -> LazyBatteryChargeLimitProvider<Arc<ScriptedDiscovery>, ScriptedReadFactory> {
        LazyBatteryChargeLimitProvider::new(Arc::new(discovery), read_factory)
    }

    #[tokio::test]
    async fn lazy_discovery_transient_error_is_preserved() {
        // При недоступности UPower первый read возвращает честную
        // ошибку (не кэшируется, не подставляется synthetic).
        let discovery = ScriptedDiscovery::new(vec![Err(ProviderError::Dbus(
            "UPower service unavailable".into(),
        ))]);
        let provider = lazy_provider(discovery, ScriptedReadFactory::new(vec![]));
        let err = provider.charge_limit().await.expect_err("transient");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn lazy_discovery_unsupported_stays_unsupported_not_transient() {
        // Реально неподдерживаемая battery capability → Unsupported,
        // НЕ преобразуется в transient unavailable.
        let discovery =
            ScriptedDiscovery::new(vec![Err(ProviderError::Unsupported("no battery".into()))]);
        let provider = lazy_provider(discovery, ScriptedReadFactory::new(vec![]));
        let err = provider.charge_limit().await.expect_err("unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));
    }

    #[tokio::test]
    async fn lazy_discovery_permission_denied_is_preserved() {
        let discovery =
            ScriptedDiscovery::new(vec![Err(ProviderError::PermissionDenied("denied".into()))]);
        let provider = lazy_provider(discovery, ScriptedReadFactory::new(vec![]));
        let err = provider
            .charge_limit()
            .await
            .expect_err("permission denied");
        assert!(matches!(err, ProviderError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn lazy_rediscovers_after_later_availability() {
        // Первый read → transient (не кэшируется), второй read снова
        // выполняет discovery и при появившейся батарее читает успешно.
        let discovery = ScriptedDiscovery::new(vec![
            Err(ProviderError::Dbus("not ready".into())),
            Ok(discovered_battery()),
        ]);
        let provider = lazy_provider(
            discovery,
            ScriptedReadFactory::new(vec![Ok(scripted_limit(60))]),
        );

        let err = provider.charge_limit().await.expect_err("transient");
        assert!(matches!(err, ProviderError::Dbus(_)));

        let limit = provider.charge_limit().await.expect("later availability");
        assert_eq!(limit.configured_percent.map(|p| p.get()), Some(60));
    }

    #[tokio::test]
    async fn lazy_mutations_unsupported_no_io() {
        let discovery = Arc::new(ScriptedDiscovery::new(vec![Err(
            ProviderError::Unsupported("no battery".into()),
        )]));
        let provider: LazyBatteryChargeLimitProvider<Arc<ScriptedDiscovery>, ScriptedReadFactory> =
            LazyBatteryChargeLimitProvider::new(discovery.clone(), ScriptedReadFactory::new(vec![]));
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
        assert_eq!(discovery.calls(), 0);
    }
}
