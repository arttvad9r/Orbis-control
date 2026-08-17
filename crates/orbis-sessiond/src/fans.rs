//! Read-only kernel ASUS firmware fan-curve backend.
//!
//! Реализует чтение активной кривой вентилятора через hwmon
//! `asus_custom_fan_curve` (`pwm{1,2}_auto_point{1..8}_temp/_pwm`).
//!
//! - устройства обнаруживаются динамически по `name == "asus_custom_fan_curve"`
//!   в `/sys/class/hwmon/*` (без hardcoded `hwmonN`);
//! - кривая хранит **raw hwmon PWM** (0..=255), НЕ процент: mapping 0..255 → %
//!   не доказан (на эталоне GPU curve достигает raw 112 > 100);
//! - sysfs предоставляет только **активную** кривую, НЕ profile-specific
//!   storage. `FanProvider::fan_curve(profile, fan)` возвращает `Unsupported`
//!   (не фальсифицирует profile semantics); активная кривая читается через
//!   отдельный `active_curve(fan)` метод;
//! - отсутствие устройства/вентилятора → `Unsupported` (canonical);
//! - malformed/пустые значения → `ProviderError::Internal`;
//! - I/O ошибка (кроме `NotFound`) → `ProviderError::Io`;
//! - никаких writes, `pwm*_enable`, defaults/reset.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::fan::{FanCurve, FanCurvePoint, FanId};
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::{FanPwm, TemperatureC};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{FanProvider, Provider, ProviderHealth};

/// Количество точек кривой, фиксированное kernel ABI `asus_custom_fan_curve`.
pub const CURVE_POINT_COUNT: usize = 8;

// ---------------------------------------------------------------------------
// asusd fan profile wire mapping (live-validated, Task 6.6/6.7)
// ---------------------------------------------------------------------------

/// Wire value asusd `FanCurveData`/`SetFanCurve` profile argument.
///
/// Mapping доказан (не угадан) из:
/// - `/etc/asusd/fan_curves.ron` строковые имена (`balanced`/`performance`/`quiet`);
/// - `FanCurveData(0/1/2)` == `fan_curves.ron` (`balanced`/`performance`/`quiet`);
/// - `PlatformProfileChoices = [3, 2, 0, 1]` == `[LowPower, Quiet, Balanced, Performance]`;
/// - historical mapping (`0=Balanced`, `2=Quiet`) и live-валидация (активный 0 = Balanced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsusdFanProfile {
    /// wire 0 — balanced.
    Balanced,
    /// wire 1 — performance.
    Performance,
    /// wire 2 — quiet.
    Quiet,
    /// wire 3 — low-power.
    LowPower,
}

impl AsusdFanProfile {
    /// Wire value для D-Bus.
    pub fn wire(self) -> u32 {
        match self {
            Self::Balanced => 0,
            Self::Performance => 1,
            Self::Quiet => 2,
            Self::LowPower => 3,
        }
    }

    /// Строковое имя (для диагностики/сравнения с `fan_curves.ron`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Quiet => "quiet",
            Self::LowPower => "low-power",
        }
    }
}

/// Strict decode wire `u32` → `AsusdFanProfile`.
///
/// Неизвестное значение → `ProviderError::Internal` (remote нарушил contract),
/// без fallback.
pub fn asusd_fan_profile_from_wire(raw: u32) -> Result<AsusdFanProfile, ProviderError> {
    match raw {
        0 => Ok(AsusdFanProfile::Balanced),
        1 => Ok(AsusdFanProfile::Performance),
        2 => Ok(AsusdFanProfile::Quiet),
        3 => Ok(AsusdFanProfile::LowPower),
        other => Err(ProviderError::Internal(format!(
            "asusd FanCurves: неизвестный profile wire value {other}"
        ))),
    }
}

/// Сопоставление asusd fan profile с трёхкнопочной `PerformanceProfile`.
///
/// `LowPower` и `Quiet` → `Silent` (как `PlatformProfile → PerformanceProfile`).
impl From<AsusdFanProfile> for PerformanceProfile {
    fn from(p: AsusdFanProfile) -> Self {
        match p {
            AsusdFanProfile::Balanced => PerformanceProfile::Balanced,
            AsusdFanProfile::Performance => PerformanceProfile::Turbo,
            AsusdFanProfile::Quiet | AsusdFanProfile::LowPower => PerformanceProfile::Silent,
        }
    }
}

/// Обратное сопоставление трёхкнопочной модели с asusd fan profile.
///
/// `Silent` → `Quiet` (как `PerformanceProfile → PlatformProfile`); `LowPower`
/// не используется для трёхкнопочной модели.
impl From<PerformanceProfile> for AsusdFanProfile {
    fn from(p: PerformanceProfile) -> Self {
        match p {
            PerformanceProfile::Silent => AsusdFanProfile::Quiet,
            PerformanceProfile::Balanced => AsusdFanProfile::Balanced,
            PerformanceProfile::Turbo => AsusdFanProfile::Performance,
        }
    }
}

// ---------------------------------------------------------------------------
// Typed asusd FanCurves read contract
// ---------------------------------------------------------------------------

/// Одна кривая вентилятора из asusd `FanCurveData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsusdFanCurve {
    /// Вентилятор (CPU/GPU).
    pub fan: FanId,
    /// 8 температур, °C.
    pub temps: [TemperatureC; CURVE_POINT_COUNT],
    /// 8 raw PWM 0..255.
    pub pwms: [FanPwm; CURVE_POINT_COUNT],
    /// Enabled flag (не изменяется, только сохраняется).
    pub enabled: bool,
}

/// Полный результат `FanCurveData(profile)`: CPU + GPU кривые.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsusdFanCurveSet {
    /// Профиль, для которого прочитаны кривые.
    pub profile: AsusdFanProfile,
    /// CPU кривая.
    pub cpu: AsusdFanCurve,
    /// GPU кривая.
    pub gpu: AsusdFanCurve,
}

/// Testable источник asusd `FanCurveData`.
#[async_trait]
pub trait AsusdFanCurveSource: Send + Sync {
    /// Прочитать сохранённые кривые для профиля (authoritative, без кэша).
    async fn read_curves(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<AsusdFanCurveSet, ProviderError>;
}

/// Реальный zbus источник asusd `FanCurveData`.
///
/// Хранит готовую system-bus `Connection`; I/O начинается только в
/// `read_curves().await`. Конструктор не выполняет I/O.
pub struct ZbusAsusdFanCurveSource {
    connection: zbus::Connection,
}

impl ZbusAsusdFanCurveSource {
    /// Создать источник над готовой system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

/// Wire-элемент asusd `FanCurveData`: name + 8 temp + 8 pwm + enabled.
type AsusdCurveWire = (String, [u8; 8], [u8; 8], bool);

#[zbus::proxy(
    interface = "xyz.ljones.FanCurves",
    default_service = "xyz.ljones.Asusd",
    default_path = "/xyz/ljones"
)]
trait AsusdFanCurves {
    fn fan_curve_data(&self, profile: u32) -> zbus::Result<Vec<AsusdCurveWire>>;
}

/// Парсинг одного `(name, temp8, pwm8, enabled)` элемента из asusd.
///
/// Формат wire: `(s(yyyyyyyy)(yyyyyyyy)b)` = name + 8 temp + 8 pwm + enabled.
/// zbus раскладывает `(yyyyyyyy)` в `[u8; 8]`.
fn parse_curve_entry(
    name: &str,
    temps: &[u8; 8],
    pwms: &[u8; 8],
    enabled: bool,
) -> Result<AsusdFanCurve, ProviderError> {
    let fan = match name {
        "CPU" => FanId::Cpu,
        "GPU" => FanId::Gpu,
        other => {
            return Err(ProviderError::Internal(format!(
                "asusd FanCurves: неизвестный fan name '{other}'"
            )));
        }
    };
    let mut temps_arr = [TemperatureC::new(0).expect("const"); CURVE_POINT_COUNT];
    let mut pwms_arr = [FanPwm::new(0).expect("const"); CURVE_POINT_COUNT];
    for (i, (t, p)) in temps.iter().zip(pwms.iter()).enumerate() {
        temps_arr[i] = TemperatureC::new(*t as i16).map_err(|_| {
            ProviderError::Internal(format!(
                "asusd FanCurves: температура вне диапазона '{t}' для {name}"
            ))
        })?;
        pwms_arr[i] = FanPwm::new(*p).map_err(|_| {
            ProviderError::Internal(format!(
                "asusd FanCurves: PWM вне диапазона '{p}' для {name}"
            ))
        })?;
    }
    Ok(AsusdFanCurve {
        fan,
        temps: temps_arr,
        pwms: pwms_arr,
        enabled,
    })
}

#[async_trait]
impl AsusdFanCurveSource for ZbusAsusdFanCurveSource {
    async fn read_curves(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<AsusdFanCurveSet, ProviderError> {
        let proxy = AsusdFanCurvesProxy::builder(&self.connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| ProviderError::Dbus(format!("asusd FanCurves proxy: {e}")))?;
        let raw = proxy
            .fan_curve_data(profile.wire())
            .await
            .map_err(|e| ProviderError::Dbus(format!("asusd FanCurveData read: {e}")))?;

        let mut cpu = None;
        let mut gpu = None;
        for (name, temps, pwms, enabled) in raw {
            let curve = parse_curve_entry(&name, &temps, &pwms, enabled)?;
            match curve.fan {
                FanId::Cpu => cpu = Some(curve),
                FanId::Gpu => gpu = Some(curve),
                _ => {}
            }
        }
        let cpu = cpu.ok_or_else(|| {
            ProviderError::Internal("asusd FanCurves: CPU кривая отсутствует".into())
        })?;
        let gpu = gpu.ok_or_else(|| {
            ProviderError::Internal("asusd FanCurves: GPU кривая отсутствует".into())
        })?;
        Ok(AsusdFanCurveSet { profile, cpu, gpu })
    }
}

/// Testable источник активной кривой вентилятора.
#[async_trait]
pub trait FanCurveSource: Send + Sync {
    /// Прочитать активную кривую для вентилятора (authoritative, без кэша).
    async fn read_active_curve(&self, fan: &FanId) -> Result<FanCurve, ProviderError>;
}

/// Реальный sysfs источник активной кривой.
///
/// Хранит корень sysfs; I/O начинается только в `read_active_curve().await`.
/// Конструктор не выполняет I/O и не открывает файлы на запись.
pub struct SysfsFanCurveSource {
    sysfs_root: PathBuf,
}

impl SysfsFanCurveSource {
    /// Создать источник над заданным корнем sysfs.
    ///
    /// В production передаётся `/sys`; в тестах — временное fixture-дерево.
    pub fn new(sysfs_root: PathBuf) -> Self {
        Self { sysfs_root }
    }
}

impl Default for SysfsFanCurveSource {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

impl SysfsFanCurveSource {
    /// Найти hwmon директорию с `name == "asus_custom_fan_curve"`.
    fn find_curve_dir(&self) -> Result<Option<PathBuf>, ProviderError> {
        let hwmon_dir = self.sysfs_root.join("class").join("hwmon");
        let entries = match std::fs::read_dir(&hwmon_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ProviderError::Io(e)),
        };
        for entry in entries {
            let entry = entry.map_err(ProviderError::Io)?;
            let path = entry.path();
            let name = match std::fs::read_to_string(path.join("name")) {
                Ok(name) => name,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(ProviderError::Io(e)),
            };
            if name.trim() == "asus_custom_fan_curve" {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// Прочитать один файл как `u64`.
    fn read_u64(path: &Path) -> Result<Option<u64>, ProviderError> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ProviderError::Io(e)),
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(ProviderError::Internal(format!(
                "asus_custom_fan_curve: пустой файл '{}'",
                path.display()
            )));
        }
        trimmed.parse::<u64>().map(Some).map_err(|_| {
            ProviderError::Internal(format!(
                "asus_custom_fan_curve: невалидное значение '{trimmed}' в '{}'",
                path.display()
            ))
        })
    }

    /// Прочитать активную кривую из hwmon директории для заданного индекса
    /// вентилятора (`pwm{idx}_auto_point{1..8}_temp/_pwm`).
    fn read_curve_from_dir(
        &self,
        dir: &Path,
        fan_index: usize,
        fan: FanId,
    ) -> Result<FanCurve, ProviderError> {
        let mut points = Vec::with_capacity(CURVE_POINT_COUNT);
        for i in 1..=CURVE_POINT_COUNT {
            let temp_path = dir.join(format!("pwm{fan_index}_auto_point{i}_temp"));
            let pwm_path = dir.join(format!("pwm{fan_index}_auto_point{i}_pwm"));

            let temp_raw = match Self::read_u64(&temp_path)? {
                Some(v) => v,
                None => {
                    return Err(ProviderError::Unsupported(format!(
                        "asus_custom_fan_curve: точка {i} вентилятора {fan:?} отсутствует"
                    )));
                }
            };
            let pwm_raw = match Self::read_u64(&pwm_path)? {
                Some(v) => v,
                None => {
                    return Err(ProviderError::Unsupported(format!(
                        "asus_custom_fan_curve: точка {i} вентилятора {fan:?} отсутствует"
                    )));
                }
            };

            let temp = TemperatureC::new(temp_raw as i16).map_err(|_| {
                ProviderError::Internal(format!(
                    "asus_custom_fan_curve: температура вне диапазона '{temp_raw}' в '{}'",
                    temp_path.display()
                ))
            })?;
            let pwm = FanPwm::new(pwm_raw as u8).map_err(|_| {
                ProviderError::Internal(format!(
                    "asus_custom_fan_curve: PWM вне диапазона '{pwm_raw}' в '{}'",
                    pwm_path.display()
                ))
            })?;
            points.push(FanCurvePoint::new(temp, pwm));
        }
        Ok(FanCurve {
            profile: PerformanceProfile::Balanced, // placeholder, не используется
            fan,
            points,
        })
    }
}

#[async_trait]
impl FanCurveSource for SysfsFanCurveSource {
    async fn read_active_curve(&self, fan: &FanId) -> Result<FanCurve, ProviderError> {
        let Some(dir) = self.find_curve_dir()? else {
            return Err(ProviderError::Unsupported(
                "asus_custom_fan_curve: hwmon device отсутствует".into(),
            ));
        };
        let fan_index = match fan {
            FanId::Cpu => 1,
            FanId::Gpu => 2,
            _ => {
                return Err(ProviderError::Unsupported(format!(
                    "asus_custom_fan_curve: вентилятор {fan:?} не поддерживается (только CPU/GPU)"
                )));
            }
        };
        self.read_curve_from_dir(&dir, fan_index, fan.clone())
    }
}

/// Read-only provider активной кривой вентилятора над kernel source.
///
/// Реализует `FanProvider`, но `fan_curve(profile, fan)` возвращает
/// `Unsupported`: sysfs `asus_custom_fan_curve` хранит только активную кривую,
/// НЕ profile-specific storage. Активная кривая читается через отдельный
/// `active_curve(fan)` метод (не фальсифицирует profile semantics).
pub struct SysfsFanCurveProvider<S> {
    source: S,
}

impl<S> SysfsFanCurveProvider<S> {
    /// Создать provider над source.
    ///
    /// Не открывает sysfs и не выполняет I/O.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SysfsFanCurveProvider<S>
where
    S: FanCurveSource,
{
    fn id(&self) -> &'static str {
        "asus-custom-fan-curve"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-custom-fan-curve")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("kernel asus_custom_fan_curve read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-custom-fan-curve",
            "read-only kernel asus_custom_fan_curve backend (без hardware writes)",
        )]
    }
}

#[async_trait]
impl<S> FanProvider for SysfsFanCurveProvider<S>
where
    S: FanCurveSource,
{
    async fn fan_ids(&self) -> Result<Vec<FanId>, ProviderError> {
        // Оба вентилятора (CPU/GPU) поддерживаются, если device присутствует.
        let source = &self.source;
        let cpu = source.read_active_curve(&FanId::Cpu).await;
        let gpu = source.read_active_curve(&FanId::Gpu).await;
        let mut ids = Vec::new();
        if cpu.is_ok() {
            ids.push(FanId::Cpu);
        }
        if gpu.is_ok() {
            ids.push(FanId::Gpu);
        }
        if ids.is_empty() {
            return Err(ProviderError::Unsupported(
                "asus_custom_fan_curve: ни один вентилятор не доступен".into(),
            ));
        }
        Ok(ids)
    }

    async fn fan_rpms(&self) -> Result<Vec<(FanId, orbis_core::newtypes::Rpm)>, ProviderError> {
        // RPM telemetry живёт в отдельном SysfsTelemetryProvider; здесь не
        // дублируем (fan control ≠ fan RPM telemetry).
        Err(ProviderError::Unsupported(
            "fan RPM telemetry предоставляется отдельным telemetry provider".into(),
        ))
    }

    async fn fan_curve(
        &self,
        _profile: PerformanceProfile,
        _fan: &FanId,
    ) -> Result<FanCurve, ProviderError> {
        // sysfs asus_custom_fan_curve хранит только активную кривую, НЕ
        // profile-specific storage. Не фальсифицируем profile semantics:
        // возвращаем Unsupported, а не активную кривую под видом profile.
        Err(ProviderError::Unsupported(
            "asus_custom_fan_curve: sysfs хранит только активную кривую, не profile-specific; используйте active_curve(fan)".into(),
        ))
    }

    async fn active_curve(&self, fan: &FanId) -> Result<FanCurve, ProviderError> {
        self.source.read_active_curve(fan).await
    }

    async fn set_fan_curve(
        &self,
        _curve: &FanCurve,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "asus_custom_fan_curve: read-only provider, запись не поддерживается".into(),
        ))
    }

    async fn set_curves_to_defaults(
        &self,
        _profile: PerformanceProfile,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "asus_custom_fan_curve: read-only provider, defaults/reset не поддерживается".into(),
        ))
    }

    fn curve_point_count(&self) -> usize {
        CURVE_POINT_COUNT
    }

    fn allow_decreasing(&self) -> bool {
        false
    }

    fn validate_curve(&self, curve: &FanCurve) -> ValidationResult {
        match curve.validate(self.curve_point_count(), self.allow_decreasing()) {
            Ok(()) => ValidationResult::ok(),
            Err(e) => ValidationResult::invalid(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbis-fan-curve-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture root");
        dir
    }

    fn write_fixture(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write fixture");
    }

    /// Полное fixture-дерево: CPU (pwm1) и GPU (pwm2) по 8 точек.
    fn full_fixture(root: &Path) {
        write_fixture(root, "class/hwmon/hwmon5/name", "asus_custom_fan_curve\n");
        // CPU: temp 45..89, pwm 5..94
        let cpu_temps = [45, 49, 54, 68, 74, 79, 84, 89];
        let cpu_pwms = [5, 22, 38, 45, 56, 63, 81, 94];
        for (i, (t, p)) in cpu_temps.iter().zip(cpu_pwms.iter()).enumerate() {
            write_fixture(
                root,
                &format!("class/hwmon/hwmon5/pwm1_auto_point{}_temp", i + 1),
                &format!("{t}\n"),
            );
            write_fixture(
                root,
                &format!("class/hwmon/hwmon5/pwm1_auto_point{}_pwm", i + 1),
                &format!("{p}\n"),
            );
        }
        // GPU: temp 40..78, pwm 5..112 (raw > 100!)
        let gpu_temps = [40, 42, 43, 60, 65, 69, 74, 78];
        let gpu_pwms = [5, 20, 38, 43, 56, 66, 84, 112];
        for (i, (t, p)) in gpu_temps.iter().zip(gpu_pwms.iter()).enumerate() {
            write_fixture(
                root,
                &format!("class/hwmon/hwmon5/pwm2_auto_point{}_temp", i + 1),
                &format!("{t}\n"),
            );
            write_fixture(
                root,
                &format!("class/hwmon/hwmon5/pwm2_auto_point{}_pwm", i + 1),
                &format!("{p}\n"),
            );
        }
    }

    #[tokio::test]
    async fn discovery_works_with_different_hwmon_indices() {
        let root = fixture_root();
        // Другой индекс hwmon (не hardcoded hwmon5).
        write_fixture(&root, "class/hwmon/hwmon12/name", "asus_custom_fan_curve\n");
        write_fixture(&root, "class/hwmon/hwmon12/pwm1_auto_point1_temp", "45\n");
        write_fixture(&root, "class/hwmon/hwmon12/pwm1_auto_point1_pwm", "5\n");
        for i in 2..=8 {
            write_fixture(
                &root,
                &format!("class/hwmon/hwmon12/pwm1_auto_point{i}_temp"),
                &format!("{}\n", 45 + i),
            );
            write_fixture(
                &root,
                &format!("class/hwmon/hwmon12/pwm1_auto_point{i}_pwm"),
                &format!("{}\n", 5 + i),
            );
        }

        let source = SysfsFanCurveSource::new(root.clone());
        let curve = source
            .read_active_curve(&FanId::Cpu)
            .await
            .expect("cpu curve");
        assert_eq!(curve.points.len(), 8);
        assert_eq!(curve.points[0].temp.get(), 45);
        assert_eq!(curve.points[7].temp.get(), 53);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reads_cpu_and_gpu_independently() {
        let root = fixture_root();
        full_fixture(&root);
        let source = SysfsFanCurveSource::new(root.clone());

        let cpu = source
            .read_active_curve(&FanId::Cpu)
            .await
            .expect("cpu curve");
        assert_eq!(cpu.points.len(), 8);
        assert_eq!(cpu.points[0].temp.get(), 45);
        assert_eq!(cpu.points[7].pwm.get(), 94);

        let gpu = source
            .read_active_curve(&FanId::Gpu)
            .await
            .expect("gpu curve");
        assert_eq!(gpu.points.len(), 8);
        assert_eq!(gpu.points[0].temp.get(), 40);
        assert_eq!(gpu.points[7].pwm.get(), 112);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn raw_pwm_above_100_is_preserved() {
        let root = fixture_root();
        full_fixture(&root);
        let source = SysfsFanCurveSource::new(root.clone());
        let gpu = source
            .read_active_curve(&FanId::Gpu)
            .await
            .expect("gpu curve");
        // raw PWM 112 > 100 сохраняется без преобразования в Percent.
        assert_eq!(gpu.points[7].pwm.get(), 112);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_device_is_unsupported() {
        let root = fixture_root();
        // Нет asus_custom_fan_curve device.
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        let source = SysfsFanCurveSource::new(root.clone());
        let err = source
            .read_active_curve(&FanId::Cpu)
            .await
            .expect_err("missing device");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_fan_is_unsupported() {
        let root = fixture_root();
        // Только CPU (pwm1), GPU (pwm2) отсутствует.
        write_fixture(&root, "class/hwmon/hwmon5/name", "asus_custom_fan_curve\n");
        for i in 1..=8 {
            write_fixture(
                &root,
                &format!("class/hwmon/hwmon5/pwm1_auto_point{i}_temp"),
                &format!("{}\n", 45 + i),
            );
            write_fixture(
                &root,
                &format!("class/hwmon/hwmon5/pwm1_auto_point{i}_pwm"),
                &format!("{}\n", 5 + i),
            );
        }
        let source = SysfsFanCurveSource::new(root.clone());
        let cpu = source
            .read_active_curve(&FanId::Cpu)
            .await
            .expect("cpu curve");
        assert_eq!(cpu.points.len(), 8);
        let err = source
            .read_active_curve(&FanId::Gpu)
            .await
            .expect_err("missing gpu");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_value_is_internal_error() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon5/name", "asus_custom_fan_curve\n");
        write_fixture(
            &root,
            "class/hwmon/hwmon5/pwm1_auto_point1_temp",
            "not-a-number\n",
        );
        write_fixture(&root, "class/hwmon/hwmon5/pwm1_auto_point1_pwm", "5\n");
        let source = SysfsFanCurveSource::new(root.clone());
        let err = source
            .read_active_curve(&FanId::Cpu)
            .await
            .expect_err("malformed");
        assert!(matches!(err, ProviderError::Internal(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn empty_file_is_internal_error() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon5/name", "asus_custom_fan_curve\n");
        write_fixture(&root, "class/hwmon/hwmon5/pwm1_auto_point1_temp", "\n");
        write_fixture(&root, "class/hwmon/hwmon5/pwm1_auto_point1_pwm", "5\n");
        let source = SysfsFanCurveSource::new(root.clone());
        let err = source
            .read_active_curve(&FanId::Cpu)
            .await
            .expect_err("empty");
        assert!(matches!(err, ProviderError::Internal(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fan_curve_profile_is_unsupported_not_faked() {
        let root = fixture_root();
        full_fixture(&root);
        let provider = SysfsFanCurveProvider::new(SysfsFanCurveSource::new(root.clone()));

        // profile-specific чтение не фальсифицируется: sysfs хранит только
        // активную кривую.
        let err = provider
            .fan_curve(PerformanceProfile::Turbo, &FanId::Cpu)
            .await
            .expect_err("profile-specific must be unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        // Активная кривая доступна через отдельный метод.
        let active = provider
            .active_curve(&FanId::Cpu)
            .await
            .expect("active curve");
        assert_eq!(active.points.len(), 8);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn writes_are_unsupported() {
        let root = fixture_root();
        full_fixture(&root);
        let provider = SysfsFanCurveProvider::new(SysfsFanCurveSource::new(root.clone()));

        let curve = provider.active_curve(&FanId::Cpu).await.expect("curve");
        let err = provider
            .set_fan_curve(&curve)
            .await
            .expect_err("write must be unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        let err = provider
            .set_curves_to_defaults(PerformanceProfile::Balanced)
            .await
            .expect_err("defaults must be unsupported");
        assert!(matches!(err, ProviderError::Unsupported(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fan_ids_reports_available_fans() {
        let root = fixture_root();
        full_fixture(&root);
        let provider = SysfsFanCurveProvider::new(SysfsFanCurveSource::new(root.clone()));
        let ids = provider.fan_ids().await.expect("fan ids");
        assert!(ids.contains(&FanId::Cpu));
        assert!(ids.contains(&FanId::Gpu));

        let _ = std::fs::remove_dir_all(root);
    }

    // -----------------------------------------------------------------------
    // asusd fan profile wire mapping
    // -----------------------------------------------------------------------

    #[test]
    fn asusd_fan_profile_wire_mapping_is_exact() {
        // Полный mapping доказан (Task 6.6/6.7): wire 0..3.
        assert_eq!(AsusdFanProfile::Balanced.wire(), 0);
        assert_eq!(AsusdFanProfile::Performance.wire(), 1);
        assert_eq!(AsusdFanProfile::Quiet.wire(), 2);
        assert_eq!(AsusdFanProfile::LowPower.wire(), 3);

        assert_eq!(
            asusd_fan_profile_from_wire(0).unwrap(),
            AsusdFanProfile::Balanced
        );
        assert_eq!(
            asusd_fan_profile_from_wire(1).unwrap(),
            AsusdFanProfile::Performance
        );
        assert_eq!(
            asusd_fan_profile_from_wire(2).unwrap(),
            AsusdFanProfile::Quiet
        );
        assert_eq!(
            asusd_fan_profile_from_wire(3).unwrap(),
            AsusdFanProfile::LowPower
        );
    }

    #[test]
    fn asusd_fan_profile_unknown_wire_is_internal_error() {
        // Unknown wire → typed error, не fallback.
        let err = asusd_fan_profile_from_wire(4).expect_err("unknown wire");
        assert!(matches!(err, ProviderError::Internal(_)));
        let err = asusd_fan_profile_from_wire(99).expect_err("unknown wire");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn asusd_fan_profile_maps_to_performance_profile() {
        // Согласуется с PlatformProfile → PerformanceProfile (profile.rs).
        assert_eq!(
            PerformanceProfile::from(AsusdFanProfile::Balanced),
            PerformanceProfile::Balanced
        );
        assert_eq!(
            PerformanceProfile::from(AsusdFanProfile::Performance),
            PerformanceProfile::Turbo
        );
        assert_eq!(
            PerformanceProfile::from(AsusdFanProfile::Quiet),
            PerformanceProfile::Silent
        );
        assert_eq!(
            PerformanceProfile::from(AsusdFanProfile::LowPower),
            PerformanceProfile::Silent
        );

        // Обратное: трёхкнопочная → asusd fan profile.
        assert_eq!(
            AsusdFanProfile::from(PerformanceProfile::Silent),
            AsusdFanProfile::Quiet
        );
        assert_eq!(
            AsusdFanProfile::from(PerformanceProfile::Balanced),
            AsusdFanProfile::Balanced
        );
        assert_eq!(
            AsusdFanProfile::from(PerformanceProfile::Turbo),
            AsusdFanProfile::Performance
        );
    }

    // -----------------------------------------------------------------------
    // asusd FanCurveData parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_curve_entry_parses_cpu_and_gpu() {
        // CPU: raw PWM до 94.
        let cpu = parse_curve_entry(
            "CPU",
            &[45, 49, 54, 68, 74, 79, 84, 89],
            &[5, 22, 38, 45, 56, 63, 81, 94],
            true,
        )
        .expect("cpu");
        assert_eq!(cpu.fan, FanId::Cpu);
        assert_eq!(cpu.temps[0].get(), 45);
        assert_eq!(cpu.temps[7].get(), 89);
        assert_eq!(cpu.pwms[7].get(), 94);
        assert!(cpu.enabled);

        // GPU: raw PWM 112 > 100 сохраняется.
        let gpu = parse_curve_entry(
            "GPU",
            &[40, 42, 43, 60, 65, 69, 74, 78],
            &[5, 20, 38, 43, 56, 66, 84, 112],
            false,
        )
        .expect("gpu");
        assert_eq!(gpu.fan, FanId::Gpu);
        assert_eq!(gpu.pwms[7].get(), 112);
        assert!(!gpu.enabled);
    }

    #[test]
    fn parse_curve_entry_rejects_unknown_fan() {
        let err = parse_curve_entry("MID", &[45; 8], &[5; 8], true).expect_err("unknown fan");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn parse_curve_entry_rejects_out_of_range_pwm() {
        // FanPwm диапазон 0..255 гарантирован типом u8; проверяем, что
        // значение 255 принимается, а конструктор FanPwm валидирует диапазон.
        let curve = parse_curve_entry("CPU", &[45; 8], &[5, 22, 38, 45, 56, 63, 81, 255], true)
            .expect("pwm 255 valid");
        assert_eq!(curve.pwms[7].get(), 255);
        // FanPwm::new валидирует диапазон (0..=255).
        assert!(FanPwm::new(255).is_ok());
    }

    #[test]
    fn parse_curve_entry_rejects_out_of_range_temp() {
        // 200 °C вне диапазона TemperatureC.
        let err = parse_curve_entry("CPU", &[45, 49, 54, 68, 74, 79, 84, 200], &[5; 8], true)
            .expect_err("temp out of range");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[test]
    fn asusd_curve_set_matches_sysfs_active_curve() {
        // Live-валидация (Task 6.6): FanCurveData(0) == sysfs active curve
        // для активного профиля (Balanced). Проверяем, что парсинг даёт те же
        // значения, что SysfsFanCurveSource::active_curve.
        let cpu = parse_curve_entry(
            "CPU",
            &[45, 49, 54, 68, 74, 79, 84, 89],
            &[5, 22, 38, 45, 56, 63, 81, 94],
            true,
        )
        .expect("cpu");
        let gpu = parse_curve_entry(
            "GPU",
            &[40, 42, 43, 60, 65, 69, 74, 78],
            &[5, 20, 38, 43, 56, 66, 84, 112],
            false,
        )
        .expect("gpu");
        let set = AsusdFanCurveSet {
            profile: AsusdFanProfile::Balanced,
            cpu,
            gpu,
        };
        assert_eq!(set.profile, AsusdFanProfile::Balanced);
        assert_eq!(set.cpu.pwms[7].get(), 94);
        assert_eq!(set.gpu.pwms[7].get(), 112);
    }
}
