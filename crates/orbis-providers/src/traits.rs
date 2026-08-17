//! Trait-ы провайдеров.

use std::time::Duration;

use async_trait::async_trait;

use orbis_capabilities::engine::CapabilityPart;
use orbis_core::action::{ActionRequirement, ApplyResult};
use orbis_core::automation::AutomationRule;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::display::DisplayMode;
use orbis_core::fan::FanCurve;
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::lighting::LightingMode;
use orbis_core::limits::{PowerLimitField, PowerLimits};
use orbis_core::profile::PerformanceProfile;
use orbis_core::telemetry::Telemetry;

use crate::error::{OperationId, ProviderError, ValidationResult};

/// Общий базовый trait всех провайдеров.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Стабильный идентификатор провайдера (например, "asusd", "mock").
    fn id(&self) -> &'static str;

    /// Идентичность backend.
    fn backend(&self) -> BackendIdentity;

    /// Таймаут для всех операций этого провайдера.
    fn timeout(&self) -> Duration;

    /// Человекочитаемое объяснение отсутствия поддержки для функции.
    fn explain_unsupported(&self, feature: &str) -> String;

    /// Текущее здоровье backend.
    fn health(&self) -> ProviderHealth;

    /// Диагностические записи.
    fn diagnostics(&self) -> Vec<DiagnosticEntry>;
}

/// Здоровье backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderHealth {
    /// Работает.
    Healthy,
    /// Работает с ограничениями.
    Degraded(String),
    /// Недоступен.
    Unavailable(String),
}

/// Режимы производительности.
#[async_trait]
pub trait PerformanceProvider: Provider {
    /// Список поддерживаемых профилей (в порядке отображения).
    async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError>;

    /// Текущий профиль.
    async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError>;

    /// Установить профиль.
    async fn set_profile(&self, profile: PerformanceProfile) -> Result<ApplyResult, ProviderError>;

    /// Профиль на AC (если поддерживается).
    async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError>;

    /// Профиль на батарее (если поддерживается).
    async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError>;

    /// Валидация запроса установки профиля.
    fn validate_set_profile(&self, profile: PerformanceProfile) -> ValidationResult;
}

/// Вентиляторы и кривые.
#[async_trait]
pub trait FanProvider: Provider {
    /// Список вентиляторов.
    async fn fan_ids(&self) -> Result<Vec<orbis_core::fan::FanId>, ProviderError>;

    /// Текущие RPM.
    async fn fan_rpms(
        &self,
    ) -> Result<Vec<(orbis_core::fan::FanId, orbis_core::newtypes::Rpm)>, ProviderError>;

    /// Кривая для профиля и вентилятора.
    async fn fan_curve(
        &self,
        profile: PerformanceProfile,
        fan: &orbis_core::fan::FanId,
    ) -> Result<FanCurve, ProviderError>;

    /// Активная кривая вентилятора (read-only, без profile).
    ///
    /// Некоторые backends (например, kernel `asus_custom_fan_curve`) хранят
    /// только одну активную кривую, НЕ profile-specific storage. Такой backend
    /// возвращает `Unsupported` из `fan_curve(profile, fan)` (не фальсифицирует
    /// profile semantics) и предоставляет активную кривую через этот метод.
    async fn active_curve(&self, fan: &orbis_core::fan::FanId) -> Result<FanCurve, ProviderError>;

    /// Установить кривую.
    async fn set_fan_curve(&self, curve: &FanCurve) -> Result<ApplyResult, ProviderError>;

    /// Сбросить кривые к заводским.
    async fn set_curves_to_defaults(
        &self,
        profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError>;

    /// Количество точек кривой, требуемое backend.
    fn curve_point_count(&self) -> usize;

    /// Допускает ли backend убывающие значения кривой.
    fn allow_decreasing(&self) -> bool;

    /// Валидация кривой (семантическая, до записи).
    fn validate_curve(&self, curve: &FanCurve) -> ValidationResult;
}

/// Typed fan curve mutation (lossless `AsusdFanProfile`, не `PerformanceProfile`).
///
/// Отдельный от `FanProvider::set_fan_curve` (который использует
/// `PerformanceProfile` и теряет различие Quiet/LowPower). Mutation идёт
/// напрямую к Hardware1 (original caller), не через sessiond.
#[async_trait]
pub trait FanCurveMutationProvider: Provider {
    /// Установить одну fan curve для профиля и вентилятора.
    ///
    /// `curve` — ровно 8 `(TemperatureC, FanPwm)` точек (raw PWM 0..255).
    async fn set_fan_curve(
        &self,
        profile: orbis_core::profile::AsusdFanProfile,
        fan: &orbis_core::fan::FanId,
        curve: &FanCurvePoints,
    ) -> Result<ApplyResult, ProviderError>;
}

/// 8 точек кривой вентилятора (typed, для mutation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanCurvePoints {
    /// 8 температур, °C.
    pub temps: [orbis_core::newtypes::TemperatureC; 8],
    /// 8 raw PWM 0..255.
    pub pwms: [orbis_core::newtypes::FanPwm; 8],
}

/// Power limits.
#[async_trait]
pub trait PowerLimitProvider: Provider {
    /// Доступные поля и их метаданные.
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError>;

    /// Установить значение поля.
    async fn set_power_limit(
        &self,
        field: PowerLimitField,
        value: i32,
    ) -> Result<ApplyResult, ProviderError>;

    /// Сбросить все поля к заводским.
    async fn restore_defaults(&self) -> Result<ApplyResult, ProviderError>;

    /// Валидация значения поля.
    fn validate_power_limit(&self, field: &PowerLimitField, value: i32) -> ValidationResult;
}

/// Батарея (телеметрия + лимит зарядки).
#[async_trait]
pub trait BatteryProvider: Provider {
    /// Лимит зарядки.
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError>;

    /// Установить лимит зарядки.
    async fn set_charge_limit(&self, percent: u8) -> Result<ApplyResult, ProviderError>;

    /// Одноразовая полная зарядка.
    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError>;

    /// Валидация лимита.
    fn validate_charge_limit(&self, percent: u8) -> ValidationResult;
}

/// GPU: три независимые сущности (ADR 0003).
#[async_trait]
pub trait GpuProvider: Provider {
    /// Запрошенный режим.
    async fn requested_mode(&self) -> Result<GpuMode, ProviderError>;

    /// Установить режим.
    async fn set_mode(&self, mode: GpuMode, confirmed: bool) -> Result<ApplyResult, ProviderError>;

    /// Физический MUX.
    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError>;

    /// Доступ приложений к dGPU.
    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError>;

    /// Фактический power state dGPU.
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError>;

    /// Требование для переключения режима.
    fn requirement_for(&self, mode: GpuMode) -> ActionRequirement;

    /// Валидация переключения.
    fn validate_mode(&self, mode: GpuMode) -> ValidationResult;
}

/// Read-only dGPU runtime power capability (отдельный GPU concept, ADR 0003).
///
/// Провайдер, реализующий только эту capability, не обязан предоставлять
/// requested mode / MUX / access policy. Это позволяет независимо подключать
/// runtime power без fake `GpuMode`.
#[async_trait]
pub trait GpuPowerProvider: Provider {
    /// Фактический power state dGPU (чтение не должно будить GPU).
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError>;
}

/// Read-only physical MUX capability (отдельный GPU concept, ADR 0003/0005).
///
/// Провайдер реализует только MUX; не обязан предоставлять product mode,
/// access policy или runtime power.
#[async_trait]
pub trait GpuMuxProvider: Provider {
    /// Физическое состояние MUX (какой GPU обслуживает внутренний дисплей).
    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError>;
}

/// Read-only dGPU access policy capability (отдельный GPU concept, ADR 0003/0005).
///
/// Провайдер реализует только access; не обязан предоставлять product mode,
/// MUX или runtime power.
#[async_trait]
pub trait GpuAccessProvider: Provider {
    /// Политика доступа приложений к dGPU.
    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError>;
}

/// Дисплей.
#[async_trait]
pub trait DisplayProvider: Provider {
    /// Текущее состояние дисплея.
    async fn display_mode(&self) -> Result<DisplayMode, ProviderError>;

    /// Установить частоту.
    async fn set_refresh_rate(
        &self,
        hz: orbis_core::newtypes::RefreshHz,
    ) -> Result<ApplyResult, ProviderError>;

    /// Установить Panel Overdrive (если доступен).
    async fn set_overdrive(&self, enabled: bool) -> Result<ApplyResult, ProviderError>;

    /// Валидация частоты.
    fn validate_refresh_rate(&self, hz: orbis_core::newtypes::RefreshHz) -> ValidationResult;
}

/// Подсветка (клавиатура/Aura).
#[async_trait]
pub trait LightingProvider: Provider {
    /// Текущий режим.
    async fn current_mode(&self) -> Result<LightingMode, ProviderError>;

    /// Установить режим.
    async fn set_mode(&self, mode: &LightingMode) -> Result<ApplyResult, ProviderError>;

    /// Установить яркость.
    async fn set_brightness(
        &self,
        percent: orbis_core::newtypes::Percent,
    ) -> Result<ApplyResult, ProviderError>;
}

/// AniMe Matrix.
#[async_trait]
pub trait AnimeProvider: Provider {
    /// Доступна ли матрица.
    async fn available(&self) -> Result<bool, ProviderError>;

    /// Включена ли.
    async fn enabled(&self) -> Result<bool, ProviderError>;

    /// Включить/выключить.
    async fn set_enabled(&self, enabled: bool) -> Result<ApplyResult, ProviderError>;

    /// Установить яркость.
    async fn set_brightness(
        &self,
        percent: orbis_core::newtypes::Percent,
    ) -> Result<ApplyResult, ProviderError>;
}

/// Горячие клавиши.
#[async_trait]
pub trait HotkeyProvider: Provider {
    /// Зарегистрировать глобальное действие.
    async fn bind(&self, action: &str, callback_id: &str) -> Result<OperationId, ProviderError>;

    /// Отменить регистрацию.
    async fn unbind(&self, operation: &OperationId) -> Result<(), ProviderError>;
}

/// Телеметрия (read-only).
#[async_trait]
pub trait TelemetryProvider: Provider {
    /// Моментальный срез телеметрии.
    async fn snapshot(&self) -> Result<Telemetry, ProviderError>;

    /// Период обновления по умолчанию.
    fn default_poll_interval(&self) -> Duration;
}

/// Автоматизация (правила, применяемые sessiond).
#[async_trait]
pub trait AutomationProvider: Provider {
    /// Список активных правил.
    async fn rules(&self) -> Result<Vec<AutomationRule>, ProviderError>;

    /// Применить правило (вернуть целевое действие для выполнения).
    async fn apply_rule(&self, rule: &AutomationRule) -> Result<(), ProviderError>;
}

/// Обновления прошивки.
#[async_trait]
pub trait FirmwareUpdateProvider: Provider {
    /// Есть ли обновления (без установки).
    async fn check_updates(&self) -> Result<Vec<FirmwareUpdate>, ProviderError>;
}

/// Описание обновления прошивки.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FirmwareUpdate {
    /// Источник.
    pub source: String,
    /// Версия.
    pub version: String,
    /// Требование (reboot и т.п.).
    pub requirement: ActionRequirement,
}

/// Список CapabilityPart-ов для построения матрицы (обёртка для mock/test).
pub trait CapabilitySource {
    /// Части capability-матрицы, предоставляемые этим источником.
    fn capability_parts(&self) -> Vec<CapabilityPart>;
}
