//! Mock-провайдеры для Этапа 2 (mock-first).
//!
//! Все операции меняют только `MockState` в памяти; никаких аппаратных вызовов.
//! Feature `mock` включена по умолчанию ТОЛЬКО на Этапе 2 и должна стать
//! не-default при появлении реальных провайдеров.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::RwLock;

use orbis_core::action::{ActionRequirement, ApplyResult, PendingAction};
use orbis_core::automation::{AutomationAction, AutomationRule, AutomationTrigger};
use orbis_core::battery::{ChargeLimit, ChargeLimitBounds};
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::display::DisplayMode;
use orbis_core::fan::{FanCurve, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::lighting::LightingMode;
use orbis_core::limits::{PowerLimitField, PowerLimits};
use orbis_core::newtypes::{Percent, RefreshHz, Rpm};
use orbis_core::profile::PerformanceProfile;
use orbis_core::telemetry::Telemetry;

use crate::error::{OperationId, ProviderError, ValidationResult};
use crate::traits::{
    AnimeProvider, AutomationProvider, BatteryProvider, DisplayProvider, FanProvider,
    FirmwareUpdate, FirmwareUpdateProvider, GpuPowerProvider, GpuProvider, HotkeyProvider,
    LightingProvider, PerformanceProvider, PowerLimitProvider, Provider, ProviderHealth,
    TelemetryProvider,
};

/// Способ имитации ошибки в mock-режиме.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockErrorMode {
    /// Без ошибок.
    None,
    /// Backend недоступен (сервис не запущен).
    BackendDown,
    /// Недостаточно прав.
    PermissionDenied,
    /// Функция не поддерживается.
    Unsupported,
    /// Операция превышает таймаут.
    Timeout,
}

/// Описание ошибки для mock-профиля.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MockStateError {
    /// Сервис не запущен.
    #[error("mock backend недоступен")]
    BackendDown,
    /// Нет прав.
    #[error("mock: недостаточно прав")]
    PermissionDenied,
    /// Не поддерживается.
    #[error("mock: функция не поддерживается")]
    Unsupported,
    /// Таймаут.
    #[error("mock: таймаут")]
    Timeout,
}

impl MockStateError {
    /// Преобразовать в ProviderError.
    pub fn to_provider(&self) -> ProviderError {
        match self {
            Self::BackendDown => ProviderError::BackendUnavailable("mock".into()),
            Self::PermissionDenied => ProviderError::PermissionDenied("mock".into()),
            Self::Unsupported => ProviderError::Unsupported("mock".into()),
            Self::Timeout => ProviderError::Timeout("mock".into()),
        }
    }
}

/// Общее состояние mock-устройства.
///
/// `PartialEq, Eq` нужны для проверки детерминированности mock-профилей
/// (см. `orbis-test-support::devices::all_profiles_are_deterministic`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockState {
    /// Текущий профиль.
    pub profile: PerformanceProfile,
    /// Доступные профили.
    pub profiles: Vec<PerformanceProfile>,
    /// Профиль на AC / батарее.
    pub profile_on_ac: Option<PerformanceProfile>,
    /// Профиль на батарее.
    pub profile_on_battery: Option<PerformanceProfile>,
    /// GPU.
    pub gpu_mode: GpuMode,
    /// Физический MUX.
    pub mux: GpuMuxState,
    /// Доступ приложений к dGPU.
    pub access_policy: GpuAccessPolicy,
    /// Фактический power state dGPU.
    pub gpu_power_state: GpuPowerState,
    /// Лимит зарядки.
    pub charge_limit: ChargeLimit,
    /// Кривые вентиляторов.
    pub fan_curves: BTreeMap<(PerformanceProfile, FanId), FanCurve>,
    /// Power limits.
    pub power_limits: PowerLimits,
    /// Дисплей.
    pub display: DisplayMode,
    /// Подсветка.
    pub lighting: LightingMode,
    /// Вентиляторы (телеметрия).
    pub fans: Vec<(FanId, Rpm)>,
    /// Телеметрия.
    pub telemetry: Telemetry,
    /// Задержка операций (для детерминированных тестов).
    pub op_delay: Duration,
    /// Режим ошибок.
    pub error_mode: MockErrorMode,
    /// Pending-действие (например, MUX после reboot).
    pub pending_action: Option<PendingAction>,
    /// Счётчик выполненных операций (для тестов).
    pub ops: u64,
}

impl MockState {
    /// Состояние по умолчанию (полный набор).
    pub fn full() -> Self {
        let profile = PerformanceProfile::Balanced;
        let mut fan_curves = BTreeMap::new();
        for p in PerformanceProfile::ALL {
            for fan in [FanId::Cpu, FanId::Gpu] {
                fan_curves.insert((p, fan.clone()), default_curve(p, fan));
            }
        }
        Self {
            profile,
            profiles: PerformanceProfile::ALL.to_vec(),
            profile_on_ac: Some(PerformanceProfile::Balanced),
            profile_on_battery: Some(PerformanceProfile::Silent),
            gpu_mode: GpuMode::Standard,
            mux: GpuMuxState::Integrated,
            access_policy: GpuAccessPolicy::Unblocked,
            gpu_power_state: GpuPowerState::Active,
            charge_limit: ChargeLimit::new(
                true,
                Some(Percent::new(80).expect("const")),
                Some(
                    ChargeLimitBounds::new(
                        Percent::new(40).expect("const"),
                        Percent::new(100).expect("const"),
                        1,
                    )
                    .expect("valid"),
                ),
            )
            .expect("valid"),
            fan_curves,
            power_limits: PowerLimits::default(),
            display: DisplayMode {
                current_hz: Some(RefreshHz::new(165).expect("const")),
                modes: vec![
                    orbis_core::display::RefreshMode::new(RefreshHz::new(60).expect("const")),
                    orbis_core::display::RefreshMode::new(RefreshHz::new(120).expect("const")),
                    orbis_core::display::RefreshMode::new(RefreshHz::new(165).expect("const")),
                ],
                overdrive: Some(true),
                hdr: orbis_core::display::HdrState::Disabled,
            },
            lighting: LightingMode::Static,
            fans: vec![
                (FanId::Cpu, Rpm::new(2400).expect("const")),
                (FanId::Gpu, Rpm::new(2600).expect("const")),
            ],
            telemetry: Telemetry::empty(),
            op_delay: Duration::from_millis(5),
            error_mode: MockErrorMode::None,
            pending_action: None,
            ops: 0,
        }
    }

    /// Применить ошибку к результату (через mock-помощник).
    #[allow(dead_code)]
    fn maybe_err<T>(&self, ok: T) -> Result<T, ProviderError> {
        match self.error_mode {
            MockErrorMode::None => Ok(ok),
            MockErrorMode::BackendDown => Err(MockStateError::BackendDown.to_provider()),
            MockErrorMode::PermissionDenied => Err(MockStateError::PermissionDenied.to_provider()),
            MockErrorMode::Unsupported => Err(MockStateError::Unsupported.to_provider()),
            MockErrorMode::Timeout => Err(MockStateError::Timeout.to_provider()),
        }
    }
}

/// Кривая по умолчанию.
fn default_curve(profile: PerformanceProfile, fan: FanId) -> FanCurve {
    let base: i16 = match profile {
        PerformanceProfile::Silent => 35,
        PerformanceProfile::Balanced => 45,
        PerformanceProfile::Turbo => 55,
    };
    let points = [
        (50, 0),
        (55, 8),
        (60, 13),
        (65, 26),
        (70, 36),
        (75, 54),
        (79, 77),
        (85, 100),
    ]
    .into_iter()
    .map(|(t, p)| {
        orbis_core::fan::FanCurvePoint::new(
            orbis_core::newtypes::TemperatureC::new(base + t - 45).expect("temp"),
            Percent::new(p).expect("pwm"),
        )
    })
    .collect();
    FanCurve {
        profile,
        fan,
        points,
    }
}

/// Общий mock-провайдер: реализует все trait-ы поверх `MockState`.
pub struct MockProvider {
    state: Arc<RwLock<MockState>>,
}

/// Преобразовать режим ошибки в ProviderError (вспомогательная функция).
fn mock_error(mode: MockErrorMode) -> ProviderError {
    match mode {
        MockErrorMode::None => unreachable!("нет ошибки"),
        MockErrorMode::BackendDown => MockStateError::BackendDown.to_provider(),
        MockErrorMode::PermissionDenied => MockStateError::PermissionDenied.to_provider(),
        MockErrorMode::Unsupported => MockStateError::Unsupported.to_provider(),
        MockErrorMode::Timeout => MockStateError::Timeout.to_provider(),
    }
}

impl MockProvider {
    /// Создать провайдер над состоянием.
    pub fn new(state: Arc<RwLock<MockState>>) -> Self {
        Self { state }
    }

    /// Доступ к состоянию (для тестов).
    pub fn state(&self) -> Arc<RwLock<MockState>> {
        self.state.clone()
    }

    /// Задержка перед операцией (детерминированная).
    async fn delay(&self) {
        let d = self.state.read().await.op_delay;
        if !d.is_zero() {
            tokio::time::sleep(d).await;
        }
    }

    /// Выполнить мутацию с учётом ошибок.
    async fn mutate<T>(
        &self,
        f: impl FnOnce(&mut MockState) -> Result<T, MockStateError>,
    ) -> Result<T, ProviderError> {
        self.delay().await;
        // Ошибка проверяется ДО мутации: при ошибке состояние не изменяется.
        let err_mode = self.state.read().await.error_mode;
        if err_mode != MockErrorMode::None {
            return Err(mock_error(err_mode));
        }
        let mut guard = self.state.write().await;
        guard.ops += 1;
        let result = f(&mut guard);
        drop(guard);
        result.map_err(|e| e.to_provider())
    }

    /// Прочитать состояние с учётом ошибок.
    async fn read<T>(
        &self,
        f: impl FnOnce(&MockState) -> Result<T, MockStateError>,
    ) -> Result<T, ProviderError> {
        self.delay().await;
        let mut guard = self.state.write().await;
        guard.ops += 1;
        let err_mode = guard.error_mode;
        let result = f(&guard);
        drop(guard);
        match err_mode {
            MockErrorMode::None => result.map_err(|e| e.to_provider()),
            MockErrorMode::Timeout => Err(ProviderError::Timeout("mock".into())),
            MockErrorMode::BackendDown => Err(ProviderError::BackendUnavailable("mock".into())),
            MockErrorMode::PermissionDenied => Err(ProviderError::PermissionDenied("mock".into())),
            MockErrorMode::Unsupported => Err(ProviderError::Unsupported("mock".into())),
        }
    }
}

impl Provider for MockProvider {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity {
            id: "mock".into(),
            version: Some("0.1.0".into()),
            service: None,
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("mock backend: функция '{feature}' недоступна в этом профиле устройства")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.mock",
            "mock backend v0.1.0 (Этап 2, без аппаратных операций)",
        )]
    }
}

#[async_trait]
impl PerformanceProvider for MockProvider {
    async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
        self.read(|s| Ok(s.profiles.clone())).await
    }

    async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
        self.read(|s| Ok(s.profile)).await
    }

    async fn set_profile(&self, profile: PerformanceProfile) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            if !s.profiles.contains(&profile) {
                return Err(MockStateError::Unsupported);
            }
            s.profile = profile;
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        self.read(|s| Ok(s.profile_on_ac)).await
    }

    async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        self.read(|s| Ok(s.profile_on_battery)).await
    }

    fn validate_set_profile(&self, profile: PerformanceProfile) -> ValidationResult {
        match profile {
            PerformanceProfile::Silent
            | PerformanceProfile::Balanced
            | PerformanceProfile::Turbo => ValidationResult::Valid,
        }
    }
}

#[async_trait]
impl FanProvider for MockProvider {
    async fn fan_ids(&self) -> Result<Vec<FanId>, ProviderError> {
        self.read(|s| Ok(s.fans.iter().map(|(f, _)| f.clone()).collect()))
            .await
    }

    async fn fan_rpms(&self) -> Result<Vec<(FanId, Rpm)>, ProviderError> {
        self.read(|s| Ok(s.fans.clone())).await
    }

    async fn fan_curve(
        &self,
        profile: PerformanceProfile,
        fan: &FanId,
    ) -> Result<FanCurve, ProviderError> {
        self.read(|s| {
            s.fan_curves
                .get(&(profile, fan.clone()))
                .cloned()
                .ok_or(MockStateError::Unsupported)
        })
        .await
    }

    async fn set_fan_curve(&self, curve: &FanCurve) -> Result<ApplyResult, ProviderError> {
        self.validate_curve(curve).into_result()?;
        self.mutate(|s| {
            s.fan_curves
                .insert((curve.profile, curve.fan.clone()), curve.clone());
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn set_curves_to_defaults(
        &self,
        profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            for fan in [FanId::Cpu, FanId::Gpu] {
                s.fan_curves
                    .insert((profile, fan.clone()), default_curve(profile, fan));
            }
            Ok(ApplyResult::Applied)
        })
        .await
    }

    fn curve_point_count(&self) -> usize {
        8
    }

    fn allow_decreasing(&self) -> bool {
        false
    }

    fn validate_curve(&self, curve: &FanCurve) -> ValidationResult {
        match curve.validate(self.curve_point_count(), self.allow_decreasing()) {
            Ok(()) => ValidationResult::Valid,
            Err(e) => ValidationResult::Invalid(e.to_string()),
        }
    }
}

#[async_trait]
impl PowerLimitProvider for MockProvider {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        self.read(|s| Ok(s.power_limits.clone())).await
    }

    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<ApplyResult, ProviderError> {
        self.validate_power_limit(&field, value).into_result()?;
        self.mutate(|s| {
            let entry = s
                .power_limits
                .fields
                .get_mut(&field)
                .ok_or(MockStateError::Unsupported)?;
            entry.value = value;
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn restore_defaults(&self) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            for v in s.power_limits.fields.values_mut() {
                if let Some(d) = v.default {
                    v.value = d;
                }
            }
            Ok(ApplyResult::Applied)
        })
        .await
    }

    fn validate_power_limit(&self, field: &PowerLimitField, value: i32) -> ValidationResult {
        // Валидация по метаданным в состоянии.
        if value < 0 {
            return ValidationResult::invalid("значение не может быть отрицательным");
        }
        let _ = field;
        ValidationResult::Valid
    }
}

#[async_trait]
impl BatteryProvider for MockProvider {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        self.read(|s| Ok(s.charge_limit)).await
    }

    async fn set_charge_limit(&self, percent: u8) -> Result<ApplyResult, ProviderError> {
        self.validate_charge_limit(percent).into_result()?;
        self.mutate(|s| {
            s.charge_limit = ChargeLimit::new(
                true,
                Some(Percent::new(percent).expect("range")),
                s.charge_limit.bounds,
            )
            .expect("valid");
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            s.charge_limit.percent = Some(s.charge_limit.bounds.expect("mock bounds").max);
            Ok(ApplyResult::Applied)
        })
        .await
    }

    fn validate_charge_limit(&self, percent: u8) -> ValidationResult {
        if (40..=100).contains(&percent) {
            ValidationResult::Valid
        } else {
            ValidationResult::invalid("лимит зарядки вне диапазона [40, 100]")
        }
    }
}

#[async_trait]
impl GpuProvider for MockProvider {
    async fn requested_mode(&self) -> Result<GpuMode, ProviderError> {
        self.read(|s| Ok(s.gpu_mode)).await
    }

    async fn set_mode(
        &self,
        mode: GpuMode,
        _confirmed: bool,
    ) -> Result<ApplyResult, ProviderError> {
        self.validate_mode(mode).into_result()?;
        self.mutate(|s| {
            let req = Self::requirement_static(mode);
            // Запрошенный режим обновляется всегда.
            s.gpu_mode = mode;
            if req != ActionRequirement::None {
                // Pending (Ultimate/Reboot, Eco/Logout): applied state (MUX,
                // доступ приложений, power) не меняется до отдельного события
                // применения/перезагрузки.
                if mode == GpuMode::Ultimate {
                    let already_pending = s.pending_action.as_ref().map(|a| a.target.as_str())
                        == Some("gpu_mux: ultimate");
                    if !already_pending {
                        s.pending_action = Some(PendingAction {
                            id: "mock-mux".into(),
                            target: "gpu_mux: ultimate".into(),
                            requirement: ActionRequirement::Reboot,
                            cancelable: true,
                            created_by: "mock".into(),
                        });
                    }
                }
                Ok(ApplyResult::Pending { requirement: req })
            } else {
                // Немедленное применение: меняем applied state.
                match mode {
                    GpuMode::Standard => {
                        s.access_policy = GpuAccessPolicy::Unblocked;
                        s.mux = GpuMuxState::Integrated;
                    }
                    GpuMode::Optimized => {
                        s.access_policy = GpuAccessPolicy::Blocked;
                        s.mux = GpuMuxState::Integrated;
                    }
                    _ => {}
                }
                Ok(ApplyResult::Applied)
            }
        })
        .await
    }

    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
        self.read(|s| Ok(s.mux)).await
    }

    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
        self.read(|s| Ok(s.access_policy)).await
    }

    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        self.read(|s| Ok(s.gpu_power_state)).await
    }

    fn requirement_for(&self, mode: GpuMode) -> ActionRequirement {
        Self::requirement_static(mode)
    }

    fn validate_mode(&self, mode: GpuMode) -> ValidationResult {
        let _ = mode;
        ValidationResult::Valid
    }
}

#[async_trait]
impl GpuPowerProvider for MockProvider {
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        // Делегирование существующему mock GPU power state; дублирования нет.
        self.read(|s| Ok(s.gpu_power_state)).await
    }
}

impl MockProvider {
    /// Статическое требование для режима (используется и в set_mode).
    fn requirement_static(mode: GpuMode) -> ActionRequirement {
        match mode {
            GpuMode::Ultimate => ActionRequirement::Reboot,
            GpuMode::Eco => ActionRequirement::Logout,
            _ => ActionRequirement::None,
        }
    }
}

#[async_trait]
impl DisplayProvider for MockProvider {
    async fn display_mode(&self) -> Result<DisplayMode, ProviderError> {
        self.read(|s| Ok(s.display.clone())).await
    }

    async fn set_refresh_rate(&self, hz: RefreshHz) -> Result<ApplyResult, ProviderError> {
        self.validate_refresh_rate(hz).into_result()?;
        self.mutate(|s| {
            s.display.current_hz = Some(hz);
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn set_overdrive(&self, enabled: bool) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            s.display.overdrive = Some(enabled);
            Ok(ApplyResult::Applied)
        })
        .await
    }

    fn validate_refresh_rate(&self, hz: RefreshHz) -> ValidationResult {
        if (30..=360).contains(&hz.get()) {
            ValidationResult::Valid
        } else {
            ValidationResult::invalid("частота вне [30, 360] Гц")
        }
    }
}

#[async_trait]
impl LightingProvider for MockProvider {
    async fn current_mode(&self) -> Result<LightingMode, ProviderError> {
        self.read(|s| Ok(s.lighting.clone())).await
    }

    async fn set_mode(&self, mode: &LightingMode) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            s.lighting = mode.clone();
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn set_brightness(&self, percent: Percent) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            s.telemetry.battery = None; // no-op, просто сохраняем согласованность
            let _ = percent;
            Ok(ApplyResult::Applied)
        })
        .await
    }
}

#[async_trait]
impl AnimeProvider for MockProvider {
    async fn available(&self) -> Result<bool, ProviderError> {
        self.read(|s| {
            Ok(s.mux == GpuMuxState::Discrete || matches!(s.lighting, LightingMode::Rainbow))
        })
        .await
    }

    async fn enabled(&self) -> Result<bool, ProviderError> {
        self.read(|s| Ok(matches!(s.lighting, LightingMode::Rainbow)))
            .await
    }

    async fn set_enabled(&self, enabled: bool) -> Result<ApplyResult, ProviderError> {
        self.mutate(|s| {
            s.lighting = if enabled {
                LightingMode::Rainbow
            } else {
                LightingMode::Off
            };
            Ok(ApplyResult::Applied)
        })
        .await
    }

    async fn set_brightness(&self, percent: Percent) -> Result<ApplyResult, ProviderError> {
        let _ = percent;
        self.mutate(|s| {
            s.ops += 0;
            Ok(ApplyResult::Applied)
        })
        .await
    }
}

#[async_trait]
impl HotkeyProvider for MockProvider {
    async fn bind(&self, action: &str, callback_id: &str) -> Result<OperationId, ProviderError> {
        self.mutate(|_| Ok(())).await?;
        Ok(OperationId(format!("{action}:{callback_id}")))
    }

    async fn unbind(&self, _operation: &OperationId) -> Result<(), ProviderError> {
        self.mutate(|_| Ok(())).await
    }
}

#[async_trait]
impl TelemetryProvider for MockProvider {
    async fn snapshot(&self) -> Result<Telemetry, ProviderError> {
        self.read(|s| Ok(s.telemetry.clone())).await
    }

    fn default_poll_interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}

#[async_trait]
impl AutomationProvider for MockProvider {
    async fn rules(&self) -> Result<Vec<AutomationRule>, ProviderError> {
        Ok(vec![
            AutomationRule::new(
                "ac-profile",
                AutomationTrigger::OnAc,
                AutomationAction::SetProfile(PerformanceProfile::Balanced),
                10,
            ),
            AutomationRule::new(
                "battery-profile",
                AutomationTrigger::OnBattery,
                AutomationAction::SetProfile(PerformanceProfile::Silent),
                10,
            ),
        ])
    }

    async fn apply_rule(&self, rule: &AutomationRule) -> Result<(), ProviderError> {
        if let AutomationAction::SetProfile(p) = &rule.action {
            let _ = self.set_profile(*p).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl FirmwareUpdateProvider for MockProvider {
    async fn check_updates(&self) -> Result<Vec<FirmwareUpdate>, ProviderError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Arc<RwLock<MockState>>, MockProvider) {
        let state = Arc::new(RwLock::new(MockState::full()));
        let provider = MockProvider::new(state.clone());
        (state, provider)
    }

    #[tokio::test]
    async fn profile_set_roundtrip() {
        let (state, provider) = setup();
        assert_eq!(
            provider.current_profile().await.unwrap(),
            PerformanceProfile::Balanced
        );
        let res = provider
            .set_profile(PerformanceProfile::Turbo)
            .await
            .unwrap();
        assert!(res.is_applied());
        assert_eq!(state.read().await.profile, PerformanceProfile::Turbo);
        assert_eq!(state.read().await.ops, 2); // read + write
    }

    #[tokio::test]
    async fn charge_limit_validation() {
        let (_, provider) = setup();
        assert!(provider.set_charge_limit(50).await.is_ok());
        assert!(provider.set_charge_limit(10).await.is_err());
    }

    #[tokio::test]
    async fn initial_gpu_applied_state() {
        let (state, _provider) = setup();
        let s = state.read().await;
        assert_eq!(s.gpu_mode, GpuMode::Standard);
        assert_eq!(s.mux, GpuMuxState::Integrated);
        assert_eq!(s.access_policy, GpuAccessPolicy::Unblocked);
        assert_eq!(s.gpu_power_state, GpuPowerState::Active);
        assert!(s.pending_action.is_none());
    }

    #[tokio::test]
    async fn ultimate_pending_keeps_applied_state() {
        let (state, provider) = setup();
        let res = crate::traits::GpuProvider::set_mode(&provider, GpuMode::Ultimate, true)
            .await
            .unwrap();
        assert!(matches!(
            res,
            ApplyResult::Pending {
                requirement: ActionRequirement::Reboot
            }
        ));
        let s = state.read().await;
        assert_eq!(s.gpu_mode, GpuMode::Ultimate); // requested
        assert!(s.pending_action.is_some());
        assert_eq!(s.mux, GpuMuxState::Integrated); // applied не изменился
        assert_eq!(s.access_policy, GpuAccessPolicy::Unblocked);
        assert_eq!(s.gpu_power_state, GpuPowerState::Active);
    }

    #[tokio::test]
    async fn repeated_ultimate_is_idempotent() {
        let (state, provider) = setup();
        crate::traits::GpuProvider::set_mode(&provider, GpuMode::Ultimate, true)
            .await
            .unwrap();
        let (id_before, mux_before, access_before) = {
            let s = state.read().await;
            let pa = s.pending_action.as_ref().expect("pending");
            (pa.id.clone(), s.mux, s.access_policy)
        };
        let res = crate::traits::GpuProvider::set_mode(&provider, GpuMode::Ultimate, true)
            .await
            .unwrap();
        assert!(matches!(res, ApplyResult::Pending { .. }));
        let s = state.read().await;
        assert_eq!(s.mux, mux_before);
        assert_eq!(s.access_policy, access_before);
        assert_eq!(s.gpu_power_state, GpuPowerState::Active);
        let pa = s.pending_action.as_ref().expect("pending");
        assert_eq!(pa.id, id_before); // OperationId не меняется
    }

    #[tokio::test]
    async fn provider_error_preserves_all_state() {
        let (state, provider) = setup();
        state.write().await.error_mode = MockErrorMode::BackendDown;
        let before = state.read().await.clone();
        let res = crate::traits::GpuProvider::set_mode(&provider, GpuMode::Ultimate, true).await;
        assert!(res.is_err());
        let after = state.read().await;
        assert_eq!(after.gpu_mode, before.gpu_mode);
        assert_eq!(after.mux, before.mux);
        assert_eq!(after.access_policy, before.access_policy);
        assert_eq!(after.gpu_power_state, before.gpu_power_state);
        assert_eq!(after.pending_action, before.pending_action);
    }

    #[tokio::test]
    async fn applied_mode_still_changes_state() {
        let (state, provider) = setup();
        let res = crate::traits::GpuProvider::set_mode(&provider, GpuMode::Optimized, true)
            .await
            .unwrap();
        assert!(matches!(res, ApplyResult::Applied));
        let s = state.read().await;
        assert_eq!(s.gpu_mode, GpuMode::Optimized);
        assert_eq!(s.access_policy, GpuAccessPolicy::Blocked);
        assert_eq!(s.mux, GpuMuxState::Integrated);
    }

    #[tokio::test]
    async fn requested_and_applied_independently_readable() {
        let (_state, provider) = setup();
        crate::traits::GpuProvider::set_mode(&provider, GpuMode::Ultimate, true)
            .await
            .unwrap();
        assert_eq!(
            crate::traits::GpuProvider::requested_mode(&provider)
                .await
                .unwrap(),
            GpuMode::Ultimate
        );
        assert_eq!(
            crate::traits::GpuProvider::mux_state(&provider)
                .await
                .unwrap(),
            GpuMuxState::Integrated
        );
    }

    #[tokio::test]
    async fn backend_down_errors() {
        let (state, provider) = setup();
        state.write().await.error_mode = MockErrorMode::BackendDown;
        assert!(matches!(
            provider.current_profile().await,
            Err(ProviderError::BackendUnavailable(_))
        ));
    }

    #[tokio::test]
    async fn fan_curve_validate() {
        let (_, provider) = setup();
        let mut curve = default_curve(PerformanceProfile::Balanced, FanId::Cpu);
        curve.points[1].temp = orbis_core::newtypes::TemperatureC::new(30).unwrap(); // убывание
        assert!(provider.set_fan_curve(&curve).await.is_err());
    }

    #[tokio::test]
    async fn refresh_rate_validate() {
        let (_, provider) = setup();
        assert!(
            provider
                .set_refresh_rate(RefreshHz::new(144).unwrap())
                .await
                .is_ok()
        );
        assert!(
            provider
                .set_refresh_rate(RefreshHz::new(5).unwrap())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn three_fan_support() {
        let (state, provider) = setup();
        state
            .write()
            .await
            .fans
            .push((FanId::Mid, Rpm::new(1500).unwrap()));
        let fans = provider.fan_ids().await.unwrap();
        assert_eq!(fans.len(), 3);
    }
}
