//! D-Bus service object для session интерфейса.
//!
//! Серверная сторона интерфейса `io.github.orbiscontrol.Session1`: read-only
//! properties (`ChargeLimit`, GPU capabilities, `Performance`). Этот модуль
//! только определяет service object и
//! domain-to-wire conversion; он не создаёт Connection, ObjectServer, runtime и
//! не регистрирует bus name — bus bootstrap выполняется отдельным микрошагом.

use std::sync::Arc;

use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};
use orbis_session_protocol::{ChargeLimitInfo, gpu_access, gpu_mux, gpu_power, performance};

/// Service object session интерфейса.
///
/// Владеет независимыми capability providers: battery + опциональные GPU
/// capabilities (power / MUX / access) + опциональный Performance Mode.
/// Провайдеры передаются извне. Конструктор не выполняет I/O, не читает
/// состояние, не открывает D-Bus и не создаёт runtime; кэш/last error/mutable
/// state отсутствуют.
pub struct SessionService {
    battery: Arc<dyn BatteryProvider>,
    gpu_power: Option<Arc<dyn GpuPowerProvider>>,
    gpu_mux: Option<Arc<dyn GpuMuxProvider>>,
    gpu_access: Option<Arc<dyn GpuAccessProvider>>,
    performance: Option<Arc<dyn PerformanceProvider>>,
}

impl SessionService {
    /// Создать service object над battery provider.
    pub fn new(battery: Arc<dyn BatteryProvider>) -> Self {
        Self {
            battery,
            gpu_power: None,
            gpu_mux: None,
            gpu_access: None,
            performance: None,
        }
    }

    /// Добавить read-only GPU power capability provider.
    pub fn with_gpu_power(mut self, provider: Arc<dyn GpuPowerProvider>) -> Self {
        self.gpu_power = Some(provider);
        self
    }

    /// Добавить read-only GPU MUX capability provider.
    pub fn with_gpu_mux(mut self, provider: Arc<dyn GpuMuxProvider>) -> Self {
        self.gpu_mux = Some(provider);
        self
    }

    /// Добавить read-only GPU access capability provider.
    pub fn with_gpu_access(mut self, provider: Arc<dyn GpuAccessProvider>) -> Self {
        self.gpu_access = Some(provider);
        self
    }

    /// Добавить read-only Performance Mode capability provider.
    pub fn with_performance(mut self, provider: Arc<dyn PerformanceProvider>) -> Self {
        self.performance = Some(provider);
        self
    }

    /// Прочитать authoritative domain Charge Limit и вернуть wire DTO.
    ///
    /// Provider вызывается ровно один раз; каждое чтение — новое; кэш
    /// отсутствует; `ProviderError` сохраняется без преобразования внутри
    /// helper; panic/retry/fallback отсутствуют.
    pub async fn read_charge_limit(&self) -> Result<ChargeLimitInfo, ProviderError> {
        let value = self.battery.charge_limit().await?;
        Ok(charge_limit_to_wire(value))
    }

    /// Прочитать authoritative dGPU power state (domain).
    pub async fn read_gpu_power(&self) -> Result<GpuPowerState, ProviderError> {
        let provider = self.gpu_power.as_ref().ok_or_else(|| {
            ProviderError::Unsupported("session: GPU power capability недоступна".into())
        })?;
        provider.power_state().await
    }

    /// Прочитать authoritative physical MUX state (domain).
    pub async fn read_gpu_mux(&self) -> Result<GpuMuxState, ProviderError> {
        let provider = self.gpu_mux.as_ref().ok_or_else(|| {
            ProviderError::Unsupported("session: GPU MUX capability недоступна".into())
        })?;
        provider.mux_state().await
    }

    /// Прочитать authoritative dGPU access policy (domain).
    pub async fn read_gpu_access(&self) -> Result<GpuAccessPolicy, ProviderError> {
        let provider = self.gpu_access.as_ref().ok_or_else(|| {
            ProviderError::Unsupported("session: GPU access capability недоступна".into())
        })?;
        provider.access_policy().await
    }

    /// Прочитать authoritative Performance Mode state (domain current + available).
    pub async fn read_performance(
        &self,
    ) -> Result<(PerformanceProfile, Vec<PerformanceProfile>), ProviderError> {
        let provider = self.performance.as_ref().ok_or_else(|| {
            ProviderError::Unsupported("session: Performance capability недоступна".into())
        })?;
        let current = provider.current_profile().await?;
        let available = provider.profiles().await?;
        Ok((current, available))
    }
}

/// Преобразовать domain `GpuPowerState` в canonical wire `u8`.
fn gpu_power_to_wire(value: GpuPowerState) -> u8 {
    match value {
        GpuPowerState::Active => gpu_power::ACTIVE,
        GpuPowerState::Suspended => gpu_power::SUSPENDED,
        GpuPowerState::Off => gpu_power::OFF,
        GpuPowerState::Stale => gpu_power::STALE,
        GpuPowerState::Unknown => gpu_power::UNKNOWN,
    }
}

/// Преобразовать domain `GpuMuxState` в canonical wire `u8`.
fn gpu_mux_to_wire(value: GpuMuxState) -> u8 {
    match value {
        GpuMuxState::Integrated => gpu_mux::INTEGRATED,
        GpuMuxState::Discrete => gpu_mux::DISCRETE,
        GpuMuxState::Unknown => gpu_mux::UNKNOWN,
    }
}

/// Преобразовать domain `GpuAccessPolicy` в canonical wire `u8`.
fn gpu_access_to_wire(value: GpuAccessPolicy) -> u8 {
    match value {
        GpuAccessPolicy::Unblocked => gpu_access::UNBLOCKED,
        GpuAccessPolicy::Blocked => gpu_access::BLOCKED,
        GpuAccessPolicy::Pending => gpu_access::PENDING,
        GpuAccessPolicy::Unknown => gpu_access::UNKNOWN,
    }
}

/// Преобразовать domain `PerformanceProfile` в canonical wire `u8`.
fn performance_current_to_wire(value: PerformanceProfile) -> u8 {
    match value {
        PerformanceProfile::Silent => performance::SILENT,
        PerformanceProfile::Balanced => performance::BALANCED,
        PerformanceProfile::Turbo => performance::TURBO,
    }
}

/// Преобразовать список доступных `PerformanceProfile` в canonical wire mask
/// (bit0=Silent, bit1=Balanced, bit2=Turbo).
fn performance_mask_to_wire(available: &[PerformanceProfile]) -> u8 {
    let mut mask: u8 = 0;
    for p in available {
        mask |= match p {
            PerformanceProfile::Silent => performance::SILENT_BIT,
            PerformanceProfile::Balanced => performance::BALANCED_BIT,
            PerformanceProfile::Turbo => performance::TURBO_BIT,
        };
    }
    mask
}

/// Преобразовать domain `ChargeLimit` в canonical wire `ChargeLimitInfo`.
///
/// Значения переносятся без clamp/округления/изменения шага; UI step не
/// применяется; при `percent == None` wire payload канонизируется
/// (`percent_present = false`, `percent = 0`).
pub fn charge_limit_to_wire(value: ChargeLimit) -> ChargeLimitInfo {
    match (value.percent, value.bounds) {
        (Some(percent), Some(bounds)) => ChargeLimitInfo::with_percent(
            value.enabled,
            percent.get(),
            bounds.min.get(),
            bounds.max.get(),
            bounds.step,
        ),
        (Some(percent), None) => {
            ChargeLimitInfo::with_percent_unknown_bounds(value.enabled, percent.get())
        }
        (None, Some(bounds)) => ChargeLimitInfo::without_percent(
            value.enabled,
            bounds.min.get(),
            bounds.max.get(),
            bounds.step,
        ),
        (None, None) => ChargeLimitInfo::without_percent_unknown_bounds(value.enabled),
    }
}

/// Преобразовать `ProviderError` в `zbus::fdo::Error`.
///
/// Детерминированное отображение классов ошибок; диагностический смысл строки
/// сохраняется; чистый mapper не логирует.
fn provider_error_to_dbus(error: ProviderError) -> zbus::fdo::Error {
    match error {
        ProviderError::Unsupported(msg) => zbus::fdo::Error::NotSupported(msg),
        ProviderError::PermissionDenied(msg) => zbus::fdo::Error::AccessDenied(msg),
        ProviderError::InvalidRequest(msg) => zbus::fdo::Error::InvalidArgs(msg),
        ProviderError::BackendUnavailable(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Timeout(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Io(e) => zbus::fdo::Error::Failed(e.to_string()),
        ProviderError::Dbus(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Internal(msg) => zbus::fdo::Error::Failed(msg),
    }
}

/// Wire-кодирование `ChargeLimitInfo` в D-Bus tuple `(bbyyyy)`.
///
/// zbus 5.13.2 server-side interface macro требует `Value: From<T>` и `T: Type`
/// для типа property; кастомный struct (`ChargeLimitInfo`) не конвертируется
/// без impl в protocol crate (за пределами scope). Кортеж из шести полей имеет
/// ту же D-Bus signature `(bbyyyy)`, что и `ChargeLimitInfo`, поэтому client
/// proxy остаётся без изменений.
type ChargeLimitTuple = (bool, bool, u8, bool, u8, u8, u8);

fn charge_limit_info_to_tuple(info: ChargeLimitInfo) -> ChargeLimitTuple {
    (
        info.enabled,
        info.percent_present,
        info.percent,
        info.bounds_present,
        info.min_percent,
        info.max_percent,
        info.step_percent,
    )
}

/// Серверный интерфейс `io.github.orbiscontrol.Session1` (getter-only).
#[zbus::interface(name = "io.github.orbiscontrol.Session1")]
impl SessionService {
    /// Текущий Battery Charge Limit (read-only property, wire signature
    /// `(bbyyyy)`).
    #[zbus(property)]
    async fn charge_limit(&self) -> zbus::fdo::Result<ChargeLimitTuple> {
        let info = self
            .read_charge_limit()
            .await
            .map_err(provider_error_to_dbus)?;
        Ok(charge_limit_info_to_tuple(info))
    }

    /// Текущий dGPU runtime power state (read-only property, wire signature `y`).
    #[zbus(property)]
    async fn gpu_power(&self) -> zbus::fdo::Result<u8> {
        let value = self
            .read_gpu_power()
            .await
            .map_err(provider_error_to_dbus)?;
        Ok(gpu_power_to_wire(value))
    }

    /// Текущее физическое MUX состояние (read-only property, wire signature `y`).
    #[zbus(property)]
    async fn gpu_mux(&self) -> zbus::fdo::Result<u8> {
        let value = self.read_gpu_mux().await.map_err(provider_error_to_dbus)?;
        Ok(gpu_mux_to_wire(value))
    }

    /// Текущая dGPU access policy (read-only property, wire signature `y`).
    #[zbus(property)]
    async fn gpu_access(&self) -> zbus::fdo::Result<u8> {
        let value = self
            .read_gpu_access()
            .await
            .map_err(provider_error_to_dbus)?;
        Ok(gpu_access_to_wire(value))
    }

    /// Текущий Performance Mode (read-only property, wire signature `(yy)`:
    /// current + available mask).
    #[zbus(property)]
    async fn performance(&self) -> zbus::fdo::Result<(u8, u8)> {
        let (current, available) = self
            .read_performance()
            .await
            .map_err(provider_error_to_dbus)?;
        Ok((
            performance_current_to_wire(current),
            performance_mask_to_wire(&available),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_core::action::ApplyResult;
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::identity::BackendIdentity;
    use orbis_core::newtypes::Percent;
    use orbis_providers::error::ValidationResult;
    use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
    use zbus::object_server::Interface;

    use super::*;
    use orbis_core::battery::ChargeLimitBounds;

    /// Заранее заданный исход scripted provider.
    #[derive(Debug, Clone, Copy)]
    enum ScriptedRead {
        Limit(ChargeLimit),
        Unsupported,
    }

    /// Тестовый BatteryProvider: очередь заранее заданных результатов,
    /// счётчик чтений; mutation-методы возвращают Unsupported без I/O.
    struct ScriptedBatteryProvider {
        results: Mutex<VecDeque<ScriptedRead>>,
        reads: AtomicUsize,
    }

    impl ScriptedBatteryProvider {
        fn new(reads: Vec<ScriptedRead>) -> Self {
            Self {
                results: Mutex::new(reads.into()),
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }

        fn to_result(read: ScriptedRead) -> Result<ChargeLimit, ProviderError> {
            match read {
                ScriptedRead::Limit(l) => Ok(l),
                ScriptedRead::Unsupported => {
                    Err(ProviderError::Unsupported("scripted unsupported".into()))
                }
            }
        }
    }

    impl Provider for ScriptedBatteryProvider {
        fn id(&self) -> &'static str {
            "scripted-battery"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-battery")
        }

        fn timeout(&self) -> Duration {
            Duration::from_millis(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-battery: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl BatteryProvider for ScriptedBatteryProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let read = self
                .results
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted provider: очередь результатов исчерпана");
            Self::to_result(read)
        }

        async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported(
                "scripted read-only backend: set_charge_limit недоступна".into(),
            ))
        }

        async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported(
                "scripted read-only backend: one_shot_full_charge недоступна".into(),
            ))
        }

        fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
            ValidationResult::invalid("read-only backend: запись charge limit не поддерживается")
        }
    }

    fn limit(enabled: bool, percent: Option<u8>, min: u8, max: u8, step: u8) -> ChargeLimit {
        ChargeLimit::new(
            enabled,
            percent.map(|p| Percent::new(p).expect("range")),
            Some(
                ChargeLimitBounds::new(
                    Percent::new(min).expect("range"),
                    Percent::new(max).expect("range"),
                    step,
                )
                .expect("valid"),
            ),
        )
        .expect("valid")
    }

    fn service(reads: Vec<ScriptedRead>) -> (SessionService, Arc<ScriptedBatteryProvider>) {
        let provider = Arc::new(ScriptedBatteryProvider::new(reads));
        let svc = SessionService::new(provider.clone());
        (svc, provider)
    }

    #[test]
    fn maps_domain_charge_limit_with_percent_to_wire() {
        let value = limit(true, Some(80), 40, 100, 5);
        let wire = charge_limit_to_wire(value);
        assert!(wire.enabled);
        assert!(wire.percent_present);
        assert_eq!(wire.percent, 80);
        assert_eq!(wire.percent(), Some(80));
        assert_eq!(wire.min_percent, 40);
        assert_eq!(wire.max_percent, 100);
        assert_eq!(wire.step_percent, 5);
    }

    #[test]
    fn maps_domain_charge_limit_without_percent_to_canonical_wire() {
        let value = limit(false, None, 40, 100, 5);
        let wire = charge_limit_to_wire(value);
        assert!(!wire.enabled);
        assert!(!wire.percent_present);
        assert_eq!(wire.percent, 0);
        assert_eq!(wire.percent(), None);
        assert_eq!(wire.min_percent, 40);
        assert_eq!(wire.max_percent, 100);
        assert_eq!(wire.step_percent, 5);
    }

    #[tokio::test]
    async fn service_reads_provider_once() {
        let (svc, provider) = service(vec![ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5))]);
        let wire = svc.read_charge_limit().await.expect("read");
        assert_eq!(wire.percent, 80);
        assert_eq!(provider.reads(), 1);
    }

    #[tokio::test]
    async fn service_does_not_cache_charge_limit() {
        let (svc, provider) = service(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
            ScriptedRead::Limit(limit(true, Some(60), 40, 100, 5)),
        ]);
        let first = svc.read_charge_limit().await.expect("read1");
        let second = svc.read_charge_limit().await.expect("read2");
        assert_eq!(first.percent, 80);
        assert_eq!(second.percent, 60);
        assert_eq!(provider.reads(), 2);
    }

    #[test]
    fn unsupported_maps_to_dbus_not_supported() {
        let err = provider_error_to_dbus(ProviderError::Unsupported("x".into()));
        assert!(matches!(err, zbus::fdo::Error::NotSupported(_)));
    }

    #[test]
    fn permission_denied_maps_to_dbus_access_denied() {
        let err = provider_error_to_dbus(ProviderError::PermissionDenied("x".into()));
        assert!(matches!(err, zbus::fdo::Error::AccessDenied(_)));
    }

    #[test]
    fn invalid_request_maps_to_dbus_invalid_args() {
        let err = provider_error_to_dbus(ProviderError::InvalidRequest("x".into()));
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
    }

    #[test]
    fn backend_errors_map_to_dbus_failed() {
        for err in [
            ProviderError::BackendUnavailable("b".into()),
            ProviderError::Timeout("t".into()),
            ProviderError::Dbus("d".into()),
            ProviderError::Internal("i".into()),
        ] {
            assert!(matches!(
                provider_error_to_dbus(err),
                zbus::fdo::Error::Failed(_)
            ));
        }
        let io = provider_error_to_dbus(ProviderError::Io(std::io::Error::other("io")));
        assert!(matches!(io, zbus::fdo::Error::Failed(_)));
    }

    #[test]
    fn server_interface_name_matches_protocol() {
        let name = <SessionService as Interface>::name();
        assert_eq!(name.to_string(), orbis_session_protocol::INTERFACE_NAME);
    }

    #[tokio::test]
    async fn dbus_property_preserves_authoritative_wire_value() {
        let (svc, provider) = service(vec![ScriptedRead::Limit(limit(true, Some(60), 40, 100, 5))]);
        // Прямой вызов async property getter (без ObjectServer).
        let wire = svc.charge_limit().await.expect("property");
        // (enabled, percent_present, percent, bounds_present, min, max, step)
        assert_eq!(wire, (true, true, 60, true, 40, 100, 5));
        assert_eq!(provider.reads(), 1);
    }

    #[tokio::test]
    async fn dbus_property_maps_unsupported_to_not_supported() {
        let (svc, _) = service(vec![ScriptedRead::Unsupported]);
        let err = svc.charge_limit().await.expect_err("unsupported");
        assert!(matches!(err, zbus::fdo::Error::NotSupported(_)));
    }

    // -----------------------------------------------------------------------
    // GPU capability routing: каждый property обращается только к своему
    // capability provider.
    // -----------------------------------------------------------------------

    struct ScriptedGpuPower {
        value: GpuPowerState,
    }
    impl Provider for ScriptedGpuPower {
        fn id(&self) -> &'static str {
            "scripted-gpu-power"
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-gpu-power")
        }
        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-gpu-power: {feature} недоступен")
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }
    #[async_trait]
    impl GpuPowerProvider for ScriptedGpuPower {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            Ok(self.value)
        }
    }

    struct ScriptedGpuMux {
        value: GpuMuxState,
    }
    impl Provider for ScriptedGpuMux {
        fn id(&self) -> &'static str {
            "scripted-gpu-mux"
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-gpu-mux")
        }
        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-gpu-mux: {feature} недоступен")
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }
    #[async_trait]
    impl GpuMuxProvider for ScriptedGpuMux {
        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            Ok(self.value)
        }
    }

    struct ScriptedGpuAccess {
        value: GpuAccessPolicy,
    }
    impl Provider for ScriptedGpuAccess {
        fn id(&self) -> &'static str {
            "scripted-gpu-access"
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-gpu-access")
        }
        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-gpu-access: {feature} недоступен")
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }
    #[async_trait]
    impl GpuAccessProvider for ScriptedGpuAccess {
        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            Ok(self.value)
        }
    }

    fn gpu_service() -> SessionService {
        SessionService::new(Arc::new(ScriptedBatteryProvider::new(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
        ])))
        .with_gpu_power(Arc::new(ScriptedGpuPower {
            value: GpuPowerState::Suspended,
        }))
        .with_gpu_mux(Arc::new(ScriptedGpuMux {
            value: GpuMuxState::Integrated,
        }))
        .with_gpu_access(Arc::new(ScriptedGpuAccess {
            value: GpuAccessPolicy::Unblocked,
        }))
    }

    #[tokio::test]
    async fn gpu_properties_route_to_own_capability() {
        let svc = gpu_service();
        // Каждый property возвращает значение строго своего capability.
        assert_eq!(svc.gpu_power().await.expect("power"), gpu_power::SUSPENDED);
        assert_eq!(svc.gpu_mux().await.expect("mux"), gpu_mux::INTEGRATED);
        assert_eq!(
            svc.gpu_access().await.expect("access"),
            gpu_access::UNBLOCKED
        );
    }

    #[tokio::test]
    async fn missing_gpu_capability_is_not_supported() {
        // SessionService без GPU capabilities.
        let svc = SessionService::new(Arc::new(ScriptedBatteryProvider::new(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
        ])));
        assert!(matches!(
            svc.gpu_power().await,
            Err(zbus::fdo::Error::NotSupported(_))
        ));
        assert!(matches!(
            svc.gpu_mux().await,
            Err(zbus::fdo::Error::NotSupported(_))
        ));
        assert!(matches!(
            svc.gpu_access().await,
            Err(zbus::fdo::Error::NotSupported(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Performance Mode capability routing
    // -----------------------------------------------------------------------

    struct ScriptedPerformance {
        current: PerformanceProfile,
        available: Vec<PerformanceProfile>,
    }
    impl Provider for ScriptedPerformance {
        fn id(&self) -> &'static str {
            "scripted-performance"
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-performance")
        }
        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-performance: {feature} недоступен")
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }
    #[async_trait]
    impl PerformanceProvider for ScriptedPerformance {
        async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
            Ok(self.available.clone())
        }
        async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
            Ok(self.current)
        }
        async fn set_profile(
            &self,
            _profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }
        async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }
        async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }
        fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
            ValidationResult::invalid("read-only")
        }
    }

    fn performance_service() -> SessionService {
        SessionService::new(Arc::new(ScriptedBatteryProvider::new(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
        ])))
        .with_performance(Arc::new(ScriptedPerformance {
            current: PerformanceProfile::Silent,
            available: vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ],
        }))
    }

    #[test]
    fn maps_domain_performance_to_wire() {
        assert_eq!(
            performance_current_to_wire(PerformanceProfile::Silent),
            performance::SILENT
        );
        assert_eq!(
            performance_current_to_wire(PerformanceProfile::Balanced),
            performance::BALANCED
        );
        assert_eq!(
            performance_current_to_wire(PerformanceProfile::Turbo),
            performance::TURBO
        );
        assert_eq!(
            performance_mask_to_wire(&[
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ]),
            0b111
        );
        assert_eq!(
            performance_mask_to_wire(&[PerformanceProfile::Balanced]),
            performance::BALANCED_BIT
        );
    }

    #[tokio::test]
    async fn performance_property_returns_wire_current_and_mask() {
        let svc = performance_service();
        assert_eq!(svc.performance().await.expect("property"), (0, 0b111));
    }

    #[tokio::test]
    async fn missing_performance_capability_is_not_supported() {
        let svc = SessionService::new(Arc::new(ScriptedBatteryProvider::new(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
        ])));
        assert!(matches!(
            svc.performance().await,
            Err(zbus::fdo::Error::NotSupported(_))
        ));
    }
}
