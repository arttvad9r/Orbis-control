//! Телеметрия и аппаратный снапшот.
//!
//! `HardwareSnapshot` — типизированная структура, НЕ универсальный JSON-словарь.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::battery::ChargeLimit;
use crate::display::DisplayMode;
use crate::fan::FanId;
use crate::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use crate::newtypes::{EnergyMWh, MilliWatt, Percent, Rpm, TemperatureC};
use crate::profile::PerformanceProfile;
use crate::warning::Warning;

/// Телеметрия одного вентилятора.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanTelemetry {
    /// Идентификатор.
    pub fan: FanId,
    /// RPM.
    pub rpm: Rpm,
    /// Процент от максимума (если доступен).
    pub percent: Option<Percent>,
}

/// Телеметрия мощности.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerTelemetry {
    /// Потребление от AC, мВт.
    pub ac: Option<MilliWatt>,
    /// Потребление/заряд батареи, мВт.
    pub battery: Option<MilliWatt>,
    /// Суммарная мощность, мВт.
    pub total: Option<MilliWatt>,
}

/// Телеметрия батареи.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryTelemetry {
    /// Уровень, %.
    pub percent: Percent,
    /// Емкость (здоровье), % от design.
    pub capacity: Option<Percent>,
    /// Энергия сейчас, мВт·ч.
    pub energy_now: Option<EnergyMWh>,
    /// Энергия при полном заряде, мВт·ч.
    pub energy_full: Option<EnergyMWh>,
    /// Число циклов (если доступно).
    pub charge_cycles: Option<u32>,
    /// Состояние (charging / discharging / full / ...).
    pub state: String,
}

/// Моментальный срез телеметрии.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Telemetry {
    /// Температура CPU, °C.
    pub cpu_temp: Option<TemperatureC>,
    /// Температура dGPU, °C (может быть устаревшей — см. power_state).
    pub gpu_temp: Option<TemperatureC>,
    /// Вентиляторы.
    pub fans: Vec<FanTelemetry>,
    /// Мощности.
    pub power: PowerTelemetry,
    /// Батарея.
    pub battery: Option<BatteryTelemetry>,
    /// Фактический power state dGPU.
    pub gpu_power_state: GpuPowerState,
    /// Отметка времени.
    pub ts: SystemTime,
}

impl Telemetry {
    /// Пустой снапшот телеметрии.
    pub fn empty() -> Self {
        Self {
            cpu_temp: None,
            gpu_temp: None,
            fans: Vec::new(),
            power: PowerTelemetry::default(),
            battery: None,
            gpu_power_state: GpuPowerState::Unknown,
            ts: SystemTime::now(),
        }
    }
}

/// Состояние GPU для снапшота.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuStatus {
    /// Запрошенный режим (желаемая policy).
    pub requested_mode: GpuMode,
    /// Физический MUX.
    pub mux: GpuMuxState,
    /// Доступ приложений к dGPU.
    pub access_policy: GpuAccessPolicy,
}

/// Полный аппаратный снапшот (агрегированное состояние для UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareSnapshot {
    /// Профиль производительности (текущий).
    pub profile: PerformanceProfile,
    /// Состояние GPU.
    pub gpu: GpuStatus,
    /// Лимит зарядки.
    pub charge_limit: ChargeLimit,
    /// Состояние дисплея.
    pub display: DisplayMode,
    /// Телеметрия.
    pub telemetry: Telemetry,
    /// Предупреждения.
    pub warnings: Vec<Warning>,
}

impl HardwareSnapshot {
    /// Пустой снапшот (для mock `--read-only-empty`).
    pub fn empty(profile: PerformanceProfile) -> Self {
        Self {
            profile,
            gpu: GpuStatus {
                requested_mode: GpuMode::Standard,
                mux: GpuMuxState::Unknown,
                access_policy: GpuAccessPolicy::Unknown,
            },
            charge_limit: ChargeLimit::new(
                false,
                None,
                Percent::new(40).expect("const"),
                Percent::new(100).expect("const"),
                1,
            )
            .expect("valid"),
            display: DisplayMode::default(),
            telemetry: Telemetry::empty(),
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_is_valid() {
        let s = HardwareSnapshot::empty(PerformanceProfile::Balanced);
        assert_eq!(s.profile, PerformanceProfile::Balanced);
        assert!(s.warnings.is_empty());
        assert_eq!(s.charge_limit.min.get(), 40);
    }

    #[test]
    fn telemetry_empty() {
        let t = Telemetry::empty();
        assert!(t.fans.is_empty());
        assert!(t.cpu_temp.is_none());
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let s = HardwareSnapshot::empty(PerformanceProfile::Turbo);
        let json = serde_json::to_string(&s).unwrap();
        let back: HardwareSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
