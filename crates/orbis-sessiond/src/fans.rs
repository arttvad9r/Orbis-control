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

impl<S> SysfsFanCurveProvider<S>
where
    S: FanCurveSource,
{
    /// Прочитать активную кривую вентилятора (read-only, без profile).
    pub async fn active_curve(&self, fan: &FanId) -> Result<FanCurve, ProviderError> {
        self.source.read_active_curve(fan).await
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
}
