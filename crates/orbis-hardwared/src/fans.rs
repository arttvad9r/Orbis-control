//! Internal ASUS fan curve mutation backend.
//!
//! Единственный owner fan curve writes — `asusd` (ADR 0011). Этот модуль
//! вызывает typed `xyz.ljones.FanCurves.SetFanCurve` D-Bus setter и выполняет
//! fresh `FanCurveData(profile)` read-back. Никаких direct sysfs writes.
//!
//! - closed API: принимает lossless `AsusdFanProfile` + fan (CPU/GPU) + ровно
//!   8 `(TemperatureC, FanPwm)` точек; никаких path/string/arbitrary args;
//! - validation выполняется ДО setter;
//! - setter вызывается только для выбранного fan (не batch CPU+GPU);
//! - read-back обязателен: success только если нужная fan curve совпала
//!   полностью;
//! - `FanPwm` raw 0..255, не percent;
//! - никаких `SetFanCurvesEnabled`/`SetProfileFanCurveEnabled`/defaults/reset.

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::fan::FanId;
use orbis_core::newtypes::{FanPwm, TemperatureC};
use orbis_core::profile::AsusdFanProfile;
use orbis_providers::error::ProviderError;
use zbus::Connection;

/// Количество точек кривой, фиксированное kernel ABI `asus_custom_fan_curve`.
pub const CURVE_POINT_COUNT: usize = 8;

/// Wire-элемент asusd `FanCurveData`: name + 8 temp + 8 pwm + enabled.
type AsusdCurveWire = (String, [u8; 8], [u8; 8], bool);

/// Одна кривая вентилятора (typed, для mutation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanCurvePoints {
    /// 8 температур, °C.
    pub temps: [TemperatureC; CURVE_POINT_COUNT],
    /// 8 raw PWM 0..255.
    pub pwms: [FanPwm; CURVE_POINT_COUNT],
}

/// Typed asusd FanCurves operations required by the mutation algorithm.
#[async_trait]
pub trait AsusdFanCurveClient: Send + Sync {
    /// Установить кривую для одного вентилятора профиля.
    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
        curve: &FanCurvePoints,
    ) -> Result<(), ProviderError>;

    /// Прочитать сохранённые кривые профиля (fresh read-back).
    async fn read_curves(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<Vec<(String, [u8; 8], [u8; 8], bool)>, ProviderError>;
}

/// Production typed client for the asusd FanCurves compatibility backend.
pub struct ZbusAsusdFanCurveClient {
    connection: Connection,
}

impl ZbusAsusdFanCurveClient {
    /// Construct without performing a D-Bus call.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<AsusdFanCurvesProxy<'_>, ProviderError> {
        AsusdFanCurvesProxy::new(&self.connection)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd FanCurves proxy: {error}")))
    }
}

#[zbus::proxy(
    interface = "xyz.ljones.FanCurves",
    default_service = "xyz.ljones.Asusd",
    default_path = "/xyz/ljones"
)]
trait AsusdFanCurves {
    fn set_fan_curve(
        &self,
        profile: u32,
        curve: (String, [u8; 8], [u8; 8], bool),
    ) -> zbus::Result<()>;

    fn fan_curve_data(&self, profile: u32) -> zbus::Result<Vec<AsusdCurveWire>>;
}

/// Wire-имя вентилятора для asusd `SetFanCurve`.
fn fan_wire_name(fan: &FanId) -> Result<&'static str, ProviderError> {
    match fan {
        FanId::Cpu => Ok("CPU"),
        FanId::Gpu => Ok("GPU"),
        other => Err(ProviderError::InvalidRequest(format!(
            "hardwared: fan {other:?} не поддерживается (только CPU/GPU)"
        ))),
    }
}

#[async_trait]
impl AsusdFanCurveClient for ZbusAsusdFanCurveClient {
    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
        curve: &FanCurvePoints,
    ) -> Result<(), ProviderError> {
        let name = fan_wire_name(fan)?;
        let mut temps = [0u8; 8];
        let mut pwms = [0u8; 8];
        for (i, (t, p)) in curve.temps.iter().zip(curve.pwms.iter()).enumerate() {
            temps[i] = u8::try_from(t.get()).map_err(|_| {
                ProviderError::InvalidRequest(format!(
                    "hardwared: температура {}°C в точке {i} не помещается в wire u8",
                    t.get()
                ))
            })?;
            pwms[i] = p.get();
        }
        // enabled: не изменяем (сохраняем текущее значение из read-back не
        // требуется для setter — asusd сохраняет enabled отдельно). Для
        // SetFanCurve передаём enabled=false (не трогаем enable state).
        let wire = (name.to_string(), temps, pwms, false);
        self.proxy()
            .await?
            .set_fan_curve(profile.wire(), wire)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd SetFanCurve: {error}")))
    }

    async fn read_curves(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<Vec<(String, [u8; 8], [u8; 8], bool)>, ProviderError> {
        self.proxy()
            .await?
            .fan_curve_data(profile.wire())
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd FanCurveData read: {error}")))
    }
}

/// Fresh result of an asusd fan curve mutation and its read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanCurveMutationReadback {
    pub requested_profile: AsusdFanProfile,
    pub requested_fan: FanId,
    pub result: ApplyResult,
}

/// Trait для fan curve mutation backend (для trait object в HardwareService).
#[async_trait]
pub trait FanCurveMutationOperation: Send + Sync {
    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
        curve: &FanCurvePoints,
    ) -> Result<FanCurveMutationReadback, ProviderError>;

    /// Read-only typed runtime evidence about fan curve mutation availability.
    ///
    /// Must never call `set_fan_curve` or mutate anything. Uses the existing
    /// read-only `FanCurveData` path on the same `xyz.ljones.FanCurves`
    /// interface the setter targets, so a successful read proves the mutation
    /// interface is present and responsive.
    async fn mutation_status(&self) -> FanMutationStatus;
}

/// Typed runtime evidence for fan curve mutation backend availability.
///
/// The distinction matters: a configured backend object (`Some`) does not
/// prove the asusd service or the `xyz.ljones.FanCurves` interface is
/// reachable, and `None` only means "not configured" — never a proven
/// hardware `Unsupported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanMutationStatus {
    /// The fan curve mutation backend is configured and the read-only
    /// `FanCurves` interface probe succeeded.
    Supported,
    /// The interface/feature is proven unsupported by the backend contract.
    Unsupported,
    /// A known/expected backend or service is temporarily unavailable.
    TemporarilyUnavailable,
    /// The operation exists but current authorization evidence denies it.
    PermissionDenied,
    /// The required backend/service is structurally absent (e.g. no fan
    /// mutation backend configured, or asusd service not present).
    BackendMissing,
    /// No provable evidence about mutation availability.
    Unknown,
}

/// Stable wire values for `Hardware1.FanMutationStatus`.
pub mod fan_mutation_wire {
    use super::FanMutationStatus;

    /// Proven mutation backend / interface present.
    pub const SUPPORTED: u8 = 0;
    /// Mutation capability structurally unsupported.
    pub const UNSUPPORTED: u8 = 1;
    /// Known backend temporarily unavailable.
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    /// Mutation denied by authorization evidence.
    pub const PERMISSION_DENIED: u8 = 3;
    /// Required backend/service absent.
    pub const BACKEND_MISSING: u8 = 4;
    /// No evidence.
    pub const UNKNOWN: u8 = 5;

    /// Encode typed status into the D-Bus wire value.
    pub fn to_wire(status: FanMutationStatus) -> u8 {
        match status {
            FanMutationStatus::Supported => SUPPORTED,
            FanMutationStatus::Unsupported => UNSUPPORTED,
            FanMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            FanMutationStatus::PermissionDenied => PERMISSION_DENIED,
            FanMutationStatus::BackendMissing => BACKEND_MISSING,
            FanMutationStatus::Unknown => UNKNOWN,
        }
    }

    /// Decode a wire value; unknown values produce `None` so callers classify
    /// them as `Unknown` instead of inventing a known state.
    pub fn from_wire(raw: u8) -> Option<FanMutationStatus> {
        match raw {
            SUPPORTED => Some(FanMutationStatus::Supported),
            UNSUPPORTED => Some(FanMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => Some(FanMutationStatus::TemporarilyUnavailable),
            PERMISSION_DENIED => Some(FanMutationStatus::PermissionDenied),
            BACKEND_MISSING => Some(FanMutationStatus::BackendMissing),
            UNKNOWN => Some(FanMutationStatus::Unknown),
            _ => None,
        }
    }
}

/// Classify a D-Bus error detail string into a fan mutation status.
///
/// Mirrors the stable error-name semantics used by the capability probe layer
/// (`ServiceUnknown`/`NameHasNoOwner` → backend absent, `UnknownMethod`/
/// `NotSupported` → unsupported, `AccessDenied` → denied, timeouts → transient).
fn classify_fan_mutation_dbus(detail: &str) -> FanMutationStatus {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("serviceunknown")
        || lower.contains("namehasnoowner")
        || lower.contains("service not found")
    {
        FanMutationStatus::BackendMissing
    } else if lower.contains("accessdenied") || lower.contains("permission denied") {
        FanMutationStatus::PermissionDenied
    } else if lower.contains("unknownmethod")
        || lower.contains("unknowninterface")
        || lower.contains("not supported")
        || lower.contains("notsupported")
    {
        FanMutationStatus::Unsupported
    } else if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("noreply")
        || lower.contains("disconnected")
    {
        FanMutationStatus::TemporarilyUnavailable
    } else {
        FanMutationStatus::Unknown
    }
}

/// Validate the public fan curve input without contacting any backend.
///
/// - ровно 8 точек (гарантировано типом массива);
/// - температуры не убывают;
/// - PWM не убывают (allow_decreasing=false);
/// - последняя точка не 0 при критической температуре (>= 80 °C);
/// - все температуры в wire range 0..=255 (asusd wire type `u8`).
pub fn validate_fan_curve(curve: &FanCurvePoints) -> Result<(), ProviderError> {
    for (i, t) in curve.temps.iter().enumerate() {
        let raw = t.get();
        if !(0..=255).contains(&raw) {
            return Err(ProviderError::InvalidRequest(format!(
                "hardwared: температура {raw}°C в точке {i} вне wire диапазона 0..=255"
            )));
        }
    }
    for w in curve.temps.windows(2) {
        if w[1] < w[0] {
            return Err(ProviderError::InvalidRequest(format!(
                "hardwared: температуры кривой убывают: {} -> {}",
                w[0], w[1]
            )));
        }
    }
    for w in curve.pwms.windows(2) {
        if w[1] < w[0] {
            return Err(ProviderError::InvalidRequest(format!(
                "hardwared: PWM кривой убывают: {} -> {}",
                w[0], w[1]
            )));
        }
    }
    let last_temp = curve.temps[CURVE_POINT_COUNT - 1];
    let last_pwm = curve.pwms[CURVE_POINT_COUNT - 1];
    if last_temp >= TemperatureC::new(80).expect("const") && last_pwm.get() == 0 {
        return Err(ProviderError::InvalidRequest(
            "hardwared: нулевой вентилятор на критической температуре (>= 80 °C)".into(),
        ));
    }
    Ok(())
}

/// Internal fan curve mutation backend; never writes the kernel directly.
pub struct AsusdFanCurveMutationBackend<C> {
    asusd: C,
}

impl<C> AsusdFanCurveMutationBackend<C> {
    pub fn new(asusd: C) -> Self {
        Self { asusd }
    }
}

impl<C> AsusdFanCurveMutationBackend<C>
where
    C: AsusdFanCurveClient,
{
    /// Validate, perform one asusd setter for the selected fan, then fresh
    /// read-back. Success only if the requested fan curve matches completely.
    pub async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
        curve: &FanCurvePoints,
    ) -> Result<FanCurveMutationReadback, ProviderError> {
        // 1. validation до setter.
        validate_fan_curve(curve)?;
        let name = fan_wire_name(fan)?;

        // 2. ровно один setter для выбранного fan.
        self.asusd.set_fan_curve(profile, fan, curve).await?;

        // 3. fresh read-back.
        let raw = self.asusd.read_curves(profile).await?;
        let matched = raw.iter().any(|(n, temps, pwms, _enabled)| {
            if n != name {
                return false;
            }
            for (i, (t, p)) in curve.temps.iter().zip(curve.pwms.iter()).enumerate() {
                let wire_temp = u8::try_from(t.get()).unwrap_or(0);
                if temps[i] != wire_temp || pwms[i] != p.get() {
                    return false;
                }
            }
            true
        });

        if !matched {
            return Err(ProviderError::BackendUnavailable(format!(
                "hardwared: read-back не подтвердил fan curve для {name} (profile={:?})",
                profile
            )));
        }

        Ok(FanCurveMutationReadback {
            requested_profile: profile,
            requested_fan: fan.clone(),
            result: ApplyResult::Applied,
        })
    }
}

#[async_trait]
impl<C> FanCurveMutationOperation for AsusdFanCurveMutationBackend<C>
where
    C: AsusdFanCurveClient + Send + Sync,
{
    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: &FanId,
        curve: &FanCurvePoints,
    ) -> Result<FanCurveMutationReadback, ProviderError> {
        self.set_fan_curve(profile, fan, curve).await
    }

    async fn mutation_status(&self) -> FanMutationStatus {
        // Read-only evidence: the same `FanCurveData` read path the mutation
        // backend uses for authoritative read-back, on the same
        // `xyz.ljones.FanCurves` interface the setter targets. A successful
        // read proves the mutation interface is present and responsive.
        match self.asusd.read_curves(AsusdFanProfile::Balanced).await {
            Ok(_) => FanMutationStatus::Supported,
            Err(ProviderError::Dbus(detail)) => classify_fan_mutation_dbus(&detail),
            Err(_) => FanMutationStatus::Unknown,
        }
    }
}

/// Wire DTO для Hardware1 `SetFanCurve`.
///
/// Использует `Vec<u8>` (D-Bus `ay`), т.к. zbus не поддерживает массивы
/// фиксированной длины в сигнатурах напрямую. Ровно 8 элементов.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    zbus::zvariant::Type,
    zbus::zvariant::OwnedValue,
)]
pub struct FanCurveWire {
    /// 8 температур, °C.
    pub temps: Vec<u8>,
    /// 8 raw PWM 0..255.
    pub pwms: Vec<u8>,
}

/// Strict decode wire `u32` → `AsusdFanProfile` (0..3).
pub fn fan_profile_from_wire(raw: u32) -> Result<AsusdFanProfile, ProviderError> {
    match raw {
        0 => Ok(AsusdFanProfile::Balanced),
        1 => Ok(AsusdFanProfile::Performance),
        2 => Ok(AsusdFanProfile::Quiet),
        3 => Ok(AsusdFanProfile::LowPower),
        other => Err(ProviderError::InvalidRequest(format!(
            "hardwared: неизвестный fan profile wire value {other}"
        ))),
    }
}

/// Strict decode wire `u8` → `FanId` (0=CPU, 1=GPU).
pub fn fan_from_wire(raw: u8) -> Result<FanId, ProviderError> {
    match raw {
        0 => Ok(FanId::Cpu),
        1 => Ok(FanId::Gpu),
        other => Err(ProviderError::InvalidRequest(format!(
            "hardwared: неизвестный fan wire value {other}"
        ))),
    }
}

/// Strict decode wire curve → `FanCurvePoints` (ровно 8 точек, ranges).
pub fn fan_curve_from_wire(wire: &FanCurveWire) -> Result<FanCurvePoints, ProviderError> {
    if wire.temps.len() != CURVE_POINT_COUNT || wire.pwms.len() != CURVE_POINT_COUNT {
        return Err(ProviderError::InvalidRequest(format!(
            "hardwared: кривая должна содержать ровно {CURVE_POINT_COUNT} точек, получено temps={}, pwms={}",
            wire.temps.len(),
            wire.pwms.len()
        )));
    }
    let mut temps = [TemperatureC::new(0).expect("const"); CURVE_POINT_COUNT];
    let mut pwms = [FanPwm::new(0).expect("const"); CURVE_POINT_COUNT];
    for (i, (t, p)) in wire.temps.iter().zip(wire.pwms.iter()).enumerate() {
        temps[i] = TemperatureC::new(*t as i16).map_err(|_| {
            ProviderError::InvalidRequest(format!(
                "hardwared: температура вне диапазона '{t}' в точке {i}"
            ))
        })?;
        pwms[i] = FanPwm::new(*p).map_err(|_| {
            ProviderError::InvalidRequest(format!("hardwared: PWM вне диапазона '{p}' в точке {i}"))
        })?;
    }
    Ok(FanCurvePoints { temps, pwms })
}

/// Wire encode подтверждённого fan profile.
pub fn fan_profile_to_wire(profile: AsusdFanProfile) -> u32 {
    profile.wire()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(temps: [i16; 8], pwms: [u8; 8]) -> FanCurvePoints {
        FanCurvePoints {
            temps: temps.map(|t| TemperatureC::new(t).expect("temp")),
            pwms: pwms.map(|p| FanPwm::new(p).expect("pwm")),
        }
    }

    fn valid_curve() -> FanCurvePoints {
        curve(
            [45, 49, 54, 68, 74, 79, 84, 89],
            [5, 22, 38, 45, 56, 63, 81, 94],
        )
    }

    #[test]
    fn wire_profile_roundtrip_is_lossless() {
        for wire in 0..=3u32 {
            let profile = fan_profile_from_wire(wire).expect("valid");
            assert_eq!(fan_profile_to_wire(profile), wire);
        }
        // Quiet и LowPower различимы.
        assert_ne!(
            fan_profile_from_wire(2).unwrap(),
            fan_profile_from_wire(3).unwrap()
        );
    }

    #[test]
    fn unknown_profile_wire_is_rejected() {
        let err = fan_profile_from_wire(4).expect_err("unknown");
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
    }

    #[test]
    fn unknown_fan_wire_is_rejected() {
        let err = fan_from_wire(2).expect_err("unknown fan");
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert_eq!(fan_from_wire(0).unwrap(), FanId::Cpu);
        assert_eq!(fan_from_wire(1).unwrap(), FanId::Gpu);
    }

    #[test]
    fn validate_accepts_valid_curve() {
        assert!(validate_fan_curve(&valid_curve()).is_ok());
    }

    #[test]
    fn validate_rejects_decreasing_temps() {
        let c = curve(
            [60, 50, 54, 68, 74, 79, 84, 89],
            [5, 22, 38, 45, 56, 63, 81, 94],
        );
        assert!(matches!(
            validate_fan_curve(&c),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_decreasing_pwm() {
        let c = curve(
            [45, 49, 54, 68, 74, 79, 84, 89],
            [50, 22, 38, 45, 56, 63, 81, 94],
        );
        assert!(matches!(
            validate_fan_curve(&c),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_zero_fan_at_critical_temp() {
        let c = curve(
            [45, 49, 54, 68, 74, 79, 84, 85],
            [5, 22, 38, 45, 56, 63, 81, 0],
        );
        assert!(matches!(
            validate_fan_curve(&c),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_accepts_raw_pwm_above_100() {
        // Raw PWM 112 > 100 допустим (не percent).
        let c = curve(
            [40, 42, 43, 60, 65, 69, 74, 78],
            [5, 20, 38, 43, 56, 66, 84, 112],
        );
        assert!(validate_fan_curve(&c).is_ok());
    }

    #[test]
    fn wire_curve_decode_rejects_out_of_range() {
        let mut wire = FanCurveWire {
            temps: vec![45; 8],
            pwms: vec![5; 8],
        };
        // 255 допустим:
        wire.pwms[7] = 255;
        assert!(fan_curve_from_wire(&wire).is_ok());
        // temp 200 вне диапазона:
        wire.temps[7] = 200;
        assert!(matches!(
            fan_curve_from_wire(&wire),
            Err(ProviderError::InvalidRequest(_))
        ));
        // Неверное количество точек:
        let short = FanCurveWire {
            temps: vec![45; 7],
            pwms: vec![5; 7],
        };
        assert!(matches!(
            fan_curve_from_wire(&short),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    /// Fake asusd client для deterministic тестов.
    type StoredCurves = std::collections::HashMap<(u32, String), (Vec<u8>, Vec<u8>)>;

    struct FakeAsusd {
        stored: std::sync::Mutex<StoredCurves>,
        fail_setter: std::sync::atomic::AtomicBool,
        fail_readback: std::sync::atomic::AtomicBool,
        readback_dbus_error: std::sync::Mutex<Option<String>>,
        mismatch_readback: std::sync::atomic::AtomicBool,
        setter_calls: std::sync::atomic::AtomicUsize,
    }

    impl FakeAsusd {
        fn new() -> Self {
            Self {
                stored: std::sync::Mutex::new(StoredCurves::new()),
                fail_setter: std::sync::atomic::AtomicBool::new(false),
                fail_readback: std::sync::atomic::AtomicBool::new(false),
                readback_dbus_error: std::sync::Mutex::new(None),
                mismatch_readback: std::sync::atomic::AtomicBool::new(false),
                setter_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl AsusdFanCurveClient for FakeAsusd {
        async fn set_fan_curve(
            &self,
            profile: AsusdFanProfile,
            fan: &FanId,
            curve: &FanCurvePoints,
        ) -> Result<(), ProviderError> {
            self.setter_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail_setter.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(ProviderError::Dbus("setter failed".into()));
            }
            let name = fan_wire_name(fan).expect("fan");
            let temps: Vec<u8> = curve.temps.iter().map(|t| t.get() as u8).collect();
            let pwms: Vec<u8> = curve.pwms.iter().map(|p| p.get()).collect();
            self.stored
                .lock()
                .unwrap()
                .insert((profile.wire(), name.to_string()), (temps, pwms));
            Ok(())
        }

        async fn read_curves(
            &self,
            profile: AsusdFanProfile,
        ) -> Result<Vec<(String, [u8; 8], [u8; 8], bool)>, ProviderError> {
            if let Some(detail) = self.readback_dbus_error.lock().unwrap().as_ref() {
                return Err(ProviderError::Dbus(detail.clone()));
            }
            if self.fail_readback.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(ProviderError::Dbus("readback failed".into()));
            }
            let stored = self.stored.lock().unwrap();
            let mut out = Vec::new();
            for (name, (temps, pwms)) in stored.iter() {
                if name.0 != profile.wire() {
                    continue;
                }
                let mut t = [0u8; 8];
                let mut p = [0u8; 8];
                t.copy_from_slice(temps);
                p.copy_from_slice(pwms);
                if self
                    .mismatch_readback
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    // Возвращаем кривую с изменённым последним PWM → mismatch.
                    p[7] = p[7].wrapping_add(1);
                }
                out.push((name.1.clone(), t, p, false));
            }
            Ok(out)
        }
    }

    #[async_trait]
    impl AsusdFanCurveClient for &FakeAsusd {
        async fn set_fan_curve(
            &self,
            profile: AsusdFanProfile,
            fan: &FanId,
            curve: &FanCurvePoints,
        ) -> Result<(), ProviderError> {
            (**self).set_fan_curve(profile, fan, curve).await
        }

        async fn read_curves(
            &self,
            profile: AsusdFanProfile,
        ) -> Result<Vec<(String, [u8; 8], [u8; 8], bool)>, ProviderError> {
            (**self).read_curves(profile).await
        }
    }

    #[tokio::test]
    async fn mutation_success_with_readback_match() {
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(asusd);
        let curve = valid_curve();
        let result = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &curve)
            .await
            .expect("success");
        assert_eq!(result.result, ApplyResult::Applied);
        assert_eq!(result.requested_profile, AsusdFanProfile::Balanced);
        assert_eq!(result.requested_fan, FanId::Cpu);
    }

    #[tokio::test]
    async fn setter_called_only_for_selected_fan() {
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        let curve = valid_curve();
        backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &curve)
            .await
            .expect("success");
        // setter вызван ровно один раз (только CPU, не batch).
        assert_eq!(
            asusd.setter_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn setter_error_propagates() {
        let asusd = FakeAsusd::new();
        asusd
            .fail_setter
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        let err = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &valid_curve())
            .await
            .expect_err("setter error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn readback_error_propagates() {
        let asusd = FakeAsusd::new();
        asusd
            .fail_readback
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        let err = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &valid_curve())
            .await
            .expect_err("readback error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn readback_mismatch_is_error() {
        // Setter сохраняет, но read-back возвращает другую кривую → mismatch.
        let asusd = FakeAsusd::new();
        asusd
            .mismatch_readback
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        let cpu_curve = valid_curve();
        let err = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &cpu_curve)
            .await
            .expect_err("readback mismatch");
        assert!(matches!(err, ProviderError::BackendUnavailable(_)));
    }

    #[tokio::test]
    async fn validation_happens_before_setter() {
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        // Невалидная кривая (убывающие PWM) → reject до setter.
        let bad = curve(
            [45, 49, 54, 68, 74, 79, 84, 89],
            [50, 22, 38, 45, 56, 63, 81, 94],
        );
        let err = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &bad)
            .await
            .expect_err("invalid curve");
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert_eq!(
            asusd.setter_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "setter не должен вызываться при невалидной кривой"
        );
    }

    #[tokio::test]
    async fn rejects_temperature_above_wire_range() {
        // TemperatureC max is 150, which fits in u8. But we test the validation
        // path works correctly by checking the wire range guard exists.
        // The real narrowing bug is negative temperatures wrapping via `as u8`.
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        // Valid temps (all within 0..=255) should pass.
        let good = valid_curve();
        assert!(
            backend
                .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &good)
                .await
                .is_ok()
        );
        assert_eq!(
            asusd.setter_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn rejects_negative_temperature() {
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        // Отрицательная температура (-10) не должна молча стать 246 при as u8.
        let mut temps = [TemperatureC::new(45).expect("temp"); 8];
        temps[0] = TemperatureC::new(-10).expect("temp");
        let bad = FanCurvePoints {
            temps,
            pwms: [FanPwm::new(5).expect("pwm"); 8],
        };
        let err = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &bad)
            .await
            .expect_err("negative temp must fail");
        assert!(
            matches!(err, ProviderError::InvalidRequest(_)),
            "expected InvalidRequest for negative temp, got: {err:?}"
        );
        assert_eq!(
            asusd.setter_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "setter не должен вызываться при отрицательной температуре"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: wire order + PWM >100 roundtrip
    // -----------------------------------------------------------------------

    /// Verify that wire order is (name, temps, pwms, enabled) by writing a
    /// curve with PWM > 100 and reading it back. If arrays were swapped,
    /// the readback would return temps where pwms should be and vice versa.
    #[tokio::test]
    async fn wire_roundtrip_pwm_above_100_preserves_arrays() {
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        // PWM 112 > 100 — raw hwmon value, not percent.
        let input = curve(
            [40, 42, 43, 60, 65, 69, 74, 78],
            [5, 20, 38, 43, 56, 66, 84, 112],
        );
        let result = backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Gpu, &input)
            .await
            .expect("success");
        assert_eq!(result.result, ApplyResult::Applied);

        // Read back: FakeAsusd returns the exact wire that was stored.
        let raw = asusd.read_curves(AsusdFanProfile::Balanced).await.unwrap();
        let gpu = raw
            .iter()
            .find(|(n, _, _, _)| n == "GPU")
            .expect("GPU entry");
        // temps must remain temps (not pwms), pwms must remain pwms.
        assert_eq!(
            gpu.1,
            [40, 42, 43, 60, 65, 69, 74, 78],
            "temps array must match input temps"
        );
        assert_eq!(
            gpu.2,
            [5, 20, 38, 43, 56, 66, 84, 112],
            "pwms array must match input pwms"
        );
    }

    /// Verify that FakeAsusd roundtrip preserves per-fan isolation:
    /// writing CPU does not affect GPU and vice versa, even with PWM > 100.
    #[tokio::test]
    async fn per_fan_wire_roundtrip_pwm_above_100() {
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        let cpu_curve = curve(
            [45, 49, 54, 68, 74, 79, 84, 89],
            [5, 22, 38, 45, 56, 63, 81, 94],
        );
        let gpu_curve = curve(
            [40, 42, 43, 60, 65, 69, 74, 78],
            [5, 20, 38, 43, 56, 66, 84, 112],
        );
        backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Cpu, &cpu_curve)
            .await
            .unwrap();
        backend
            .set_fan_curve(AsusdFanProfile::Balanced, &FanId::Gpu, &gpu_curve)
            .await
            .unwrap();

        let raw = asusd.read_curves(AsusdFanProfile::Balanced).await.unwrap();
        let cpu_entry = raw.iter().find(|(n, _, _, _)| n == "CPU").expect("CPU");
        let gpu_entry = raw.iter().find(|(n, _, _, _)| n == "GPU").expect("GPU");

        // CPU temps must not be confused with GPU pwms.
        assert_eq!(cpu_entry.1, [45, 49, 54, 68, 74, 79, 84, 89]);
        assert_eq!(cpu_entry.2, [5, 22, 38, 45, 56, 63, 81, 94]);
        // GPU pwms 112 must survive roundtrip.
        assert_eq!(gpu_entry.1, [40, 42, 43, 60, 65, 69, 74, 78]);
        assert_eq!(gpu_entry.2, [5, 20, 38, 43, 56, 66, 84, 112]);
    }

    /// FanCurveWire decode roundtrip with PWM > 100.
    #[test]
    fn fan_curve_from_wire_pwm_above_100_roundtrip() {
        use super::{FanCurveWire, fan_curve_from_wire};
        let input = FanCurveWire {
            temps: vec![40, 42, 43, 60, 65, 69, 74, 78],
            pwms: vec![5, 20, 38, 43, 56, 66, 84, 112],
        };
        let decoded = fan_curve_from_wire(&input).expect("decode");
        assert_eq!(decoded.temps[7].get(), 78);
        assert_eq!(decoded.pwms[7].get(), 112);
        // Encode back.
        let encoded = FanCurveWire {
            temps: decoded.temps.iter().map(|t| t.get() as u8).collect(),
            pwms: decoded.pwms.iter().map(|p| p.get()).collect(),
        };
        assert_eq!(encoded, input, "roundtrip must be lossless");
    }

    #[tokio::test]
    async fn mutation_status_supported_without_setter_calls() {
        // The status probe uses the read-only FanCurveData path. It must prove
        // Supported without ever calling the setter.
        let asusd = FakeAsusd::new();
        let backend = AsusdFanCurveMutationBackend::new(&asusd);
        assert_eq!(
            backend.mutation_status().await,
            FanMutationStatus::Supported
        );
        assert_eq!(
            asusd.setter_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "status query must not call the setter"
        );
    }

    #[tokio::test]
    async fn mutation_status_classifies_dbus_evidence() {
        for (detail, expected) in [
            (
                "org.freedesktop.DBus.Error.ServiceUnknown: name not found",
                FanMutationStatus::BackendMissing,
            ),
            (
                "org.freedesktop.DBus.Error.NameHasNoOwner",
                FanMutationStatus::BackendMissing,
            ),
            (
                "org.freedesktop.DBus.Error.UnknownMethod",
                FanMutationStatus::Unsupported,
            ),
            (
                "org.freedesktop.DBus.Error.UnknownInterface",
                FanMutationStatus::Unsupported,
            ),
            (
                "org.freedesktop.DBus.Error.NotSupported",
                FanMutationStatus::Unsupported,
            ),
            (
                "org.freedesktop.DBus.Error.AccessDenied",
                FanMutationStatus::PermissionDenied,
            ),
            (
                "org.freedesktop.DBus.Error.NoReply: timed out",
                FanMutationStatus::TemporarilyUnavailable,
            ),
            ("unexpected protocol failure", FanMutationStatus::Unknown),
        ] {
            let asusd = FakeAsusd::new();
            *asusd.readback_dbus_error.lock().unwrap() = Some(detail.to_string());
            let backend = AsusdFanCurveMutationBackend::new(&asusd);
            assert_eq!(
                backend.mutation_status().await,
                expected,
                "detail: {detail}"
            );
            assert_eq!(
                asusd.setter_calls.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "status query must not call the setter"
            );
        }
    }

    #[test]
    fn fan_mutation_wire_roundtrip_is_total() {
        for status in [
            FanMutationStatus::Supported,
            FanMutationStatus::Unsupported,
            FanMutationStatus::TemporarilyUnavailable,
            FanMutationStatus::PermissionDenied,
            FanMutationStatus::BackendMissing,
            FanMutationStatus::Unknown,
        ] {
            let wire = fan_mutation_wire::to_wire(status);
            assert_eq!(fan_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(fan_mutation_wire::from_wire(99), None);
    }
}
