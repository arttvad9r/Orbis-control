//! D-Bus service object для session интерфейса.
//!
//! Серверная сторона интерфейса `io.github.orbiscontrol.Session1`: read-only
//! properties (`ChargeLimit`, GPU capabilities, `Performance`). Этот модуль
//! только определяет service object и
//! domain-to-wire conversion; он не создаёт Connection, ObjectServer, runtime и
//! не регистрирует bus name — bus bootstrap выполняется отдельным микрошагом.

use std::sync::Arc;

use orbis_core::battery::ChargeLimit;
use orbis_core::fan::{FanCurve, FanCurvePoint, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::newtypes::{FanPwm, TemperatureC};
use orbis_core::profile::{AsusdFanProfile, PerformanceProfile};
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};
use orbis_session_protocol::{ChargeLimitInfo, gpu_access, gpu_mux, gpu_power, performance};

use crate::fans::{AsusdFanCurveSource, asusd_fan_profile_from_wire};

/// Service object session интерфейса.
///
/// Владеет независимыми capability providers: battery + опциональные GPU
/// capabilities (power / MUX / access) + опциональный Performance Mode +
/// опциональный read-only asusd fan curve источник.
/// Провайдеры передаются извне. Конструктор не выполняет I/O, не читает
/// состояние, не открывает D-Bus и не создаёт runtime; кэш/last error/mutable
/// state отсутствуют.
pub struct SessionService {
    battery: Arc<dyn BatteryProvider>,
    gpu_power: Option<Arc<dyn GpuPowerProvider>>,
    gpu_mux: Option<Arc<dyn GpuMuxProvider>>,
    gpu_access: Option<Arc<dyn GpuAccessProvider>>,
    performance: Option<Arc<dyn PerformanceProvider>>,
    fan_curves: Option<Arc<dyn AsusdFanCurveSource>>,
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
            fan_curves: None,
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

    /// Добавить read-only asusd fan curve source (profile-specific curves).
    pub fn with_fan_curves(mut self, source: Arc<dyn AsusdFanCurveSource>) -> Self {
        self.fan_curves = Some(source);
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

    /// Прочитать сохранённую fan curve для профиля и вентилятора.
    ///
    /// Выполняет один authoritative read через asusd fan curve source
    /// (`read_curves(profile)`) и выбирает кривую запрошенного вентилятора.
    /// Кэш отсутствует; CPU/GPU не смешиваются (каждый fan читается из
    /// собственной кривой одного прочитанного set). Ошибки источника
    /// сохраняются честно (включая Unsupported/PermissionDenied/malformed).
    pub async fn read_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: FanId,
    ) -> Result<FanCurve, ProviderError> {
        let source = self.fan_curves.as_ref().ok_or_else(|| {
            ProviderError::Unsupported("session: fan curve capability недоступна".into())
        })?;
        let set = source.read_curves(profile).await?;
        let curve = match fan {
            FanId::Cpu => set.cpu,
            FanId::Gpu => set.gpu,
            other => {
                return Err(ProviderError::Unsupported(format!(
                    "session: fan curve для {other:?} не поддерживается (только CPU/GPU)"
                )));
            }
        };
        Ok(domain_from_asusd_curve(profile, curve))
    }
}

/// Преобразовать wire-прочитанную `AsusdFanCurve` в доменную `FanCurve`.
///
/// Lossless: raw PWM 0..255 переносятся без процентов; профиль отображается
/// в `PerformanceProfile` (для UI), сам lossless `AsusdFanProfile` сохраняется
/// на wire (wire DTO включает profile).
fn domain_from_asusd_curve(
    profile: AsusdFanProfile,
    curve: crate::fans::AsusdFanCurve,
) -> FanCurve {
    let points = curve
        .temps
        .iter()
        .zip(curve.pwms.iter())
        .map(|(t, p)| FanCurvePoint::new(*t, *p))
        .collect();
    FanCurve {
        profile: PerformanceProfile::from(profile),
        fan: curve.fan,
        points,
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
/// Значения configured/effective переносятся раздельно, без clamp/округления;
/// отсутствующие значения канонизируются в payload.
pub fn charge_limit_to_wire(value: ChargeLimit) -> ChargeLimitInfo {
    let bounds = value.bounds;
    let (bounds_present, min_percent, max_percent, step_percent) = match bounds {
        Some(bounds) => (true, bounds.min.get(), bounds.max.get(), bounds.step),
        None => (false, 0, 0, 0),
    };
    let configured = value.configured_percent.map(|p| p.get());
    let effective = value.effective_percent.map(|p| p.get());
    ChargeLimitInfo {
        enabled: value.enabled,
        configured_percent_present: configured.is_some(),
        configured_percent: configured.unwrap_or(0),
        effective_percent_present: effective.is_some(),
        effective_percent: effective.unwrap_or(0),
        bounds_present,
        min_percent,
        max_percent,
        step_percent,
    }
}

/// Преобразовать domain `ProviderError` в `zbus::fdo::Error`.
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
        ProviderError::Conflict(msg) => zbus::fdo::Error::Failed(msg),
    }
}

/// Wire-кодирование `ChargeLimitInfo` в D-Bus tuple `(bbybybyyy)`.
///
/// zbus 5.13.2 server-side interface macro требует `Value: From<T>` и `T: Type`
/// для типа property; кастомный struct (`ChargeLimitInfo`) не конвертируется
/// без impl в protocol crate (за пределами scope). Кортеж из девяти полей имеет
/// ту же D-Bus signature `(bbybybyyy)`, что и `ChargeLimitInfo`, поэтому client
/// proxy остаётся без изменений.
type ChargeLimitTuple = (bool, bool, u8, bool, u8, bool, u8, u8, u8);

fn charge_limit_info_to_tuple(info: ChargeLimitInfo) -> ChargeLimitTuple {
    (
        info.enabled,
        info.configured_percent_present,
        info.configured_percent,
        info.effective_percent_present,
        info.effective_percent,
        info.bounds_present,
        info.min_percent,
        info.max_percent,
        info.step_percent,
    )
}

/// Wire-кодирование `FanCurveInfo` в D-Bus tuple `(uyayay)`.
///
/// Ту же причину, что и `ChargeLimitTuple`: кастомный struct не конвертируется
/// в `Value` в server-side interface macro. Кортеж из четырёх полей имеет ту же
/// D-Bus signature `(uyayay)`, что и `FanCurveInfo`, поэтому client proxy
/// декодирует tuple в `FanCurveInfo`.
type FanCurveTuple = (u32, u8, Vec<u8>, Vec<u8>);

fn asusd_curve_to_wire_tuple(
    profile: AsusdFanProfile,
    curve: &crate::fans::AsusdFanCurve,
) -> FanCurveTuple {
    (
        profile.wire(),
        fan_id_to_wire(&curve.fan),
        curve.temps.iter().map(|t| t.get() as u8).collect(),
        curve.pwms.iter().map(|p| p.get()).collect(),
    )
}

/// Strict mapping `FanId` → wire `u8` (0=CPU, 1=GPU).
fn fan_id_to_wire(fan: &FanId) -> u8 {
    match fan {
        FanId::Cpu => orbis_session_protocol::fan_id::CPU,
        FanId::Gpu => orbis_session_protocol::fan_id::GPU,
        other => panic!("fan_id_to_wire: неподдерживаемый fan {other:?} (только CPU/GPU)"),
    }
}

/// Strict decode wire `u8` → `FanId` (0=CPU, 1=GPU).
fn fan_id_from_wire(raw: u8) -> Result<FanId, ProviderError> {
    match raw {
        orbis_session_protocol::fan_id::CPU => Ok(FanId::Cpu),
        orbis_session_protocol::fan_id::GPU => Ok(FanId::Gpu),
        other => Err(ProviderError::InvalidRequest(format!(
            "session: неизвестный fan wire value {other}"
        ))),
    }
}

/// Серверный интерфейс `io.github.orbiscontrol.Session1` (getter-only).
#[zbus::interface(name = "io.github.orbiscontrol.Session1")]
impl SessionService {
    /// Текущий Battery Charge Limit (read-only property, wire signature
    /// `(bbybybyyy)`).
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

    /// Сохранённая fan curve для профиля и вентилятора
    /// (read-only method, wire signature `(uyayay)`).
    async fn fan_curve(&self, profile: u32, fan: u8) -> zbus::fdo::Result<FanCurveTuple> {
        let profile = asusd_fan_profile_from_wire(profile).map_err(provider_error_to_dbus)?;
        let fan = fan_id_from_wire(fan).map_err(provider_error_to_dbus)?;
        let curve = self
            .read_fan_curve(profile, fan.clone())
            .await
            .map_err(provider_error_to_dbus)?;
        let temps: [TemperatureC; 8] = curve
            .points
            .iter()
            .map(|p| p.temp)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| zbus::fdo::Error::Failed("fan curve: 8 точек обязательны".into()))?;
        let pwms: [FanPwm; 8] = curve
            .points
            .iter()
            .map(|p| p.pwm)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| zbus::fdo::Error::Failed("fan curve: 8 точек обязательны".into()))?;
        let asusd = crate::fans::AsusdFanCurve {
            fan,
            temps,
            pwms,
            enabled: true,
        };
        Ok(asusd_curve_to_wire_tuple(profile, &asusd))
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
    use orbis_core::newtypes::{FanPwm, Percent, TemperatureC};
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
        assert!(wire.configured_percent_present);
        assert_eq!(wire.configured_percent, 80);
        assert!(wire.effective_percent_present);
        assert_eq!(wire.effective_percent, 80);
        assert_eq!(wire.min_percent, 40);
        assert_eq!(wire.max_percent, 100);
        assert_eq!(wire.step_percent, 5);
    }

    #[test]
    fn maps_domain_charge_limit_without_percent_to_canonical_wire() {
        let value = limit(false, None, 40, 100, 5);
        let wire = charge_limit_to_wire(value);
        assert!(!wire.enabled);
        assert!(!wire.configured_percent_present);
        assert_eq!(wire.configured_percent, 0);
        assert!(!wire.effective_percent_present);
        assert_eq!(wire.effective_percent, 0);
        assert_eq!(wire.min_percent, 40);
        assert_eq!(wire.max_percent, 100);
        assert_eq!(wire.step_percent, 5);
    }

    #[tokio::test]
    async fn service_reads_provider_once() {
        let (svc, provider) = service(vec![ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5))]);
        let wire = svc.read_charge_limit().await.expect("read");
        assert_eq!(wire.configured_percent, 80);
        assert_eq!(wire.effective_percent, 80);
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
        assert_eq!(first.configured_percent, 80);
        assert_eq!(second.configured_percent, 60);
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
        // (enabled, configured_present, configured, effective_present,
        // effective, bounds_present, min, max, step)
        assert_eq!(wire, (true, true, 60, true, 60, true, 40, 100, 5));
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

    // -----------------------------------------------------------------------
    // Read-only path invariant tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn mutation_methods_return_unsupported() {
        // All mutation methods on the read-only providers must return
        // Unsupported without performing any I/O or side effects.
        let (_svc, provider) =
            service(vec![ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5))]);

        // set_charge_limit must not be callable and must return Unsupported.
        let err = provider
            .set_charge_limit(90)
            .await
            .expect_err("set unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        // one_shot_full_charge must not be callable.
        let err = provider
            .one_shot_full_charge()
            .await
            .expect_err("oneshot unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        // validate_charge_limit must return invalid.
        assert!(matches!(
            provider.validate_charge_limit(80),
            ValidationResult::Invalid(_)
        ));

        // Verify no reads were consumed by mutation attempts.
        assert_eq!(provider.reads(), 0);
    }

    #[tokio::test]
    async fn read_only_service_produces_consistent_charge_limit() {
        // Multiple reads from the same service should be consistent and
        // independent — no caching, no mutation side effects.
        let (svc, provider) = service(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
            ScriptedRead::Limit(limit(true, Some(60), 40, 100, 5)),
            ScriptedRead::Limit(limit(false, None, 40, 100, 5)),
        ]);

        let first = svc.read_charge_limit().await.expect("read1");
        assert_eq!(first.configured_percent, 80);
        assert!(first.configured_percent_present);

        let second = svc.read_charge_limit().await.expect("read2");
        assert_eq!(second.configured_percent, 60);
        assert!(second.configured_percent_present);

        let third = svc.read_charge_limit().await.expect("read3");
        assert!(!third.enabled);
        assert!(!third.configured_percent_present);

        assert_eq!(provider.reads(), 3);
    }

    #[tokio::test]
    async fn gpu_unsupported_when_provider_absent() {
        // GPU capabilities are optional; when absent, properties return
        // NotSupported — not an error or crash.
        let (svc, _) = service(vec![ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5))]);
        // No GPU providers configured.
        assert!(matches!(
            svc.read_gpu_power().await,
            Err(ProviderError::Unsupported(_))
        ));
        assert!(matches!(
            svc.read_gpu_mux().await,
            Err(ProviderError::Unsupported(_))
        ));
        assert!(matches!(
            svc.read_gpu_access().await,
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[tokio::test]
    async fn performance_unsupported_when_provider_absent() {
        // Performance capability is optional; when absent, returns Unsupported.
        let (svc, _) = service(vec![ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5))]);
        let err = svc.read_performance().await.expect_err("no provider");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        // Also verify the D-Bus property returns NotSupported.
        assert!(matches!(
            svc.performance().await,
            Err(zbus::fdo::Error::NotSupported(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Profile-specific fan curve read (read_fan_curve / fan_curve method)
    // -----------------------------------------------------------------------

    struct ScriptedAsusdFanSource {
        cpu_by_profile: std::collections::HashMap<AsusdFanProfile, crate::fans::AsusdFanCurve>,
        gpu_by_profile: std::collections::HashMap<AsusdFanProfile, crate::fans::AsusdFanCurve>,
        reads: AtomicUsize,
    }

    fn asusd_curve(fan: FanId, first_temp: i16, pwm_base: u8) -> crate::fans::AsusdFanCurve {
        let mut temps = [TemperatureC::new(0).expect("const"); 8];
        let mut pwms = [FanPwm::new(0).expect("const"); 8];
        for i in 0..8 {
            temps[i] = TemperatureC::new(first_temp + i as i16 * 5).expect("temp");
            pwms[i] = FanPwm::new(pwm_base + i as u8 * 10).expect("pwm");
        }
        crate::fans::AsusdFanCurve {
            fan,
            temps,
            pwms,
            enabled: true,
        }
    }

    #[async_trait]
    impl AsusdFanCurveSource for ScriptedAsusdFanSource {
        async fn read_curves(
            &self,
            profile: AsusdFanProfile,
        ) -> Result<crate::fans::AsusdFanCurveSet, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let cpu =
                self.cpu_by_profile.get(&profile).cloned().ok_or_else(|| {
                    ProviderError::Unsupported(format!("profile {profile:?} cpu"))
                })?;
            let gpu =
                self.gpu_by_profile.get(&profile).cloned().ok_or_else(|| {
                    ProviderError::Unsupported(format!("profile {profile:?} gpu"))
                })?;
            Ok(crate::fans::AsusdFanCurveSet { profile, cpu, gpu })
        }
    }

    impl ScriptedAsusdFanSource {
        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    fn fan_curve_service() -> (SessionService, Arc<ScriptedAsusdFanSource>) {
        let source = Arc::new(ScriptedAsusdFanSource {
            cpu_by_profile: {
                let mut m = std::collections::HashMap::new();
                m.insert(AsusdFanProfile::Balanced, asusd_curve(FanId::Cpu, 40, 10));
                m.insert(AsusdFanProfile::Quiet, asusd_curve(FanId::Cpu, 30, 5));
                m
            },
            gpu_by_profile: {
                let mut m = std::collections::HashMap::new();
                m.insert(AsusdFanProfile::Balanced, asusd_curve(FanId::Gpu, 25, 0));
                m.insert(AsusdFanProfile::Quiet, asusd_curve(FanId::Gpu, 20, 0));
                m
            },
            reads: AtomicUsize::new(0),
        });
        let svc = SessionService::new(Arc::new(ScriptedBatteryProvider::new(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
        ])))
        .with_fan_curves(source.clone());
        (svc, source)
    }

    #[tokio::test]
    async fn service_reads_profile_specific_curve_from_own_source() {
        let (svc, source) = fan_curve_service();
        let curve = svc
            .read_fan_curve(AsusdFanProfile::Balanced, FanId::Cpu)
            .await
            .expect("balanced curve");
        assert_eq!(curve.profile, PerformanceProfile::Balanced);
        assert_eq!(curve.fan, FanId::Cpu);
        assert_eq!(curve.points[0].temp.get(), 40);
        assert_eq!(source.reads(), 1);
    }

    #[tokio::test]
    async fn service_reads_quiet_pair_returns_own_curve() {
        // Quiet и Balanced читаются независимо: каждый профиль — собственный
        // sentinel, а не замена активной кривой.
        let (svc, source) = fan_curve_service();
        let quiet = svc
            .read_fan_curve(AsusdFanProfile::Quiet, FanId::Cpu)
            .await
            .expect("quiet curve");
        assert_eq!(quiet.points[0].temp.get(), 30);
        assert_eq!(source.reads(), 1);

        let balanced = svc
            .read_fan_curve(AsusdFanProfile::Balanced, FanId::Cpu)
            .await
            .expect("balanced curve");
        assert_eq!(balanced.points[0].temp.get(), 40);
        assert_eq!(source.reads(), 2);
    }

    #[tokio::test]
    async fn service_cpu_gpu_not_mixed() {
        let (svc, _) = fan_curve_service();
        let cpu = svc
            .read_fan_curve(AsusdFanProfile::Balanced, FanId::Cpu)
            .await
            .expect("cpu");
        let gpu = svc
            .read_fan_curve(AsusdFanProfile::Balanced, FanId::Gpu)
            .await
            .expect("gpu");
        assert_eq!(cpu.fan, FanId::Cpu);
        assert_eq!(gpu.fan, FanId::Gpu);
        assert_eq!(cpu.points[0].temp.get(), 40);
        assert_eq!(gpu.points[0].temp.get(), 25);
    }

    #[tokio::test]
    async fn service_fan_curve_unsupported_when_source_absent() {
        let (svc, _) = service(vec![ScriptedRead::Limit(limit(true, Some(40), 40, 100, 5))]);
        let err = svc
            .read_fan_curve(AsusdFanProfile::Balanced, FanId::Cpu)
            .await
            .expect_err("no source");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        assert!(matches!(
            svc.fan_curve(0, 0).await,
            Err(zbus::fdo::Error::NotSupported(_))
        ));
    }

    #[tokio::test]
    async fn service_fan_curve_method_wire_roundtrip() {
        let (svc, _) = fan_curve_service();
        let tuple = svc.fan_curve(0, 0).await.expect("method");
        // (profile, fan, temps, pwms)
        assert_eq!(tuple.0, orbis_session_protocol::fan_profile::BALANCED);
        assert_eq!(tuple.1, orbis_session_protocol::fan_id::CPU);
        assert_eq!(tuple.2.len(), 8);
        assert_eq!(tuple.2[0], 40);
        assert_eq!(tuple.2[7], 75);
        assert_eq!(tuple.3[0], 10);
        assert_eq!(tuple.3[7], 80);
    }

    #[tokio::test]
    async fn service_fan_curve_method_rejects_unknown_wire() {
        let (svc, _) = fan_curve_service();
        // unknown profile wire → Internal → Failed (strict decode без fallback).
        assert!(matches!(
            svc.fan_curve(7, 0).await,
            Err(zbus::fdo::Error::Failed(_))
        ));
        // unknown fan wire → InvalidArgs.
        assert!(matches!(
            svc.fan_curve(0, 9).await,
            Err(zbus::fdo::Error::InvalidArgs(_))
        ));
    }
}
