//! Определения mock-профилей устройств.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use orbis_core::display::{DisplayMode, HdrState, PanelOverdriveState, RefreshMode};
use orbis_core::fan::{FanCurve, FanCurvePoint, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
use orbis_core::newtypes::{FanPwm, Percent, RefreshHz, Rpm, TemperatureC};
use orbis_core::profile::PerformanceProfile;
use orbis_core::telemetry::{BatteryTelemetry, FanTelemetry, PowerTelemetry, Telemetry};
use orbis_providers::mock::{MockErrorMode, MockState};

/// Имя профиля (canonical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum ProfileName {
    /// Zephyrus с полным набором функций.
    zephyrus_full,
    /// TUF FA707NV — реалистичный, по обезличенной фикстуре.
    tuf_fa707nv_realistic,
    /// TUF без AniMe.
    tuf_no_anime,
    /// Ноутбук без MUX.
    device_no_mux,
    /// Устройство без asusd (только kernel ABI / UPower).
    device_no_asusd,
    /// Read-only sysfs (запись запрещена).
    device_read_only,
    /// Устройство с тремя вентиляторами.
    device_three_fans,
    /// Не-ASUS устройство.
    non_asus,
    /// Backend-сбой (сервисы недоступны).
    backend_failure,
    /// Pending reboot (MUX запрошен, не применён).
    pending_reboot,
    /// Permission denied.
    permission_denied,
}

impl ProfileName {
    /// Все имена.
    pub const ALL: &'static [ProfileName] = &[
        ProfileName::zephyrus_full,
        ProfileName::tuf_fa707nv_realistic,
        ProfileName::tuf_no_anime,
        ProfileName::device_no_mux,
        ProfileName::device_no_asusd,
        ProfileName::device_read_only,
        ProfileName::device_three_fans,
        ProfileName::non_asus,
        ProfileName::backend_failure,
        ProfileName::pending_reboot,
        ProfileName::permission_denied,
    ];

    /// Разобрать из строки.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.as_str() == s)
    }

    /// Имя для CLI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::zephyrus_full => "zephyrus-full",
            Self::tuf_fa707nv_realistic => "tuf-fa707nv-realistic",
            Self::tuf_no_anime => "tuf-no-anime",
            Self::device_no_mux => "device-no-mux",
            Self::device_no_asusd => "device-no-asusd",
            Self::device_read_only => "device-read-only",
            Self::device_three_fans => "device-three-fans",
            Self::non_asus => "non-asus",
            Self::backend_failure => "backend-failure",
            Self::pending_reboot => "pending-reboot",
            Self::permission_denied => "permission-denied",
        }
    }
}

/// Описание mock-устройства: как строить `MockState`.
#[derive(Debug, Clone)]
pub struct MockDeviceProfile {
    /// Имя.
    pub name: ProfileName,
    /// Человекочитаемое описание.
    pub description: &'static str,
    /// Строитель состояния.
    pub builder: fn() -> MockState,
}

/// Кривая по умолчанию (8 точек).
pub fn default_curve(profile: PerformanceProfile, fan: FanId) -> FanCurve {
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
        FanCurvePoint::new(
            TemperatureC::new(base + t - 45).expect("temp"),
            FanPwm::new(p).expect("pwm"),
        )
    })
    .collect();
    FanCurve {
        profile,
        fan,
        points,
    }
}

/// Заполнить кривые для списка вентиляторов и профилей.
fn fill_curves(state: &mut MockState, fans: &[FanId]) {
    for p in PerformanceProfile::ALL {
        for fan in fans {
            state
                .fan_curves
                .insert((p, fan.clone()), default_curve(p, fan.clone()));
        }
    }
}

/// Телеметрия по умолчанию.
fn telemetry(cpu: i16, gpu: i16, fans: Vec<(FanId, Rpm)>, battery: bool) -> Telemetry {
    Telemetry {
        cpu_temp: Some(TemperatureC::new(cpu).expect("temp")),
        gpu_temp: Some(TemperatureC::new(gpu).expect("temp")),
        fans: fans
            .into_iter()
            .map(|(fan, rpm)| FanTelemetry {
                fan,
                rpm,
                percent: Some(Percent::new(40).expect("pwm")),
            })
            .collect(),
        power: PowerTelemetry {
            ac: orbis_core::newtypes::MilliWatt::new(28_000).ok(),
            battery: if battery {
                orbis_core::newtypes::MilliWatt::new(9_500).ok()
            } else {
                None
            },
            total: None,
            gpu: None,
        },
        ac_online: Some(true),
        battery: if battery {
            Some(BatteryTelemetry {
                percent: Percent::new(80).expect("pct"),
                capacity: Some(Percent::new(89).expect("pct")),
                energy_now: Some(orbis_core::newtypes::EnergyMWh::new(76_350).expect("mwh")),
                energy_full: Some(orbis_core::newtypes::EnergyMWh::new(80_140).expect("mwh")),
                charge_cycles: Some(0),
                state: "discharging".into(),
            })
        } else {
            None
        },
        gpu_power_state: GpuPowerState::Active,
        // Фиксированный timestamp: профили должны создаваться детерминированно
        // (для snapshot-тестов), поэтому НЕ используем SystemTime::now().
        ts: std::time::SystemTime::UNIX_EPOCH,
    }
}

/// Базовый «полный» профиль (zephyrus-like).
fn base_full() -> MockState {
    let mut s = MockState::full();
    s.fans = vec![
        (FanId::Cpu, Rpm::new(2400).expect("rpm")),
        (FanId::Gpu, Rpm::new(2600).expect("rpm")),
    ];
    fill_curves(&mut s, &[FanId::Cpu, FanId::Gpu]);
    s.power_limits = PowerLimits {
        fields: BTreeMap::from([
            (
                PowerLimitField::Spl,
                PowerLimitValue::new(45, 20, 80, 5, Some(45), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::Sppt,
                PowerLimitValue::new(60, 20, 100, 5, Some(60), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::Fppt,
                PowerLimitValue::new(70, 20, 110, 5, Some(70), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::GpuTempTarget,
                PowerLimitValue::new(75, 60, 87, 1, Some(75), Unit::DegreesC).expect("pl"),
            ),
        ]),
    };
    s.telemetry = telemetry(72, 65, s.fans.clone(), true);
    s
}

fn zephyrus_full() -> MockState {
    let mut s = base_full();
    s.display = DisplayMode {
        current_hz: Some(RefreshHz::new(165).expect("hz")),
        modes: vec![
            RefreshMode::new(RefreshHz::new(60).expect("hz")),
            RefreshMode::new(RefreshHz::new(120).expect("hz")),
            RefreshMode::new(RefreshHz::new(165).expect("hz")),
        ],
        overdrive: PanelOverdriveState::Enabled,
        hdr: HdrState::Supported,
    };
    s.mux = GpuMuxState::Integrated;
    s
}

fn tuf_fa707nv_realistic() -> MockState {
    // Реалистичный профиль по обезличенной фикстуре FA707NV:
    // 2 вентилятора, PPT-поля read-only, charge_mode read-only, MUX reboot.
    let mut s = base_full();
    s.profiles = PerformanceProfile::ALL.to_vec();
    s.profile_on_ac = Some(PerformanceProfile::Balanced);
    s.profile_on_battery = Some(PerformanceProfile::Silent);
    // charge_mode = ReadOnly в фикстуре (не представлен здесь как отдельная
    // capability; фиксируется в capability-матрице UI)
    s.mux = GpuMuxState::Integrated;
    s.display.overdrive = PanelOverdriveState::Enabled;
    s.power_limits = PowerLimits {
        fields: BTreeMap::from([
            // Значения из probe: ppt_pl1_spl=5 и т.д., но семантика требует
            // верификации (см. research-report §4.1.1). В mock храним
            // правдоподобные значения с пометкой в capabilities.
            (
                PowerLimitField::Spl,
                PowerLimitValue::new(45, 20, 80, 5, Some(45), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::Sppt,
                PowerLimitValue::new(60, 20, 100, 5, Some(60), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::Fppt,
                PowerLimitValue::new(70, 20, 110, 5, Some(70), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::GpuDynamicBoost,
                PowerLimitValue::new(5, 0, 25, 1, Some(5), Unit::Watts).expect("pl"),
            ),
            (
                PowerLimitField::GpuTempTarget,
                PowerLimitValue::new(75, 60, 87, 1, Some(75), Unit::DegreesC).expect("pl"),
            ),
        ]),
    };
    s.telemetry = telemetry(68, 62, s.fans.clone(), true);
    s
}

fn tuf_no_anime() -> MockState {
    let mut s = base_full();
    s.display.overdrive = PanelOverdriveState::Disabled;
    s.lighting = orbis_core::lighting::LightingMode::Breathing;
    s
}

fn device_no_mux() -> MockState {
    let mut s = base_full();
    s.mux = GpuMuxState::Unknown;
    s.gpu_mode = GpuMode::Standard;
    s
}

fn device_no_asusd() -> MockState {
    // asusd отсутствует: профиль через UPower + kernel ABI (mock).
    let mut s = MockState::full();
    s.profiles = PerformanceProfile::ALL.to_vec();
    s.fans = vec![(FanId::Cpu, Rpm::new(2300).expect("rpm"))];
    fill_curves(&mut s, &[FanId::Cpu]);
    s.power_limits = PowerLimits::default();
    s.mux = GpuMuxState::Unknown;
    s.access_policy = GpuAccessPolicy::Unknown;
    s.gpu_power_state = GpuPowerState::Unknown;
    s.telemetry = telemetry(60, 0, s.fans.clone(), true);
    s
}

fn device_read_only() -> MockState {
    let mut s = base_full();
    s.error_mode = MockErrorMode::PermissionDenied; // приближение: запись запрещена
    s.access_policy = GpuAccessPolicy::Unknown;
    s
}

fn device_three_fans() -> MockState {
    let mut s = base_full();
    s.fans = vec![
        (FanId::Cpu, Rpm::new(2400).expect("rpm")),
        (FanId::Gpu, Rpm::new(2600).expect("rpm")),
        (FanId::Mid, Rpm::new(1500).expect("rpm")),
    ];
    fill_curves(&mut s, &[FanId::Cpu, FanId::Gpu, FanId::Mid]);
    s.telemetry = telemetry(70, 64, s.fans.clone(), true);
    s
}

fn non_asus() -> MockState {
    let mut s = MockState::full();
    s.profiles = vec![PerformanceProfile::Balanced, PerformanceProfile::Turbo];
    s.mux = GpuMuxState::Unknown;
    s.access_policy = GpuAccessPolicy::Unknown;
    s.gpu_power_state = GpuPowerState::Unknown;
    s.display = DisplayMode {
        current_hz: Some(RefreshHz::new(60).expect("hz")),
        modes: vec![RefreshMode::new(RefreshHz::new(60).expect("hz"))],
        overdrive: PanelOverdriveState::Unknown,
        hdr: HdrState::Disabled,
    };
    s.telemetry = telemetry(
        55,
        0,
        vec![(FanId::System, Rpm::new(1200).expect("rpm"))],
        true,
    );
    s
}

fn backend_failure() -> MockState {
    let mut s = MockState::full();
    s.error_mode = MockErrorMode::BackendDown;
    s
}

fn pending_reboot() -> MockState {
    let mut s = base_full();
    s.gpu_mode = GpuMode::Ultimate;
    s.mux = GpuMuxState::Discrete;
    s.pending_action = Some(orbis_core::action::PendingAction {
        id: "mock-mux-1".into(),
        target: "gpu_mux: ultimate".into(),
        requirement: orbis_core::action::ActionRequirement::Reboot,
        cancelable: true,
        created_by: "mock".into(),
    });
    s
}

fn permission_denied() -> MockState {
    let mut s = base_full();
    s.error_mode = MockErrorMode::PermissionDenied;
    s
}

/// Список всех профилей.
pub fn all_profiles() -> Vec<MockDeviceProfile> {
    vec![
        MockDeviceProfile {
            name: ProfileName::zephyrus_full,
            description: "Zephyrus с полным набором функций (MUX, Aura, Overdrive, power limits)",
            builder: zephyrus_full,
        },
        MockDeviceProfile {
            name: ProfileName::tuf_fa707nv_realistic,
            description: "TUF FA707NV: 2 вентилятора, PPT read-only, MUX reboot (по фикстуре)",
            builder: tuf_fa707nv_realistic,
        },
        MockDeviceProfile {
            name: ProfileName::tuf_no_anime,
            description: "TUF без AniMe: нет матрицы, ограниченная подсветка",
            builder: tuf_no_anime,
        },
        MockDeviceProfile {
            name: ProfileName::device_no_mux,
            description: "Ноутбук без физического MUX",
            builder: device_no_mux,
        },
        MockDeviceProfile {
            name: ProfileName::device_no_asusd,
            description: "Устройство без asusd (UPower + kernel ABI)",
            builder: device_no_asusd,
        },
        MockDeviceProfile {
            name: ProfileName::device_read_only,
            description: "Read-only sysfs: записи запрещены",
            builder: device_read_only,
        },
        MockDeviceProfile {
            name: ProfileName::device_three_fans,
            description: "Три вентилятора (CPU/GPU/Mid)",
            builder: device_three_fans,
        },
        MockDeviceProfile {
            name: ProfileName::non_asus,
            description: "Не-ASUS ноутбук (только стандартные интерфейсы)",
            builder: non_asus,
        },
        MockDeviceProfile {
            name: ProfileName::backend_failure,
            description: "Все backend-и недоступны",
            builder: backend_failure,
        },
        MockDeviceProfile {
            name: ProfileName::pending_reboot,
            description: "MUX Ultimate запрошен, ожидает reboot",
            builder: pending_reboot,
        },
        MockDeviceProfile {
            name: ProfileName::permission_denied,
            description: "Недостаточно прав на все операции",
            builder: permission_denied,
        },
    ]
}

/// Профиль по имени.
pub fn profile_by_name(name: &str) -> Option<MockDeviceProfile> {
    let n = ProfileName::parse(name)?;
    all_profiles().into_iter().find(|p| p.name == n)
}

/// Построить состояние по профилю.
pub fn build_state(name: &str) -> Option<MockState> {
    profile_by_name(name).map(|p| {
        let mut s = (p.builder)();
        // Детерминированность: профили, строящиеся через MockState::full()
        // (например, backend-failure), наследуют Telemetry::empty() с
        // SystemTime::now(). Нормализуем timestamp, чтобы все 11 профилей
        // создавались детерминированно.
        s.telemetry.ts = std::time::SystemTime::UNIX_EPOCH;
        s
    })
}

/// Обёртка: Arc<RwLock<MockState>> для провайдера.
pub fn build_state_arc(name: &str) -> Option<Arc<RwLock<MockState>>> {
    build_state(name).map(|s| Arc::new(RwLock::new(s)))
}

/// Список имён профилей для справки CLI.
pub fn profile_names() -> Vec<&'static str> {
    ProfileName::ALL.iter().map(|p| p.as_str()).collect()
}

/// Backend-и, ассоциированные с профилем (для диагностики).
pub fn profile_backends(name: &str) -> Vec<BackendIdentity> {
    match ProfileName::parse(name) {
        Some(ProfileName::backend_failure) => vec![],
        Some(ProfileName::device_no_asusd) => {
            vec![BackendIdentity {
                id: "upower".into(),
                version: Some("1.x".into()),
                service: Some("org.freedesktop.UPower".into()),
            }]
        }
        Some(ProfileName::non_asus) => {
            vec![BackendIdentity {
                id: "upower".into(),
                version: Some("1.x".into()),
                service: Some("org.freedesktop.UPower".into()),
            }]
        }
        _ => {
            vec![
                BackendIdentity {
                    id: "asusd".into(),
                    version: Some("6.3.8".into()),
                    service: Some("xyz.ljones.Asusd".into()),
                },
                BackendIdentity {
                    id: "upower".into(),
                    version: Some("1.x".into()),
                    service: Some("org.freedesktop.UPower".into()),
                },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_names_parse_roundtrip() {
        for n in ProfileName::ALL {
            assert_eq!(ProfileName::parse(n.as_str()), Some(*n));
        }
        assert_eq!(ProfileName::ALL.len(), 11);
    }

    #[test]
    fn every_profile_builds() {
        for p in all_profiles() {
            let s = (p.builder)();
            assert!(
                s.profiles.contains(&s.profile),
                "{}: profile not in list",
                p.name.as_str()
            );
        }
    }

    #[test]
    fn all_profiles_are_deterministic() {
        for name in ProfileName::ALL {
            let a = build_state(name.as_str()).expect("build a");
            let b = build_state(name.as_str()).expect("build b");
            assert_eq!(
                a,
                b,
                "профиль '{}' создаётся недетерминированно",
                name.as_str()
            );
        }
    }

    #[test]
    fn tuf_realistic_has_two_fans() {
        let s = build_state("tuf-fa707nv-realistic").unwrap();
        assert_eq!(s.fans.len(), 2);
        assert_eq!(s.power_limits.fields.len(), 5);
    }

    #[test]
    fn three_fans_profile() {
        let s = build_state("device-three-fans").unwrap();
        assert_eq!(s.fans.len(), 3);
    }

    #[test]
    fn backend_failure_sets_error() {
        let s = build_state("backend-failure").unwrap();
        assert_eq!(s.error_mode, MockErrorMode::BackendDown);
    }

    #[test]
    fn pending_reboot_has_action() {
        let s = build_state("pending-reboot").unwrap();
        assert!(s.pending_action.is_some());
    }

    #[test]
    fn permission_denied_profile() {
        let s = build_state("permission-denied").unwrap();
        assert_eq!(s.error_mode, MockErrorMode::PermissionDenied);
    }

    #[test]
    fn unknown_profile_none() {
        assert!(build_state("does-not-exist").is_none());
    }
}
