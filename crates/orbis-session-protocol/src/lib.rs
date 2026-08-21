//! # Orbis Session Protocol
//!
//! Нейтральный read-only D-Bus wire-контракт между `orbis-session-client`
//! (GUI-side D-Bus client) и `orbis-sessiond` (user session daemon).
//!
//! - контракт содержит только чтение Battery Charge Limit;
//! - crate не создаёт D-Bus connection, runtime и не обращается к hardware;
//! - версия интерфейса зафиксирована в имени `Session1`.
//!
//! Запрещено добавлять в этот crate mutation API (getter-only контракт)
//! и зависимости от доменного/runtime слоя.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Стабильное имя D-Bus service (system/session bus).
pub const BUS_NAME: &str = "io.github.orbiscontrol.Session";

/// Стабильный object path D-Bus service.
pub const OBJECT_PATH: &str = "/io/github/orbiscontrol/Session";

/// Имя интерфейса D-Bus (версия интерфейса закодирована как `1`).
pub const INTERFACE_NAME: &str = "io.github.orbiscontrol.Session1";

/// Wire DTO Battery Charge Limit.
///
/// Представление явное и стабильное: D-Bus не имеет универсального нативного
/// `Option<u8>`, поэтому достоверность `percent` выражается отдельным флагом
/// configured/effective percent presence fields.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    zbus::zvariant::Type,
    zbus::zvariant::OwnedValue,
)]
pub struct ChargeLimitInfo {
    /// Активна ли функция ограничения заряда.
    pub enabled: bool,
    /// Достоверно ли reported/configured значение.
    pub configured_percent_present: bool,
    /// Reported/configured end threshold; при отсутствии равен 0.
    pub configured_percent: u8,
    /// Достоверно ли effective hardware значение.
    pub effective_percent_present: bool,
    /// Effective kernel end threshold; при отсутствии равен 0.
    pub effective_percent: u8,
    /// Известны ли hardware/backend constraints (min/max/step).
    pub bounds_present: bool,
    /// Нижняя поддерживаемая граница; при `bounds_present == false` равен 0.
    pub min_percent: u8,
    /// Верхняя поддерживаемая граница; при `bounds_present == false` равен 0.
    pub max_percent: u8,
    /// Минимальный объявленный шаг backend; при `bounds_present == false` равен 0.
    pub step_percent: u8,
}

/// One source-labelled threshold observation on the Session1 wire.
/// `(source, value, observed_at_ms, freshness, confidence)`.
pub type BatteryThresholdObservationTuple = (u8, u8, u64, u8, u8);

/// Battery threshold evidence wire: `(state, observations)`.
pub type BatteryThresholdEvidenceTuple = (u8, Vec<BatteryThresholdObservationTuple>);

/// Prefix for preserving typed evidence conflicts across the FDO error mapper.
pub const BATTERY_THRESHOLD_CONFLICT_PREFIX: &str = "battery-threshold-conflict:";

/// Source discriminants for battery threshold observations.
pub mod battery_threshold_source {
    /// Source discriminants for threshold observations.
    /// UPower source.
    pub const UPOWER: u8 = 0;
    /// ASUS backend source.
    pub const ASUS_BACKEND: u8 = 1;
    /// Kernel sysfs source.
    pub const SYSFS: u8 = 2;
}

/// Aggregate state discriminants for battery threshold evidence.
pub mod battery_threshold_state {
    /// Aggregate evidence-state discriminants.
    /// One source observed.
    pub const OBSERVED: u8 = 0;
    /// Equal sources confirmed.
    pub const CONFIRMED: u8 = 1;
    /// Conflicting source values.
    pub const CONFLICT: u8 = 2;
    /// No usable evidence.
    pub const UNKNOWN: u8 = 3;
}

/// Freshness discriminants for battery threshold observations.
pub mod battery_threshold_freshness {
    /// Freshness discriminants.
    /// Fresh observation.
    pub const FRESH: u8 = 0;
    /// Stale observation.
    pub const STALE: u8 = 1;
    /// Unknown freshness.
    pub const UNKNOWN: u8 = 2;
}

/// Confidence discriminants for battery threshold observations.
pub mod battery_threshold_confidence {
    /// Confidence discriminants.
    /// Low confidence.
    pub const LOW: u8 = 0;
    /// Medium confidence.
    pub const MEDIUM: u8 = 1;
    /// High confidence.
    pub const HIGH: u8 = 2;
}

impl ChargeLimitInfo {
    /// Создать DTO с достоверным текущим значением процента и известными bounds.
    ///
    /// Диапазон/шаг не валидируются здесь: проверка согласованности
    /// min/max/step выполняется на границе конкретного backend/domain
    /// conversion (protocol crate не считает диапазон 40..=100 универсальным).
    pub const fn with_percent(
        enabled: bool,
        configured_percent: u8,
        effective_percent: u8,
        min_percent: u8,
        max_percent: u8,
        step_percent: u8,
    ) -> Self {
        Self {
            enabled,
            configured_percent_present: true,
            configured_percent,
            effective_percent_present: true,
            effective_percent,
            bounds_present: true,
            min_percent,
            max_percent,
            step_percent,
        }
    }

    /// Создать DTO с достоверным текущим значением процента и неизвестными
    /// bounds (`bounds_present = false`, min/max/step = 0).
    pub const fn with_percent_unknown_bounds(
        enabled: bool,
        configured_percent: u8,
        effective_percent: u8,
    ) -> Self {
        Self {
            enabled,
            configured_percent_present: true,
            configured_percent,
            effective_percent_present: true,
            effective_percent,
            bounds_present: false,
            min_percent: 0,
            max_percent: 0,
            step_percent: 0,
        }
    }

    /// Создать DTO без достоверного значения процента и с известными bounds.
    ///
    /// Устанавливает configured/effective presence в false и значения в 0.
    pub const fn without_percent(
        enabled: bool,
        min_percent: u8,
        max_percent: u8,
        step_percent: u8,
    ) -> Self {
        Self {
            enabled,
            configured_percent_present: false,
            configured_percent: 0,
            effective_percent_present: false,
            effective_percent: 0,
            bounds_present: true,
            min_percent,
            max_percent,
            step_percent,
        }
    }

    /// Создать DTO без достоверного значения процента и с неизвестными bounds.
    ///
    /// Устанавливает configured/effective presence в false и значения в 0,
    /// `bounds_present = false`, min/max/step = 0.
    pub const fn without_percent_unknown_bounds(enabled: bool) -> Self {
        Self {
            enabled,
            configured_percent_present: false,
            configured_percent: 0,
            effective_percent_present: false,
            effective_percent: 0,
            bounds_present: false,
            min_percent: 0,
            max_percent: 0,
            step_percent: 0,
        }
    }

    /// Configured/reported процент, если он достоверен.
    pub const fn configured_percent(self) -> Option<u8> {
        if self.configured_percent_present {
            Some(self.configured_percent)
        } else {
            None
        }
    }

    /// Effective hardware процент, если он достоверен.
    pub const fn effective_percent(self) -> Option<u8> {
        if self.effective_percent_present {
            Some(self.effective_percent)
        } else {
            None
        }
    }
}

/// Wire DTO Performance Mode state.
///
/// Компактный canonical wire: `current` — wire-значение текущего профиля
/// (`performance::*`), `available_mask` — битовая маска доступных профилей
/// (bit0=Silent, bit1=Balanced, bit2=Turbo). D-Bus не имеет нативного
/// enum/битового set, поэтому значения зафиксированы явно и проверяются
/// строго на client boundary.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    zbus::zvariant::Type,
    zbus::zvariant::OwnedValue,
)]
pub struct PerformanceInfo {
    /// Wire-значение текущего профиля (`performance::SILENT` и т.д.).
    pub current: u8,
    /// Битовая маска доступных профилей (bit0=Silent, bit1=Balanced, bit2=Turbo).
    pub available_mask: u8,
}

/// Getter-only zbus proxy контракт интерфейса `Session1`.
///
/// Свойства (`ChargeLimit`, `GpuPower`, `GpuMux`, `GpuAccess`, `Performance`)
/// являются read-only; Performance mutation выполняется через Hardware1 на
/// system bus, не через Session1/sessiond.
#[zbus::proxy(
    interface = "io.github.orbiscontrol.Session1",
    default_service = "io.github.orbiscontrol.Session",
    default_path = "/io/github/orbiscontrol/Session"
)]
pub trait Session1 {
    /// Текущий Battery Charge Limit (read-only property).
    #[zbus(property)]
    fn charge_limit(&self) -> zbus::Result<ChargeLimitInfo>;

    /// Source-labelled battery threshold evidence (read-only method).
    fn battery_threshold_evidence(&self) -> zbus::Result<BatteryThresholdEvidenceTuple>;

    /// Текущий dGPU runtime power state (read-only property).
    #[zbus(property)]
    fn gpu_power(&self) -> zbus::Result<u8>;

    /// Текущее физическое MUX состояние (read-only property).
    #[zbus(property)]
    fn gpu_mux(&self) -> zbus::Result<u8>;

    /// Текущая dGPU access policy (read-only property).
    #[zbus(property)]
    fn gpu_access(&self) -> zbus::Result<u8>;

    /// Текущий Performance Mode (current + available, read-only property).
    #[zbus(property)]
    fn performance(&self) -> zbus::Result<PerformanceInfo>;

    /// Сохранённая fan curve для профиля и вентилятора (read-only method).
    fn fan_curve(&self, profile: u32, fan: u8) -> zbus::Result<FanCurveInfo>;
}

/// Wire-значения `GpuPowerState` (domain enum в protocol crate).
pub mod gpu_power {
    /// dGPU активна (D0).
    pub const ACTIVE: u8 = 0;
    /// Низкопотребляющее состояние (D3cold и т.п.).
    pub const SUSPENDED: u8 = 1;
    /// Выключена/не обнаруживается.
    pub const OFF: u8 = 2;
    /// Последнее известное значение устарело.
    pub const STALE: u8 = 3;
    /// Semantic state неизвестно.
    pub const UNKNOWN: u8 = 4;
}

/// Wire-значения `GpuMuxState` (domain enum в protocol crate).
pub mod gpu_mux {
    /// MUX направлен на iGPU.
    pub const INTEGRATED: u8 = 0;
    /// MUX направлен на dGPU.
    pub const DISCRETE: u8 = 1;
    /// Semantic state неизвестно.
    pub const UNKNOWN: u8 = 2;
}

/// Wire-значения `GpuAccessPolicy` (domain enum в protocol crate).
pub mod gpu_access {
    /// dGPU доступна приложениям.
    pub const UNBLOCKED: u8 = 0;
    /// dGPU заблокирована для приложений.
    pub const BLOCKED: u8 = 1;
    /// Переключение в процессе.
    pub const PENDING: u8 = 2;
    /// Semantic state неизвестно.
    pub const UNKNOWN: u8 = 3;
}

/// Wire-значения `PerformanceProfile` и маска доступности.
pub mod performance {
    /// Silent (quiet/low-power).
    pub const SILENT: u8 = 0;
    /// Balanced.
    pub const BALANCED: u8 = 1;
    /// Turbo (performance).
    pub const TURBO: u8 = 2;
    /// Бит маски: Silent доступен.
    pub const SILENT_BIT: u8 = 1;
    /// Бит маски: Balanced доступен.
    pub const BALANCED_BIT: u8 = 1 << 1;
    /// Бит маски: Turbo доступен.
    pub const TURBO_BIT: u8 = 1 << 2;
}

/// Wire-значения `AsusdFanProfile` (lossless asusd fan profile).
///
/// Значения фиксированы и совпадают с asusd wire (0..3); Silent
/// автоматически в Quiet/LowPower НЕ маппится (остаётся lossless).
pub mod fan_profile {
    /// Balanced (wire 0).
    pub const BALANCED: u32 = 0;
    /// Performance (wire 1).
    pub const PERFORMANCE: u32 = 1;
    /// Quiet (wire 2).
    pub const QUIET: u32 = 2;
    /// LowPower (wire 3).
    pub const LOW_POWER: u32 = 3;
}

/// Wire-значения `FanId` (вентилятор).
pub mod fan_id {
    /// CPU вентилятор.
    pub const CPU: u8 = 0;
    /// GPU вентилятор.
    pub const GPU: u8 = 1;
}

/// Wire DTO сохранённой fan curve для профиля и вентилятора.
///
/// Кривая хранит **raw hwmon PWM** (0..=255), НЕ процент; mapping 0..255 → %
/// не доказан. Точки фиксированы: 8 температур (°C, raw u8) + 8 PWM (raw);
/// длина массивов (8) проверяется на wire boundary — protocol не терпит
/// синтетических/fallback кривых. `profile` — wire `fan_profile::*`,
/// `fan` — wire `fan_id::*`.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, zbus::zvariant::Type,
)]
pub struct FanCurveInfo {
    /// Wire профиль (`fan_profile::*`).
    pub profile: u32,
    /// Wire вентилятор (`fan_id::*`).
    pub fan: u8,
    /// 8 температур, °C (raw wire u8).
    pub temps: Vec<u8>,
    /// 8 raw PWM 0..255.
    pub pwms: Vec<u8>,
    /// Whether the backend has this stored curve enabled.
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Type;

    #[test]
    fn dbus_names_are_stable() {
        assert_eq!(BUS_NAME, "io.github.orbiscontrol.Session");
        assert_eq!(OBJECT_PATH, "/io/github/orbiscontrol/Session");
        assert_eq!(INTERFACE_NAME, "io.github.orbiscontrol.Session1");
    }

    #[test]
    fn charge_limit_with_percent() {
        let info = ChargeLimitInfo::with_percent(true, 80, 80, 40, 100, 5);
        assert!(info.enabled);
        assert!(info.configured_percent_present);
        assert_eq!(info.configured_percent, 80);
        assert!(info.effective_percent_present);
        assert_eq!(info.effective_percent, 80);
        assert!(info.bounds_present);
        assert_eq!(info.min_percent, 40);
        assert_eq!(info.max_percent, 100);
        assert_eq!(info.step_percent, 5);
    }

    #[test]
    fn charge_limit_with_percent_unknown_bounds() {
        let info = ChargeLimitInfo::with_percent_unknown_bounds(true, 80, 80);
        assert!(info.enabled);
        assert!(info.configured_percent_present);
        assert_eq!(info.configured_percent, 80);
        assert!(info.effective_percent_present);
        assert_eq!(info.effective_percent, 80);
        assert!(!info.bounds_present);
        assert_eq!(info.min_percent, 0);
        assert_eq!(info.max_percent, 0);
        assert_eq!(info.step_percent, 0);
    }

    #[test]
    fn charge_limit_without_percent() {
        let info = ChargeLimitInfo::without_percent(false, 40, 100, 5);
        assert!(!info.enabled);
        assert!(!info.configured_percent_present);
        assert_eq!(info.configured_percent, 0);
        assert!(!info.effective_percent_present);
        assert_eq!(info.effective_percent, 0);
        assert!(info.bounds_present);
        assert_eq!(info.min_percent, 40);
        assert_eq!(info.max_percent, 100);
        assert_eq!(info.step_percent, 5);
    }

    #[test]
    fn charge_limit_without_percent_unknown_bounds() {
        let info = ChargeLimitInfo::without_percent_unknown_bounds(false);
        assert!(!info.enabled);
        assert!(!info.configured_percent_present);
        assert_eq!(info.configured_percent, 0);
        assert!(!info.effective_percent_present);
        assert_eq!(info.effective_percent, 0);
        assert!(!info.bounds_present);
        assert_eq!(info.min_percent, 0);
        assert_eq!(info.max_percent, 0);
        assert_eq!(info.step_percent, 0);
    }

    #[test]
    fn charge_limit_dbus_signature_is_stable() {
        // bool configured bool/u8 effective bool/u8 bounds/u8/u8/u8.
        let expected: zbus::zvariant::Signature =
            "(bbybybyyy)".try_into().expect("valid signature");
        assert_eq!(*ChargeLimitInfo::SIGNATURE, expected);
    }

    #[test]
    fn charge_limit_serde_roundtrip() {
        let info = ChargeLimitInfo::with_percent(true, 80, 80, 40, 100, 5);
        let ctx = zbus::zvariant::serialized::Context::new_dbus(zbus::zvariant::Endian::Little, 0);
        let data = zbus::zvariant::to_bytes(ctx, &info).expect("serialize");
        let (decoded, _): (ChargeLimitInfo, usize) = data.deserialize().expect("deserialize");
        assert_eq!(decoded, info);
    }

    #[test]
    fn gpu_wire_constants_are_stable() {
        // GpuPowerState wire values (domain: Active/Suspended/Off/Stale/Unknown).
        assert_eq!(gpu_power::ACTIVE, 0);
        assert_eq!(gpu_power::SUSPENDED, 1);
        assert_eq!(gpu_power::OFF, 2);
        assert_eq!(gpu_power::STALE, 3);
        assert_eq!(gpu_power::UNKNOWN, 4);
        // GpuMuxState wire values (domain: Integrated/Discrete/Unknown).
        assert_eq!(gpu_mux::INTEGRATED, 0);
        assert_eq!(gpu_mux::DISCRETE, 1);
        assert_eq!(gpu_mux::UNKNOWN, 2);
        // GpuAccessPolicy wire values (domain: Unblocked/Blocked/Pending/Unknown).
        assert_eq!(gpu_access::UNBLOCKED, 0);
        assert_eq!(gpu_access::BLOCKED, 1);
        assert_eq!(gpu_access::PENDING, 2);
        assert_eq!(gpu_access::UNKNOWN, 3);
        // PerformanceProfile wire values (domain: Silent/Balanced/Turbo).
        assert_eq!(performance::SILENT, 0);
        assert_eq!(performance::BALANCED, 1);
        assert_eq!(performance::TURBO, 2);
        assert_eq!(performance::SILENT_BIT, 1);
        assert_eq!(performance::BALANCED_BIT, 2);
        assert_eq!(performance::TURBO_BIT, 4);
    }

    #[test]
    fn performance_dbus_signature_is_stable() {
        // u8 u8 -> "(yy)"
        let expected: zbus::zvariant::Signature = "(yy)".try_into().expect("valid signature");
        assert_eq!(*PerformanceInfo::SIGNATURE, expected);
    }

    #[test]
    fn fan_wire_constants_are_stable() {
        // AsusdFanProfile wire values (lossless 0..3).
        assert_eq!(fan_profile::BALANCED, 0);
        assert_eq!(fan_profile::PERFORMANCE, 1);
        assert_eq!(fan_profile::QUIET, 2);
        assert_eq!(fan_profile::LOW_POWER, 3);
        // FanId wire values.
        assert_eq!(fan_id::CPU, 0);
        assert_eq!(fan_id::GPU, 1);
    }

    #[test]
    fn fan_curve_roundtrip_preserves_sentinel_curves() {
        // Sentinel-кривые (A/B/C) проходят wire round-trip без потери.
        let sentinel = |profile: u32, temps: &[u8; 8], pwms: &[u8; 8]| FanCurveInfo {
            profile,
            fan: fan_id::CPU,
            temps: temps.to_vec(),
            pwms: pwms.to_vec(),
            enabled: true,
        };
        let a = sentinel(
            fan_profile::BALANCED,
            &[45, 49, 54, 68, 74, 79, 84, 89],
            &[5, 22, 38, 45, 56, 63, 81, 94],
        );
        let b = sentinel(
            fan_profile::BALANCED,
            &[40, 44, 50, 60, 70, 76, 82, 90],
            &[3, 18, 35, 42, 50, 58, 70, 99],
        );
        let c = sentinel(
            fan_profile::QUIET,
            &[42, 46, 55, 64, 73, 80, 86, 92],
            &[2, 12, 28, 38, 46, 54, 66, 88],
        );
        for info in [a, b, c] {
            let ctx =
                zbus::zvariant::serialized::Context::new_dbus(zbus::zvariant::Endian::Little, 0);
            let data = zbus::zvariant::to_bytes(ctx, &info).expect("serialize");
            let (decoded, _): (FanCurveInfo, usize) = data.deserialize().expect("deserialize");
            assert_eq!(decoded, info);
        }
    }

    #[test]
    fn fan_dbus_signature_is_stable() {
        // u32(u) y(y) 8-temperature array(ay) 8-pwm array(ay) enabled (b).
        let expected: zbus::zvariant::Signature = "(uyayayb)".try_into().expect("valid signature");
        assert_eq!(*FanCurveInfo::SIGNATURE, expected);
    }

    #[test]
    fn performance_serde_roundtrip() {
        let info = PerformanceInfo {
            current: performance::BALANCED,
            available_mask: 0b111,
        };
        let ctx = zbus::zvariant::serialized::Context::new_dbus(zbus::zvariant::Endian::Little, 0);
        let data = zbus::zvariant::to_bytes(ctx, &info).expect("serialize");
        let (decoded, _): (PerformanceInfo, usize) = data.deserialize().expect("deserialize");
        assert_eq!(decoded, info);
    }
}
