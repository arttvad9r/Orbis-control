//! Телеметрия и аппаратный снапшот.
//!
//! `HardwareSnapshot` — типизированная структура, НЕ универсальный JSON-словарь.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::battery::{ChargeLimit, ChargeLimitBounds};
use crate::display::DisplayMode;
use crate::fan::FanId;
use crate::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use crate::newtypes::{EnergyMWh, MilliWatt, Percent, Rpm, TemperatureC};
use crate::profile::PerformanceProfile;
use crate::warning::Warning;

/// Телеметрия одного вентилятора.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanTelemetry {
    /// Source/backend that produced this RPM observation.
    #[serde(default)]
    pub source: String,
    /// Идентификатор.
    pub fan: FanId,
    /// Original backend label, when available.
    #[serde(default)]
    pub label: String,
    /// RPM.
    pub rpm: Rpm,
    /// Процент от максимума (если доступен).
    pub percent: Option<Percent>,
    /// Quality of this individual fan observation.
    #[serde(default)]
    pub quality: FanTelemetryQuality,
}

/// Quality of one fan RPM observation, independent from curve capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FanTelemetryQuality {
    /// Source, identity/label and RPM are all known.
    #[default]
    Complete,
    /// The RPM is usable but some source metadata is incomplete.
    Partial,
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
    /// Потребление dGPU, мВт (из hwmon amdgpu `power1_input`).
    ///
    /// Это telemetry-значение (потребление в ваттах) и НЕ является
    /// `GpuPowerState` (Active/Suspended/Off) — capability state живёт в
    /// отдельном `GpuPowerProvider`.
    pub gpu: Option<MilliWatt>,
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
    /// Подключён ли AC-адаптер (из `power_supply` `online`).
    pub ac_online: Option<bool>,
    /// Батарея.
    pub battery: Option<BatteryTelemetry>,
    /// Фактический power state dGPU.
    pub gpu_power_state: GpuPowerState,
    /// Отметка времени.
    pub ts: SystemTime,
}

/// Quality of the data fields in one telemetry snapshot.
///
/// This is deliberately independent from backend availability and sample
/// freshness: a responding backend may return `Partial` or `Empty` data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryQuality {
    /// All expected top-level telemetry groups contain useful data.
    Complete,
    /// Some useful telemetry is present, but one or more groups are absent.
    Partial,
    /// The provider returned successfully but no useful telemetry fields exist.
    Empty,
    /// A previously successful sample is retained but is no longer fresh.
    Stale,
    /// No usable sample exists because collection failed.
    Failed,
}

impl Telemetry {
    /// Пустой снапшот телеметрии.
    pub fn empty() -> Self {
        Self {
            cpu_temp: None,
            gpu_temp: None,
            fans: Vec::new(),
            power: PowerTelemetry::default(),
            ac_online: None,
            battery: None,
            gpu_power_state: GpuPowerState::Unknown,
            ts: SystemTime::now(),
        }
    }

    /// Classify the useful data present in this snapshot without considering
    /// backend availability or age.
    pub fn quality(&self) -> TelemetryQuality {
        let has_power = self.power.ac.is_some()
            || self.power.battery.is_some()
            || self.power.total.is_some()
            || self.power.gpu.is_some();
        let has_any = self.cpu_temp.is_some()
            || self.gpu_temp.is_some()
            || !self.fans.is_empty()
            || has_power
            || self.ac_online.is_some()
            || self.battery.is_some()
            || !matches!(self.gpu_power_state, GpuPowerState::Unknown);

        if !has_any {
            return TelemetryQuality::Empty;
        }

        let complete = self.cpu_temp.is_some()
            && self.gpu_temp.is_some()
            && !self.fans.is_empty()
            && has_power
            && self.ac_online.is_some()
            && self.battery.is_some()
            && !matches!(self.gpu_power_state, GpuPowerState::Unknown);

        if complete {
            TelemetryQuality::Complete
        } else {
            TelemetryQuality::Partial
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
                None,
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
        assert_eq!(s.charge_limit.bounds.unwrap().min.get(), 40);
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

    #[test]
    fn snapshot_roundtrip_preserves_partial_telemetry() {
        let mut t = Telemetry::empty();
        t.cpu_temp = Some(TemperatureC::new(46).unwrap());
        // GPU temp unavailable, fans empty, battery partial.
        t.battery = Some(BatteryTelemetry {
            percent: Percent::new(80).unwrap(),
            capacity: None,
            energy_now: None,
            energy_full: None,
            charge_cycles: None,
            state: "Discharging".into(),
        });
        t.ac_online = Some(false);

        let s = HardwareSnapshot {
            profile: PerformanceProfile::Balanced,
            gpu: GpuStatus {
                requested_mode: GpuMode::Standard,
                mux: GpuMuxState::Unknown,
                access_policy: GpuAccessPolicy::Unknown,
            },
            charge_limit: ChargeLimit::new(
                false,
                Some(Percent::new(80).unwrap()),
                Some(Percent::new(80).unwrap()),
                Some(
                    ChargeLimitBounds::new(
                        Percent::new(40).unwrap(),
                        Percent::new(100).unwrap(),
                        1,
                    )
                    .unwrap(),
                ),
            )
            .unwrap(),
            display: DisplayMode::default(),
            telemetry: t,
            warnings: Vec::new(),
        };

        let json = serde_json::to_string(&s).unwrap();
        let back: HardwareSnapshot = serde_json::from_str(&json).unwrap();

        // Partial telemetry preserved.
        assert_eq!(
            back.telemetry.cpu_temp,
            Some(TemperatureC::new(46).unwrap())
        );
        assert_eq!(back.telemetry.gpu_temp, None);
        assert!(back.telemetry.fans.is_empty());
        assert_eq!(back.telemetry.ac_online, Some(false));
        let b = back.telemetry.battery.unwrap();
        assert_eq!(b.percent.get(), 80);
        assert_eq!(b.capacity, None);
        assert_eq!(b.charge_cycles, None);
        assert_eq!(b.state, "Discharging");
    }

    // -----------------------------------------------------------------------
    // Telemetry roundtrip tests
    // -----------------------------------------------------------------------

    #[test]
    fn telemetry_roundtrip_preserves_all_values() {
        let t = Telemetry {
            cpu_temp: Some(TemperatureC::new(46).unwrap()),
            gpu_temp: Some(TemperatureC::new(43).unwrap()),
            fans: vec![
                FanTelemetry {
                    source: "test".into(),
                    fan: FanId::Cpu,
                    label: "cpu_fan".into(),
                    rpm: Rpm::new(2600).unwrap(),
                    percent: None,
                    quality: FanTelemetryQuality::Complete,
                },
                FanTelemetry {
                    source: "test".into(),
                    fan: FanId::Gpu,
                    label: "gpu_fan".into(),
                    rpm: Rpm::new(2100).unwrap(),
                    percent: None,
                    quality: FanTelemetryQuality::Complete,
                },
            ],
            power: PowerTelemetry {
                ac: None,
                battery: None,
                total: None,
                gpu: Some(MilliWatt::new(13_073).unwrap()),
            },
            ac_online: Some(true),
            battery: Some(BatteryTelemetry {
                percent: Percent::new(100).unwrap(),
                capacity: Some(Percent::new(87).unwrap()),
                energy_now: None,
                energy_full: None,
                charge_cycles: Some(5),
                state: "Full".into(),
            }),
            gpu_power_state: GpuPowerState::Unknown,
            ts: SystemTime::UNIX_EPOCH,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn cpu_temp_none_roundtrip() {
        let mut t = Telemetry::empty();
        t.cpu_temp = None;
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cpu_temp, None);
    }

    #[test]
    fn gpu_power_none_roundtrip() {
        let mut t = Telemetry::empty();
        t.power.gpu = None;
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.power.gpu, None);
    }

    #[test]
    fn ac_online_false_roundtrip() {
        let mut t = Telemetry::empty();
        t.ac_online = Some(false);
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ac_online, Some(false));
    }

    #[test]
    fn ac_online_none_roundtrip() {
        let mut t = Telemetry::empty();
        t.ac_online = None;
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ac_online, None);
    }

    #[test]
    fn fan_rpm_zero_roundtrip() {
        let mut t = Telemetry::empty();
        t.fans = vec![FanTelemetry {
            source: "test".into(),
            fan: FanId::Cpu,
            label: "cpu_fan".into(),
            rpm: Rpm::new(0).unwrap(),
            percent: None,
            quality: FanTelemetryQuality::Complete,
        }];
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fans.len(), 1);
        assert_eq!(back.fans[0].rpm.get(), 0);
        assert_eq!(back.fans[0].fan, FanId::Cpu);
    }

    #[test]
    fn single_gpu_fan_identity_preserved() {
        // Only GPU fan present → identity must remain Gpu, not renumbered.
        let mut t = Telemetry::empty();
        t.fans = vec![FanTelemetry {
            source: "test".into(),
            fan: FanId::Gpu,
            label: "gpu_fan".into(),
            rpm: Rpm::new(3200).unwrap(),
            percent: None,
            quality: FanTelemetryQuality::Complete,
        }];
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fans.len(), 1);
        assert_eq!(back.fans[0].fan, FanId::Gpu);
        assert_eq!(back.fans[0].rpm.get(), 3200);
    }

    #[test]
    fn mixed_fans_preserve_identity() {
        let mut t = Telemetry::empty();
        t.fans = vec![
            FanTelemetry {
                source: "test".into(),
                fan: FanId::Gpu,
                label: "gpu_fan".into(),
                rpm: Rpm::new(3200).unwrap(),
                percent: None,
                quality: FanTelemetryQuality::Complete,
            },
            FanTelemetry {
                source: "test".into(),
                fan: FanId::Other("custom".into()),
                label: "custom".into(),
                rpm: Rpm::new(1500).unwrap(),
                percent: None,
                quality: FanTelemetryQuality::Complete,
            },
        ];
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fans.len(), 2);
        assert_eq!(back.fans[0].fan, FanId::Gpu);
        assert_eq!(back.fans[1].fan, FanId::Other("custom".to_string()));
    }

    #[test]
    fn battery_optional_metadata_none_roundtrip() {
        let mut t = Telemetry::empty();
        t.battery = Some(BatteryTelemetry {
            percent: Percent::new(80).unwrap(),
            capacity: None,
            energy_now: None,
            energy_full: None,
            charge_cycles: None,
            state: "Discharging".into(),
        });
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        let b = back.battery.unwrap();
        assert_eq!(b.percent.get(), 80);
        assert_eq!(b.capacity, None);
        assert_eq!(b.charge_cycles, None);
        assert_eq!(b.state, "Discharging");
    }

    #[test]
    fn numeric_edge_values_roundtrip() {
        let mut t = Telemetry::empty();
        t.cpu_temp = Some(TemperatureC::new(-50).unwrap());
        t.gpu_temp = Some(TemperatureC::new(150).unwrap());
        t.power.gpu = Some(MilliWatt::new(10_000_000).unwrap());
        t.fans = vec![FanTelemetry {
            source: "test".into(),
            fan: FanId::Cpu,
            label: "cpu_fan".into(),
            rpm: Rpm::new(65535).unwrap(),
            percent: Some(Percent::new(100).unwrap()),
            quality: FanTelemetryQuality::Complete,
        }];
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cpu_temp, Some(TemperatureC::new(-50).unwrap()));
        assert_eq!(back.gpu_temp, Some(TemperatureC::new(150).unwrap()));
        assert_eq!(back.power.gpu, Some(MilliWatt::new(10_000_000).unwrap()));
        assert_eq!(back.fans[0].rpm.get(), 65535);
    }

    #[test]
    fn partial_telemetry_survives_roundtrip() {
        // Partial: only cpu_temp and battery present, rest None.
        let t = Telemetry {
            cpu_temp: Some(TemperatureC::new(46).unwrap()),
            gpu_temp: None,
            fans: vec![],
            power: PowerTelemetry::default(),
            ac_online: None,
            battery: Some(BatteryTelemetry {
                percent: Percent::new(80).unwrap(),
                capacity: None,
                energy_now: None,
                energy_full: None,
                charge_cycles: None,
                state: "Discharging".into(),
            }),
            gpu_power_state: GpuPowerState::Unknown,
            ts: SystemTime::UNIX_EPOCH,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // Verify None fields are preserved, not collapsed.
        assert!(back.fans.is_empty());
        assert_eq!(back.ac_online, None);
        assert_eq!(back.power.gpu, None);
    }

    #[test]
    fn malformed_json_does_not_become_valid_default() {
        // Malformed JSON for numeric fields should fail to parse, not produce
        // a default valid value.
        let result: Result<Telemetry, _> =
            serde_json::from_str(r#"{"cpu_temp":"not_a_number","fans":[],"power":{}}"#);
        assert!(
            result.is_err(),
            "malformed JSON must not produce valid Telemetry"
        );
    }
}
