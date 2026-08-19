//! Capability-модель: статусы функций устройства.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::action::ActionRequirement;
use crate::battery::ChargeLimitBounds;
use crate::gpu::GpuMode;
use crate::identity::BackendIdentity;
use crate::limits::{PowerLimitField, Unit};
use crate::profile::PerformanceProfile;

/// Идентификатор функции устройства (словарь capability-матрицы).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureId {
    /// Режимы производительности.
    Performance,
    /// Профиль на AC.
    ProfileOnAc,
    /// Профиль на батарее.
    ProfileOnBattery,
    /// Лимит зарядки.
    ChargeLimit,
    /// Режим зарядки (charge_mode).
    ChargeMode,
    /// Физический MUX.
    GpuMux,
    /// Фактический runtime power state dGPU.
    GpuPower,
    /// Политика доступа приложений к dGPU.
    GpuAccess,
    /// Product GPU policy (Eco/Standard/Ultimate/Optimized).
    GpuProductPolicy,
    /// dgpu_disable (Eco).
    DgpuDisable,
    /// Panel Overdrive.
    PanelOverdrive,
    /// MiniLED backlight mode (device-specific firmware enumeration).
    MiniLed,
    /// Screen auto-brightness toggle (kernel asus-armoury).
    ScreenAutoBrightness,
    /// PPT PL1 (SPL).
    PptPl1Spl,
    /// PPT PL2 (SPPT).
    PptPl2Sppt,
    /// PPT PL3 (FPPT).
    PptFppt,
    /// NVIDIA Dynamic Boost.
    NvDynamicBoost,
    /// GPU temperature target.
    NvTempTarget,
    /// CPU boost (cpufv).
    CpuBoost,
    /// Вентиляторы (телеметрия).
    Fans,
    /// Кривые вентиляторов.
    FanCurves,
    /// Телеметрия батареи.
    BatteryTelemetry,
    /// Подсветка клавиатуры.
    KeyboardBacklight,
    /// Aura RGB.
    Aura,
    /// AniMe Matrix.
    Anime,
    /// Slash Lighting.
    Slash,
    /// Смена частоты дисплея.
    DisplayRefresh,
    /// Текущее состояние outputs (Wayland/compositor read-only).
    DisplayOutput,
    /// Яркость дисплея.
    DisplayBacklight,
    /// Глобальные горячие клавиши.
    Hotkeys,
    /// Автоматизация.
    Automation,
    /// Оверлей.
    Overlay,
}

impl FeatureId {
    /// Все известные функции (для полного прогона probe).
    pub const ALL: &'static [FeatureId] = &[
        FeatureId::Performance,
        FeatureId::ProfileOnAc,
        FeatureId::ProfileOnBattery,
        FeatureId::ChargeLimit,
        FeatureId::ChargeMode,
        FeatureId::GpuMux,
        FeatureId::GpuPower,
        FeatureId::GpuAccess,
        FeatureId::GpuProductPolicy,
        FeatureId::DgpuDisable,
        FeatureId::PanelOverdrive,
        FeatureId::MiniLed,
        FeatureId::ScreenAutoBrightness,
        FeatureId::PptPl1Spl,
        FeatureId::PptPl2Sppt,
        FeatureId::PptFppt,
        FeatureId::NvDynamicBoost,
        FeatureId::NvTempTarget,
        FeatureId::CpuBoost,
        FeatureId::Fans,
        FeatureId::FanCurves,
        FeatureId::BatteryTelemetry,
        FeatureId::KeyboardBacklight,
        FeatureId::Aura,
        FeatureId::Anime,
        FeatureId::Slash,
        FeatureId::DisplayRefresh,
        FeatureId::DisplayOutput,
        FeatureId::DisplayBacklight,
        FeatureId::Hotkeys,
        FeatureId::Automation,
        FeatureId::Overlay,
    ];

    /// Ключ для журналов и диагностики.
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureId::Performance => "performance",
            FeatureId::ProfileOnAc => "profile_on_ac",
            FeatureId::ProfileOnBattery => "profile_on_battery",
            FeatureId::ChargeLimit => "charge_limit",
            FeatureId::ChargeMode => "charge_mode",
            FeatureId::GpuMux => "gpu_mux",
            FeatureId::GpuPower => "gpu_power",
            FeatureId::GpuAccess => "gpu_access",
            FeatureId::GpuProductPolicy => "gpu_product_policy",
            FeatureId::DgpuDisable => "dgpu_disable",
            FeatureId::PanelOverdrive => "panel_overdrive",
            FeatureId::MiniLed => "mini_led",
            FeatureId::ScreenAutoBrightness => "screen_auto_brightness",
            FeatureId::PptPl1Spl => "ppt_pl1_spl",
            FeatureId::PptPl2Sppt => "ppt_pl2_sppt",
            FeatureId::PptFppt => "ppt_fppt",
            FeatureId::NvDynamicBoost => "nv_dynamic_boost",
            FeatureId::NvTempTarget => "nv_temp_target",
            FeatureId::CpuBoost => "cpu_boost",
            FeatureId::Fans => "fans",
            FeatureId::FanCurves => "fan_curves",
            FeatureId::BatteryTelemetry => "battery_telemetry",
            FeatureId::KeyboardBacklight => "keyboard_backlight",
            FeatureId::Aura => "aura",
            FeatureId::Anime => "anime",
            FeatureId::Slash => "slash",
            FeatureId::DisplayRefresh => "display_refresh",
            FeatureId::DisplayOutput => "display_output",
            FeatureId::DisplayBacklight => "display_backlight",
            FeatureId::Hotkeys => "hotkeys",
            FeatureId::Automation => "automation",
            FeatureId::Overlay => "overlay",
        }
    }
}

/// Статус функции (семантика уточнена в `docs/provider-matrix.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    /// Capability или operation доказанно поддерживается.
    Supported,
    /// Поддерживается, но для operation есть явное требование (reboot/logout).
    SupportedWithRequirement,
    /// Read capability существует, но write capability отсутствует или запрещена.
    ReadOnly,
    /// Обычно доступно, но временно недоступно из-за backend/lifecycle state.
    TemporarilyUnavailable,
    /// Capability или operation доказанно не поддерживается контрактом/backend.
    Unsupported,
    /// Потенциальная capability известна, но требуемый backend/service отсутствует.
    BackendMissing,
    /// Capability/operation существует, но текущая authorization evidence отказывает.
    PermissionDenied,
    /// Функция доступна только в экспериментальном режиме.
    Experimental,
    /// Конфликт владельцев интерфейса.
    Conflicted,
    /// Evidence недостаточно для классификации capability/operation.
    Unknown,
}

impl CapabilityStatus {
    /// Человекочитаемое имя (для диагностики; локализация — на уровне UI).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::SupportedWithRequirement => "supported_with_requirement",
            Self::ReadOnly => "read_only",
            Self::TemporarilyUnavailable => "temporarily_unavailable",
            Self::Unsupported => "unsupported",
            Self::BackendMissing => "backend_missing",
            Self::PermissionDenied => "permission_denied",
            Self::Experimental => "experimental",
            Self::Conflicted => "conflicted",
            Self::Unknown => "unknown",
        }
    }
}

/// Уровень риска операции.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Безопасная операция.
    Safe,
    /// Требует подтверждения.
    Confirmation,
    /// Опасная операция.
    Dangerous,
    /// Экспериментальная операция.
    Experimental,
}

/// Причина статуса: техническая + рекомендация для пользователя.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReason {
    /// Техническая причина (может содержать errno).
    pub reason: String,
    /// Рекомендуемое действие.
    pub suggestion: String,
    /// Backend, предоставивший статус.
    pub backend: Option<BackendIdentity>,
    /// D-Bus endpoint или sysfs-путь.
    pub endpoint: Option<String>,
    /// Требование (reboot/logout/...).
    pub requirement: Option<ActionRequirement>,
    /// Уровень риска.
    pub risk: RiskLevel,
    /// Время последней успешной проверки.
    pub checked_at: Option<SystemTime>,
}

/// Статус одной operation capability (read или write).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationCapability {
    /// Канонический статус operation.
    pub status: CapabilityStatus,
    /// Причина статуса, если она известна.
    #[serde(default)]
    pub reason: Option<CapabilityReason>,
}

impl OperationCapability {
    /// Создать operation metadata без дополнительной причины.
    pub fn new(status: CapabilityStatus) -> Self {
        Self {
            status,
            reason: None,
        }
    }

    /// Создать operation metadata с причиной.
    pub fn with_reason(status: CapabilityStatus, reason: CapabilityReason) -> Self {
        Self {
            status,
            reason: Some(reason),
        }
    }
}

/// Независимые read/write semantics одной capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityOperations {
    /// Статус authoritative read operation.
    #[serde(default = "default_unknown_operation")]
    pub read: OperationCapability,
    /// Статус write operation.
    #[serde(default = "default_unknown_operation")]
    pub write: OperationCapability,
}

fn default_unknown_operation() -> OperationCapability {
    OperationCapability::new(CapabilityStatus::Unknown)
}

impl Default for CapabilityOperations {
    fn default() -> Self {
        Self {
            read: default_unknown_operation(),
            write: default_unknown_operation(),
        }
    }
}

/// Typed integer constraints without an observed current value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegerConstraints {
    /// Минимум, если доказан.
    pub min: Option<i32>,
    /// Максимум, если доказан.
    pub max: Option<i32>,
    /// Шаг, если доказан.
    pub step: Option<i32>,
    /// Default, если backend его сообщает.
    #[serde(default)]
    pub default: Option<i32>,
}

/// Typed constraint для одного power-limit поля.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerLimitConstraint {
    /// Поле power-limit.
    pub field: PowerLimitField,
    /// Диапазон и шаг без текущего observed value.
    pub range: IntegerConstraints,
    /// Единица измерения.
    pub unit: Unit,
}

/// Constraints capability без observed hardware state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CapabilityConstraints {
    /// Evidence недостаточно, чтобы определить constraints.
    #[default]
    Unknown,
    /// Capability не имеет дополнительных constraints.
    None,
    /// Battery charge-limit constraints.
    ChargeLimit(ChargeLimitBounds),
    /// Generic integer range.
    Integer(IntegerConstraints),
    /// Typed Performance choices.
    PerformanceProfiles(Vec<PerformanceProfile>),
    /// Typed product GPU choices; наличие choices не доказывает backend.
    GpuModes(Vec<GpuMode>),
    /// Typed power-limit metadata.
    PowerLimits(Vec<PowerLimitConstraint>),
}

/// Capability одной функции.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Статус.
    pub status: CapabilityStatus,
    /// Причина (обязательна при статусе != Supported).
    #[serde(default)]
    pub reason: Option<CapabilityReason>,
    /// Независимые read/write operation statuses.
    #[serde(default)]
    pub operations: CapabilityOperations,
    /// Typed constraints; `Unknown` не равен `None`.
    #[serde(default)]
    pub constraints: CapabilityConstraints,
}

impl Capability {
    /// Создать capability с пустой причиной.
    pub fn new(status: CapabilityStatus) -> Self {
        Self {
            status,
            reason: None,
            operations: CapabilityOperations::default(),
            constraints: CapabilityConstraints::Unknown,
        }
    }

    /// Создать capability с причиной.
    pub fn with_reason(status: CapabilityStatus, reason: CapabilityReason) -> Self {
        Self {
            status,
            reason: Some(reason),
            operations: CapabilityOperations::default(),
            constraints: CapabilityConstraints::Unknown,
        }
    }

    /// Добавить operation-level read/write metadata.
    pub fn with_operations(mut self, operations: CapabilityOperations) -> Self {
        self.operations = operations;
        self
    }

    /// Добавить typed constraints без помещения observed value в capability.
    pub fn with_constraints(mut self, constraints: CapabilityConstraints) -> Self {
        self.constraints = constraints;
        self
    }
}

/// Матрица возможностей устройства.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    /// Функция -> capability.
    pub features: BTreeMap<FeatureId, Capability>,
}

impl DeviceCapabilities {
    /// Получить статус функции.
    pub fn status(&self, feature: FeatureId) -> CapabilityStatus {
        self.features
            .get(&feature)
            .map(|c| c.status)
            .unwrap_or(CapabilityStatus::Unknown)
    }

    /// Полная причина по функции.
    pub fn reason(&self, feature: FeatureId) -> Option<&CapabilityReason> {
        self.features.get(&feature).and_then(|c| c.reason.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_by_default() {
        let caps = DeviceCapabilities::default();
        assert_eq!(caps.status(FeatureId::Anime), CapabilityStatus::Unknown);
    }

    #[test]
    fn status_roundtrip() {
        for s in [
            CapabilityStatus::Supported,
            CapabilityStatus::ReadOnly,
            CapabilityStatus::Unsupported,
            CapabilityStatus::PermissionDenied,
            CapabilityStatus::Unknown,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: CapabilityStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn feature_id_roundtrip() {
        let json = serde_json::to_string(&FeatureId::GpuMux).unwrap();
        assert_eq!(json, "\"gpu_mux\"");
        let back: FeatureId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, FeatureId::GpuMux);

        assert_eq!(
            serde_json::to_string(&FeatureId::DgpuDisable).unwrap(),
            "\"dgpu_disable\""
        );
        assert_eq!(
            serde_json::to_string(&FeatureId::GpuPower).unwrap(),
            "\"gpu_power\""
        );
        assert_eq!(
            serde_json::to_string(&FeatureId::GpuAccess).unwrap(),
            "\"gpu_access\""
        );
        assert_eq!(
            serde_json::to_string(&FeatureId::GpuProductPolicy).unwrap(),
            "\"gpu_product_policy\""
        );
        assert_eq!(
            serde_json::to_string(&FeatureId::MiniLed).unwrap(),
            "\"mini_led\""
        );
        assert_eq!(FeatureId::MiniLed.as_str(), "mini_led");
        assert_eq!(
            serde_json::to_string(&FeatureId::ScreenAutoBrightness).unwrap(),
            "\"screen_auto_brightness\""
        );
        assert_eq!(
            FeatureId::ScreenAutoBrightness.as_str(),
            "screen_auto_brightness"
        );
        assert_eq!(
            serde_json::to_string(&FeatureId::DisplayOutput).unwrap(),
            "\"display_output\""
        );
        assert_eq!(FeatureId::DisplayOutput.as_str(), "display_output");
    }

    #[test]
    fn matrix_insert() {
        let mut caps = DeviceCapabilities::default();
        caps.features.insert(
            FeatureId::GpuMux,
            Capability::with_reason(
                CapabilityStatus::SupportedWithRequirement,
                CapabilityReason {
                    reason: "mux latched by firmware".into(),
                    suggestion: "reboot".into(),
                    backend: None,
                    endpoint: Some("/sys/devices/platform/asus-nb-wmi/gpu_mux_mode".into()),
                    requirement: Some(ActionRequirement::Reboot),
                    risk: RiskLevel::Confirmation,
                    checked_at: None,
                },
            ),
        );
        assert_eq!(
            caps.status(FeatureId::GpuMux),
            CapabilityStatus::SupportedWithRequirement
        );
        assert_eq!(
            caps.reason(FeatureId::GpuMux).unwrap().requirement,
            Some(ActionRequirement::Reboot)
        );
    }

    #[test]
    fn capability_status_semantics_roundtrip() {
        for status in [
            CapabilityStatus::Supported,
            CapabilityStatus::SupportedWithRequirement,
            CapabilityStatus::ReadOnly,
            CapabilityStatus::TemporarilyUnavailable,
            CapabilityStatus::Unsupported,
            CapabilityStatus::BackendMissing,
            CapabilityStatus::PermissionDenied,
            CapabilityStatus::Experimental,
            CapabilityStatus::Conflicted,
            CapabilityStatus::Unknown,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: CapabilityStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn operation_statuses_are_independent() {
        let capability =
            Capability::new(CapabilityStatus::Supported).with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(CapabilityStatus::Unsupported),
            });
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );

        let denied =
            Capability::new(CapabilityStatus::Supported).with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(CapabilityStatus::PermissionDenied),
            });
        assert_eq!(
            denied.operations.write.status,
            CapabilityStatus::PermissionDenied
        );

        let unsupported =
            Capability::new(CapabilityStatus::Unsupported).with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Unsupported),
                write: OperationCapability::new(CapabilityStatus::Unsupported),
            });
        assert_eq!(
            unsupported.operations.read.status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            unsupported.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn unknown_constraints_are_not_none_constraints() {
        assert_ne!(CapabilityConstraints::Unknown, CapabilityConstraints::None);

        let bounds = ChargeLimitBounds::new(
            crate::newtypes::Percent::new(40).unwrap(),
            crate::newtypes::Percent::new(100).unwrap(),
            1,
        )
        .unwrap();
        let capability = Capability::new(CapabilityStatus::Supported)
            .with_constraints(CapabilityConstraints::ChargeLimit(bounds));
        assert_eq!(
            capability.constraints,
            CapabilityConstraints::ChargeLimit(bounds)
        );
    }

    #[test]
    fn old_capability_json_deserializes_with_default_metadata() {
        let old_json = r#"{"status":"supported","reason":null}"#;
        let capability: Capability = serde_json::from_str(old_json).unwrap();
        assert_eq!(capability.status, CapabilityStatus::Supported);
        assert_eq!(capability.operations.read.status, CapabilityStatus::Unknown);
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unknown
        );
        assert_eq!(capability.constraints, CapabilityConstraints::Unknown);
    }
}
