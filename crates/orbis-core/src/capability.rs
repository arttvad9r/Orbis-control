//! Capability-модель: статусы функций устройства.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::action::ActionRequirement;
use crate::identity::BackendIdentity;

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
    /// dgpu_disable (Eco).
    DgpuDisable,
    /// Panel Overdrive.
    PanelOverdrive,
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
        FeatureId::DgpuDisable,
        FeatureId::PanelOverdrive,
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
            FeatureId::DgpuDisable => "dgpu_disable",
            FeatureId::PanelOverdrive => "panel_overdrive",
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
    /// Поддерживается и работает.
    Supported,
    /// Поддерживается, но есть требование (reboot/logout и т.п.).
    SupportedWithRequirement,
    /// Значение достоверно читается, запись отсутствует или запрещена.
    ReadOnly,
    /// Может стать доступной без изменения программы/оборудования.
    TemporarilyUnavailable,
    /// Поддержка достоверно опровергнута (ENODEV/ENOTSUP/EOPNOTSUPP).
    Unsupported,
    /// Backend отсутствует (сервис не установлен/не запущен).
    BackendMissing,
    /// Ядро/D-Bus отклоняет операцию из-за прав.
    PermissionDenied,
    /// Функция доступна только в экспериментальном режиме.
    Experimental,
    /// Конфликт владельцев интерфейса.
    Conflicted,
    /// Недостаточно информации.
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

/// Capability одной функции.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Статус.
    pub status: CapabilityStatus,
    /// Причина (обязательна при статусе != Supported).
    pub reason: Option<CapabilityReason>,
}

impl Capability {
    /// Создать capability с пустой причиной.
    pub fn new(status: CapabilityStatus) -> Self {
        Self { status, reason: None }
    }

    /// Создать capability с причиной.
    pub fn with_reason(status: CapabilityStatus, reason: CapabilityReason) -> Self {
        Self { status, reason: Some(reason) }
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
        assert_eq!(caps.status(FeatureId::GpuMux), CapabilityStatus::SupportedWithRequirement);
        assert_eq!(caps.reason(FeatureId::GpuMux).unwrap().requirement, Some(ActionRequirement::Reboot));
    }
}
