//! Read-only sysfs telemetry provider.
//!
//! Динамически обнаруживает устройства в `/sys/class/hwmon` и
//! `/sys/class/power_supply`; корень sysfs передаётся извне (в production —
//! `/sys`, в тестах — временное fixture-дерево). Никаких hardcoded
//! `hwmonN`/`BAT1`/`ACAD` путей.
//!
//! - только read-only чтение; никаких writes, fan control, power-limit writes;
//! - каждый вызов `snapshot()` выполняет новый authoritative read (кэш
//!   отсутствует);
//! - отсутствующие необязательные файлы → `None` (не ломают snapshot);
//! - неудачное чтение обнаруженного источника → `None` для этой группы плюс
//!   запись в `Telemetry.field_gaps` (Denied/Malformed/Unavailable), чтобы
//!   отказ/повреждение источника не был неотличим от структурного отсутствия
//!   (#117); остальные группы не затрагиваются;
//! - ошибки candidate metadata (`hwmon/name`, `power_supply/type`) пропускают
//!   только этот discovery entry; ошибки после выбора источника не скрываются;
//! - malformed/пустые значения выбранного источника → `ProviderError::Internal`;
//! - I/O ошибки выбранного источника (кроме `NotFound`) → `ProviderError::Io`;
//! - не вычисляются недоказанные значения: `energy_now/full`, `total` power,
//!   battery power; `FanTelemetry.percent` всегда `None`;
//! - telemetry НЕ смешивается с `GpuPower/GpuMux/GpuAccess` capability state:
//!   `Telemetry.gpu_power_state` остаётся `Unknown`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::fan::FanId;
use orbis_core::gpu::GpuPowerState;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::{MilliWatt, Percent, Rpm, TemperatureC};
use orbis_core::telemetry::{
    BatteryTelemetry, FanTelemetry, PowerTelemetry, Telemetry, TelemetryField, TelemetryFieldGap,
};

use crate::error::ProviderError;
use crate::traits::{Provider, ProviderHealth, TelemetryProvider};

/// Read-only sysfs telemetry provider.
///
/// Хранит корень sysfs; I/O начинается только в `snapshot().await`.
/// Конструктор не выполняет I/O, не проверяет существование файлов и не
/// открывает их на запись.
pub struct SysfsTelemetryProvider {
    sysfs_root: PathBuf,
}

impl SysfsTelemetryProvider {
    /// Создать provider над заданным корнем sysfs.
    ///
    /// В production передаётся `/sys`; в тестах — временное fixture-дерево.
    pub fn new(sysfs_root: PathBuf) -> Self {
        Self { sysfs_root }
    }
}

impl Default for SysfsTelemetryProvider {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys"))
    }
}

impl Provider for SysfsTelemetryProvider {
    fn id(&self) -> &'static str {
        "sysfs-telemetry"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("sysfs-telemetry")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("sysfs telemetry: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.sysfs-telemetry",
            "read-only sysfs hwmon/power_supply telemetry backend",
        )]
    }
}

#[async_trait]
impl TelemetryProvider for SysfsTelemetryProvider {
    async fn snapshot(&self) -> Result<Telemetry, ProviderError> {
        let hwmon_dir = self.sysfs_root.join("class").join("hwmon");
        let power_dir = self.sysfs_root.join("class").join("power_supply");

        // hwmon discovery failure is snapshot-global: we cannot proceed
        // without knowing which hwmon entries exist. But individual sensor
        // read failures are field-local — they set the metric to None
        // without destroying independent metrics.
        let hwmon_dirs = read_dir_optional(&hwmon_dir)?;

        let mut cpu_temp = None;
        let mut gpu_temp = None;
        let mut gpu_power = None;
        let mut fans = Vec::new();
        let mut field_gaps = Vec::new();

        for dir in &hwmon_dirs {
            let Some(name) = read_discovery_string(&dir.join("name")) else {
                continue;
            };
            match name.as_str() {
                "k10temp" => match read_temp_c(&dir.join("temp1_input")) {
                    Ok(value) => cpu_temp = value,
                    Err(error) => record_gap(&mut field_gaps, TelemetryField::CpuTemp, &error),
                },
                "amdgpu" => {
                    match read_amdgpu_edge_temp(dir) {
                        Ok(value) => gpu_temp = value,
                        Err(error) => record_gap(&mut field_gaps, TelemetryField::GpuTemp, &error),
                    }
                    match read_milli_watt(&dir.join("power1_input")) {
                        Ok(value) => gpu_power = value,
                        Err(error) => record_gap(&mut field_gaps, TelemetryField::GpuPower, &error),
                    }
                }
                "asus" => {
                    let (read_fans, first_failure) = read_asus_fans(dir);
                    fans = read_fans;
                    // A fully failing fan set is recorded as one aggregate gap;
                    // individual skipped fans stay optional-sensor behavior.
                    if fans.is_empty() {
                        if let Some(error) = first_failure {
                            record_gap(&mut field_gaps, TelemetryField::Fans, &error);
                        }
                    }
                }
                _ => {}
            }
        }

        // power_supply discovery failure is also snapshot-global.
        let power_dirs = read_dir_optional(&power_dir)?;

        let mut battery = None;
        let mut ac_online = None;
        for dir in &power_dirs {
            let Some(supply_type) = read_discovery_string(&dir.join("type")) else {
                continue;
            };
            match supply_type.as_str() {
                "Battery" => {
                    if battery.is_none() {
                        match read_battery(dir) {
                            Ok(value) => battery = value,
                            Err(error) => {
                                record_gap(&mut field_gaps, TelemetryField::Battery, &error)
                            }
                        }
                    }
                }
                // External supplies use several kernel type names (Mains,
                // USB*, Wireless, ...). Require an explicit non-Battery type
                // plus the standard `online` attribute instead of selecting
                // the first arbitrary power_supply that happens to have it.
                _ if ac_online.is_none() && dir.join("online").exists() => match read_online(dir) {
                    Ok(value) => ac_online = value,
                    Err(error) => record_gap(&mut field_gaps, TelemetryField::AcOnline, &error),
                },
                _ => {}
            }
        }

        Ok(Telemetry {
            cpu_temp,
            gpu_temp,
            fans,
            power: PowerTelemetry {
                ac: None,
                battery: None,
                total: None,
                gpu: gpu_power,
            },
            ac_online,
            battery,
            // Telemetry не смешивается с GpuPower/GpuMux/GpuAccess capability
            // state: capability живёт в отдельном GpuPowerProvider.
            gpu_power_state: GpuPowerState::Unknown,
            field_gaps,
            ts: SystemTime::now(),
        })
    }

    fn default_poll_interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}

/// Прочитать директорию; отсутствие директории → пустой список (не ошибка).
fn read_dir_optional(dir: &Path) -> Result<Vec<PathBuf>, ProviderError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ProviderError::Io(e)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(ProviderError::Io)?;
        paths.push(entry.path());
    }
    Ok(paths)
}

/// Прочитать один файл как строку.
///
/// Отсутствие файла → `None`; пустое содержимое → `Internal`; прочие I/O
/// ошибки → `Io`.
fn read_string(path: &Path) -> Result<Option<String>, ProviderError> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ProviderError::Io(e)),
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(ProviderError::Internal(format!(
            "sysfs telemetry: пустой файл '{}'",
            path.display()
        )));
    }
    Ok(Some(trimmed.to_string()))
}

/// Прочитать metadata, используемый только для классификации discovery entry.
/// Любая ошибка здесь означает «этот кандидат нельзя надёжно классифицировать»;
/// она не должна ломать уже доступную telemetry из независимых источников.
fn read_discovery_string(path: &Path) -> Option<String> {
    read_string(path).ok().flatten()
}

/// Прочитать один файл как `u64`.
fn read_u64(path: &Path) -> Result<Option<u64>, ProviderError> {
    let Some(trimmed) = read_string(path)? else {
        return Ok(None);
    };
    trimmed.parse::<u64>().map(Some).map_err(|_| {
        ProviderError::Internal(format!(
            "sysfs telemetry: невалидное значение '{trimmed}' в '{}'",
            path.display()
        ))
    })
}

fn narrowing_error(kind: &str, raw: u64, path: &Path) -> ProviderError {
    ProviderError::Internal(format!(
        "sysfs telemetry: {kind} вне представимого диапазона '{raw}' в '{}'",
        path.display()
    ))
}

/// Прочитать температуру: sysfs м°C → domain °C.
fn read_temp_c(path: &Path) -> Result<Option<TemperatureC>, ProviderError> {
    let Some(raw) = read_u64(path)? else {
        return Ok(None);
    };
    let celsius =
        i16::try_from(raw / 1000).map_err(|_| narrowing_error("температура", raw, path))?;
    TemperatureC::new(celsius).map(Some).map_err(|_| {
        ProviderError::Internal(format!(
            "sysfs telemetry: температура вне domain диапазона '{raw}' в '{}'",
            path.display()
        ))
    })
}

/// Прочитать RPM вентилятора.
fn read_rpm(path: &Path) -> Result<Option<Rpm>, ProviderError> {
    let Some(raw) = read_u64(path)? else {
        return Ok(None);
    };
    let rpm = u16::try_from(raw).map_err(|_| narrowing_error("RPM", raw, path))?;
    Rpm::new(rpm).map(Some).map_err(|_| {
        ProviderError::Internal(format!(
            "sysfs telemetry: RPM вне domain диапазона '{raw}' в '{}'",
            path.display()
        ))
    })
}

/// Прочитать мощность: sysfs мкВт → domain мВт.
fn read_milli_watt(path: &Path) -> Result<Option<MilliWatt>, ProviderError> {
    let Some(raw) = read_u64(path)? else {
        return Ok(None);
    };
    let milliwatt =
        u32::try_from(raw / 1000).map_err(|_| narrowing_error("мощность", raw, path))?;
    MilliWatt::new(milliwatt).map(Some).map_err(|_| {
        ProviderError::Internal(format!(
            "sysfs telemetry: мощность вне domain диапазона '{raw}' в '{}'",
            path.display()
        ))
    })
}

/// Прочитать процент.
fn read_percent(path: &Path) -> Result<Option<Percent>, ProviderError> {
    let Some(raw) = read_u64(path)? else {
        return Ok(None);
    };
    let percent = u8::try_from(raw).map_err(|_| narrowing_error("процент", raw, path))?;
    Percent::new(percent).map(Some).map_err(|_| {
        ProviderError::Internal(format!(
            "sysfs telemetry: процент вне domain диапазона '{raw}' в '{}'",
            path.display()
        ))
    })
}

/// Прочитать температуру dGPU: ищем `temp*_label == "edge"`.
///
/// Если label отсутствует — fallback на `temp1_input`.
fn read_amdgpu_edge_temp(dir: &Path) -> Result<Option<TemperatureC>, ProviderError> {
    for i in 1..=10 {
        let label = read_string(&dir.join(format!("temp{i}_label")))?;
        if label.as_deref() == Some("edge") {
            return read_temp_c(&dir.join(format!("temp{i}_input")));
        }
    }
    read_temp_c(&dir.join("temp1_input"))
}

/// Read a discovered source failure into its typed field gap class (#117).
///
/// Permission failures stay `Denied`; other I/O failures are transient
/// `Unavailable`; parse/empty/range failures are `Malformed`. The first
/// recorded class per group wins.
fn record_gap(
    gaps: &mut Vec<(TelemetryField, TelemetryFieldGap)>,
    field: TelemetryField,
    error: &ProviderError,
) {
    let gap = match error {
        ProviderError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            TelemetryFieldGap::Denied
        }
        ProviderError::Io(_) => TelemetryFieldGap::Unavailable,
        _ => TelemetryFieldGap::Malformed,
    };
    if !gaps.iter().any(|(candidate, _)| *candidate == field) {
        gaps.push((field, gap));
    }
}

/// Прочитать CPU/GPU (и другие) вентиляторы ASUS hwmon по labels.
///
/// `FanTelemetry.percent` всегда `None` (нет доказанного источника max RPM).
///
/// Каждый fan обрабатывается независимо: ошибка одного fan (malformed
/// label/RPM) пропускает только этот entry, сохраняя остальные. Первая
/// обнаруженная ошибка чтения возвращается вызывающему как field-local
/// evidence; полностью пустой результат с ошибкой означает, что ни один
/// вентилятор не был прочитан.
fn read_asus_fans(dir: &Path) -> (Vec<FanTelemetry>, Option<ProviderError>) {
    let mut fans = Vec::new();
    let mut first_failure = None;
    for i in 1..=4 {
        // Skip fans with missing or malformed labels (read_string returns
        // None for NotFound, Err for malformed — both are skip-safe).
        let Some(label) = read_string(&dir.join(format!("fan{i}_label")))
            .ok()
            .flatten()
        else {
            continue;
        };
        let fan = match label.as_str() {
            "cpu_fan" => FanId::Cpu,
            "gpu_fan" => FanId::Gpu,
            "mid_fan" => FanId::Mid,
            "system_fan" => FanId::System,
            other => FanId::Other(other.to_string()),
        };
        // RPM read failure skips this fan without killing others.
        let rpm = match read_rpm(&dir.join(format!("fan{i}_input"))) {
            Ok(value) => value,
            Err(error) => {
                if first_failure.is_none() {
                    first_failure = Some(error);
                }
                continue;
            }
        };
        let Some(rpm) = rpm else {
            continue;
        };
        fans.push(FanTelemetry {
            source: "asus-hwmon".into(),
            fan,
            label,
            rpm,
            percent: None,
            quality: orbis_core::telemetry::FanTelemetryQuality::Complete,
        });
    }
    (fans, first_failure)
}

/// Прочитать battery telemetry из power_supply директории.
///
/// `energy_now`/`energy_full` не вычисляются (на многих машинах отсутствуют);
/// health вычисляется из `charge_full`/`charge_full_design` (доказанное
/// стандартное отношение), при `design == 0` → `None`.
///
/// Optional metadata (health, cycle_count) that fails to parse degrades to
/// `None` without killing the entire battery metric — percent and state are
/// the required fields.
fn read_battery(dir: &Path) -> Result<Option<BatteryTelemetry>, ProviderError> {
    let Some(percent) = read_percent(&dir.join("capacity"))? else {
        return Ok(None);
    };
    let status = read_string(&dir.join("status"))?.unwrap_or_default();
    // cycle_count is optional metadata: malformed file → None, not error.
    let charge_cycles = read_u64(&dir.join("cycle_count"))
        .ok()
        .flatten()
        .and_then(|raw| u32::try_from(raw).ok());
    let charge_full = read_u64(&dir.join("charge_full"))?;
    let charge_full_design = read_u64(&dir.join("charge_full_design"))?;
    let capacity = match (charge_full, charge_full_design) {
        (Some(full), Some(design)) if design > 0 => {
            let scaled = full.checked_mul(100).ok_or_else(|| {
                ProviderError::Internal(format!(
                    "sysfs telemetry: battery health overflow для '{}'",
                    dir.join("charge_full").display()
                ))
            })?;
            let pct = scaled / design;
            let clamped = u8::try_from(pct.min(100))
                .map_err(|_| narrowing_error("battery health", pct, &dir.join("charge_full")))?;
            Some(Percent::new(clamped).map_err(|_| {
                ProviderError::Internal(format!(
                    "sysfs telemetry: battery health вне domain диапазона '{pct}'"
                ))
            })?)
        }
        _ => None,
    };
    Ok(Some(BatteryTelemetry {
        percent,
        capacity,
        energy_now: None,
        energy_full: None,
        charge_cycles,
        state: status,
    }))
}

/// Прочитать AC `online` (0/1) из power_supply директории.
fn read_online(dir: &Path) -> Result<Option<bool>, ProviderError> {
    let Some(raw) = read_u64(&dir.join("online"))? else {
        return Ok(None);
    };
    match raw {
        0 => Ok(Some(false)),
        1 => Ok(Some(true)),
        other => Err(ProviderError::Internal(format!(
            "sysfs telemetry: невалидное AC online значение '{other}' в '{}'",
            dir.join("online").display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use orbis_core::telemetry::FanTelemetryQuality;

    use super::*;

    /// Создать уникальный временный fixture-корень.
    fn fixture_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbis-sysfs-telemetry-test-{}-{}",
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

    /// Записать fixture-файл (создавая родительские директории).
    fn write_fixture(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write fixture");
    }

    fn battery_type(root: &Path, name: &str) {
        write_fixture(
            root,
            &format!("class/power_supply/{name}/type"),
            "Battery\n",
        );
    }

    fn external_type(root: &Path, name: &str) {
        write_fixture(root, &format!("class/power_supply/{name}/type"), "Mains\n");
    }

    /// Полное fixture-дерево: k10temp, amdgpu, asus, BAT1, ACAD.
    fn full_fixture(root: &Path) {
        write_fixture(root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(root, "class/hwmon/hwmon0/temp1_input", "46375\n");
        write_fixture(root, "class/hwmon/hwmon0/temp1_label", "Tctl\n");

        write_fixture(root, "class/hwmon/hwmon1/name", "amdgpu\n");
        write_fixture(root, "class/hwmon/hwmon1/temp1_input", "43000\n");
        write_fixture(root, "class/hwmon/hwmon1/temp1_label", "edge\n");
        write_fixture(root, "class/hwmon/hwmon1/power1_input", "13073000\n");

        write_fixture(root, "class/hwmon/hwmon2/name", "asus\n");
        write_fixture(root, "class/hwmon/hwmon2/fan1_input", "2600\n");
        write_fixture(root, "class/hwmon/hwmon2/fan1_label", "cpu_fan\n");
        write_fixture(root, "class/hwmon/hwmon2/fan2_input", "2100\n");
        write_fixture(root, "class/hwmon/hwmon2/fan2_label", "gpu_fan\n");

        battery_type(root, "BAT1");
        write_fixture(root, "class/power_supply/BAT1/capacity", "100\n");
        write_fixture(root, "class/power_supply/BAT1/status", "Full\n");
        write_fixture(root, "class/power_supply/BAT1/cycle_count", "0\n");
        write_fixture(root, "class/power_supply/BAT1/charge_full", "4962000\n");
        write_fixture(
            root,
            "class/power_supply/BAT1/charge_full_design",
            "5675000\n",
        );

        external_type(root, "ACAD");
        write_fixture(root, "class/power_supply/ACAD/online", "1\n");
    }

    #[tokio::test]
    async fn full_fixture_reads_all_fields() {
        let root = fixture_root();
        full_fixture(&root);
        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.cpu_temp, Some(TemperatureC::new(46).expect("c")));
        assert_eq!(t.gpu_temp, Some(TemperatureC::new(43).expect("c")));
        assert_eq!(t.power.gpu, Some(MilliWatt::new(13_073).expect("mw")));
        assert_eq!(t.ac_online, Some(true));
        // Every discovered source read successfully: no gap evidence.
        assert!(t.field_gaps.is_empty());

        assert_eq!(t.fans.len(), 2);
        let cpu = t
            .fans
            .iter()
            .find(|f| f.fan == FanId::Cpu)
            .expect("cpu fan");
        assert_eq!(cpu.rpm, Rpm::new(2600).expect("rpm"));
        assert_eq!(cpu.percent, None);
        assert_eq!(cpu.label, "cpu_fan");
        assert_eq!(cpu.source, "asus-hwmon");
        assert_eq!(cpu.quality, FanTelemetryQuality::Complete);
        let gpu = t
            .fans
            .iter()
            .find(|f| f.fan == FanId::Gpu)
            .expect("gpu fan");
        assert_eq!(gpu.rpm, Rpm::new(2100).expect("rpm"));
        assert_eq!(gpu.percent, None);
        assert_eq!(gpu.label, "gpu_fan");
        assert_eq!(gpu.source, "asus-hwmon");
        assert_eq!(gpu.quality, FanTelemetryQuality::Complete);

        let battery = t.battery.expect("battery");
        assert_eq!(battery.percent, Percent::new(100).expect("pct"));
        assert_eq!(battery.state, "Full");
        assert_eq!(battery.charge_cycles, Some(0));
        // health = 4962000 / 5675000 = 87.4% → 87
        assert_eq!(battery.capacity, Some(Percent::new(87).expect("pct")));
        assert_eq!(battery.energy_now, None);
        assert_eq!(battery.energy_full, None);

        // Не вычисляются недоказанные значения.
        assert_eq!(t.power.ac, None);
        assert_eq!(t.power.battery, None);
        assert_eq!(t.power.total, None);

        // Telemetry не смешивается с GPU capability state.
        assert_eq!(t.gpu_power_state, GpuPowerState::Unknown);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn discovery_works_with_different_hwmon_indices() {
        let root = fixture_root();
        // Другие индексы hwmon (не hardcoded hwmon0/1/2).
        write_fixture(&root, "class/hwmon/hwmon7/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon7/temp1_input", "50000\n");
        write_fixture(&root, "class/hwmon/hwmon3/name", "amdgpu\n");
        write_fixture(&root, "class/hwmon/hwmon3/temp1_input", "41000\n");
        write_fixture(&root, "class/hwmon/hwmon3/temp1_label", "edge\n");
        write_fixture(&root, "class/hwmon/hwmon9/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon9/fan1_input", "3000\n");
        write_fixture(&root, "class/hwmon/hwmon9/fan1_label", "cpu_fan\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");

        assert_eq!(t.cpu_temp, Some(TemperatureC::new(50).expect("c")));
        assert_eq!(t.gpu_temp, Some(TemperatureC::new(41).expect("c")));
        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].rpm, Rpm::new(3000).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_files_yield_none_without_breaking_snapshot() {
        let root = fixture_root();
        // Только k10temp + BAT1 без необязательных файлов.
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "46375\n");
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "80\n");
        // status/cycle_count/charge_full/charge_full_design отсутствуют.
        // amdgpu/asus/ACAD отсутствуют.

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");

        assert_eq!(t.cpu_temp, Some(TemperatureC::new(46).expect("c")));
        assert_eq!(t.gpu_temp, None);
        assert_eq!(t.power.gpu, None);
        assert!(t.fans.is_empty());
        assert_eq!(t.ac_online, None);

        let battery = t.battery.expect("battery");
        assert_eq!(battery.percent, Percent::new(80).expect("pct"));
        assert_eq!(battery.state, "");
        assert_eq!(battery.charge_cycles, None);
        assert_eq!(battery.capacity, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn empty_sysfs_root_yields_empty_snapshot() {
        let root = fixture_root();
        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");

        assert_eq!(t.cpu_temp, None);
        assert_eq!(t.gpu_temp, None);
        assert!(t.fans.is_empty());
        assert_eq!(t.battery, None);
        assert_eq!(t.ac_online, None);
        // Structural absence everywhere: no failed reads, no gap evidence.
        assert!(t.field_gaps.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_battery_capacity_kills_group_with_gap_evidence() {
        // #117: a malformed required battery field previously made the whole
        // battery look structurally absent; now the group stays None but the
        // Malformed gap proves a battery source was discovered and failed.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "46375\n");
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "garbage\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.battery, None);
        assert_eq!(
            t.gap(TelemetryField::Battery),
            Some(TelemetryFieldGap::Malformed)
        );
        // Independent metrics stay intact and gap-free.
        assert_eq!(t.cpu_temp, Some(TemperatureC::new(46).expect("c")));
        assert_eq!(t.gap(TelemetryField::CpuTemp), None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn all_discovered_sources_failing_yields_ok_empty_snapshot() {
        // #117: discovery finds every source, but every selected value read
        // fails. The snapshot stays Ok with no useful data (quality Empty) —
        // never fake values and never a provider error — so consumers can
        // distinguish "no observations" from a successful collection.
        let root = fixture_root();
        full_fixture(&root);
        for rel in [
            "class/hwmon/hwmon0/temp1_input",
            "class/hwmon/hwmon1/temp1_input",
            "class/hwmon/hwmon1/power1_input",
            "class/hwmon/hwmon2/fan1_input",
            "class/hwmon/hwmon2/fan2_input",
            "class/power_supply/BAT1/capacity",
            "class/power_supply/ACAD/online",
        ] {
            write_fixture(&root, rel, "garbage\n");
        }

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot stays Ok");
        assert_eq!(t.quality(), orbis_core::telemetry::TelemetryQuality::Empty);
        assert_eq!(t.cpu_temp, None);
        assert_eq!(t.gpu_temp, None);
        assert_eq!(t.power.gpu, None);
        assert!(t.fans.is_empty());
        assert_eq!(t.battery, None);
        assert_eq!(t.ac_online, None);
        // #117: every discovered group that failed records field-local
        // evidence instead of silently collapsing into structural absence.
        for field in [
            TelemetryField::CpuTemp,
            TelemetryField::GpuTemp,
            TelemetryField::GpuPower,
            TelemetryField::Fans,
            TelemetryField::AcOnline,
            TelemetryField::Battery,
        ] {
            assert_eq!(
                t.gap(field),
                Some(TelemetryFieldGap::Malformed),
                "expected Malformed gap for {field:?}"
            );
        }
        assert_eq!(t.field_gaps.len(), 6);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn unreadable_source_degrades_field_locally() {
        // #117: a permission-denied selected source becomes field-local
        // absence (None); independent metrics stay intact. Skipped when the
        // process can bypass file permissions (root).
        use std::os::unix::fs::PermissionsExt;

        let root = fixture_root();
        full_fixture(&root);
        let temp = root.join("class/hwmon/hwmon0/temp1_input");
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        if std::fs::read_to_string(&temp).is_ok() {
            return; // running privileged; permission simulation not observable
        }

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot stays Ok");
        assert_eq!(t.cpu_temp, None);
        // The denied read is field-local evidence, distinct from absence.
        assert_eq!(
            t.gap(TelemetryField::CpuTemp),
            Some(TelemetryFieldGap::Denied)
        );
        // Independent fields survive without any gap evidence.
        assert_eq!(t.gap(TelemetryField::GpuTemp), None);
        assert_eq!(t.gap(TelemetryField::Battery), None);
        assert_eq!(t.gap(TelemetryField::AcOnline), None);
        assert_eq!(t.gpu_temp, Some(TemperatureC::new(43).expect("c")));
        assert_eq!(
            t.battery.expect("battery").percent,
            Percent::new(100).expect("pct")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn unreadable_source_class_is_unavailable_for_other_io_errors() {
        // A selected value file that cannot be read for a non-permission I/O
        // reason (here: it is a directory) records Unavailable, not Denied.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        std::fs::create_dir_all(root.join("class/hwmon/hwmon0/temp1_input")).unwrap();

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot stays Ok");
        assert_eq!(t.cpu_temp, None);
        assert_eq!(
            t.gap(TelemetryField::CpuTemp),
            Some(TelemetryFieldGap::Unavailable)
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_cpu_temp_degrades_to_none() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider
            .snapshot()
            .await
            .expect("snapshot must succeed despite malformed CPU temp");
        assert_eq!(t.cpu_temp, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn empty_cpu_temp_file_degrades_to_none() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider
            .snapshot()
            .await
            .expect("snapshot must succeed despite empty CPU temp file");
        assert_eq!(t.cpu_temp, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn out_of_range_cpu_temp_degrades_to_none() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        // 999999 м°C = 999 °C — вне диапазона TemperatureC.
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "999999\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider
            .snapshot()
            .await
            .expect("snapshot must succeed despite out-of-range CPU temp");
        assert_eq!(t.cpu_temp, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_fan_rpm_degrades_to_empty() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon2/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon2/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon2/fan1_input", "65536\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider
            .snapshot()
            .await
            .expect("snapshot must succeed despite malformed fan RPM");
        assert!(t.fans.is_empty());
        // All labeled fans failed to produce RPM: aggregate gap evidence.
        assert_eq!(
            t.gap(TelemetryField::Fans),
            Some(TelemetryFieldGap::Malformed)
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn discovery_skips_unclassifiable_entries() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/decoy/name", "\n");
        write_fixture(&root, "class/power_supply/decoy/type", "\n");

        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "47000\n");
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "77\n");
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "1\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");

        assert_eq!(t.cpu_temp, Some(TemperatureC::new(47).expect("c")));
        assert_eq!(
            t.battery.expect("battery").percent,
            Percent::new(77).expect("pct")
        );
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn power_supply_discovery_requires_explicit_type() {
        let root = fixture_root();
        // Decoys deliberately expose familiar attributes under the wrong type.
        external_type(&root, "NOT_A_BATTERY");
        write_fixture(&root, "class/power_supply/NOT_A_BATTERY/capacity", "1\n");
        battery_type(&root, "BAT_WITH_ONLINE");
        write_fixture(&root, "class/power_supply/BAT_WITH_ONLINE/online", "0\n");

        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "77\n");
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "1\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(
            t.battery.expect("battery").percent,
            Percent::new(77).unwrap()
        );
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn units_are_converted_correctly() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "46375\n"); // 46.375°C → 46
        write_fixture(&root, "class/hwmon/hwmon1/name", "amdgpu\n");
        write_fixture(&root, "class/hwmon/hwmon1/temp1_input", "43000\n"); // 43°C
        write_fixture(&root, "class/hwmon/hwmon1/temp1_label", "edge\n");
        write_fixture(&root, "class/hwmon/hwmon1/power1_input", "13073000\n"); // 13073 мВт
        write_fixture(&root, "class/hwmon/hwmon2/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon2/fan1_input", "2600\n"); // 2600 RPM
        write_fixture(&root, "class/hwmon/hwmon2/fan1_label", "cpu_fan\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");

        assert_eq!(t.cpu_temp, Some(TemperatureC::new(46).expect("c")));
        assert_eq!(t.gpu_temp, Some(TemperatureC::new(43).expect("c")));
        assert_eq!(t.power.gpu, Some(MilliWatt::new(13_073).expect("mw")));
        assert_eq!(t.fans[0].rpm, Rpm::new(2600).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn ac_online_parses_zero_and_one() {
        let root = fixture_root();
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "0\n");
        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.ac_online, Some(false));

        write_fixture(&root, "class/power_supply/ACAD/online", "1\n");
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn battery_health_clamps_to_100() {
        let root = fixture_root();
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "50\n");
        write_fixture(&root, "class/power_supply/BAT1/charge_full", "6000000\n");
        write_fixture(
            &root,
            "class/power_supply/BAT1/charge_full_design",
            "5000000\n",
        );

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        let battery = t.battery.expect("battery");
        assert_eq!(battery.capacity, Some(Percent::new(100).expect("pct")));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn battery_health_zero_design_is_none() {
        let root = fixture_root();
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "50\n");
        write_fixture(&root, "class/power_supply/BAT1/charge_full", "6000000\n");
        write_fixture(&root, "class/power_supply/BAT1/charge_full_design", "0\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        let battery = t.battery.expect("battery");
        assert_eq!(battery.capacity, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn no_mock_telemetry_values_are_used() {
        let root = fixture_root();
        // Fixture значения отличаются от mock fixture (72/65/28000).
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "46375\n");
        write_fixture(&root, "class/hwmon/hwmon1/name", "amdgpu\n");
        write_fixture(&root, "class/hwmon/hwmon1/temp1_input", "43000\n");
        write_fixture(&root, "class/hwmon/hwmon1/temp1_label", "edge\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");

        assert_ne!(t.cpu_temp, Some(TemperatureC::new(72).expect("mock")));
        assert_ne!(t.gpu_temp, Some(TemperatureC::new(65).expect("mock")));
        assert_eq!(t.power.ac, None); // mock 28000 мВт не используется

        let _ = std::fs::remove_dir_all(root);
    }

    // -----------------------------------------------------------------------
    // Partial-failure regression tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn malformed_cpu_temp_does_not_break_battery_ac_fans() {
        // Malformed CPU temp should degrade cpu_temp to None without destroying
        // independent metrics: battery, AC, fans, GPU.
        let root = fixture_root();
        full_fixture(&root);
        // Corrupt CPU temp with malformed value.
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        // CPU temp degraded to None.
        assert_eq!(t.cpu_temp, None);
        // All other metrics remain intact.
        assert_eq!(t.gpu_temp, Some(TemperatureC::new(43).expect("c")));
        assert_eq!(t.power.gpu, Some(MilliWatt::new(13_073).expect("mw")));
        assert_eq!(t.fans.len(), 2);
        assert!(t.battery.is_some());
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_gpu_power_does_not_break_cpu() {
        // Malformed GPU power1_input should degrade gpu_power to None without
        // destroying CPU temp or other metrics.
        let root = fixture_root();
        full_fixture(&root);
        // Corrupt GPU power with non-numeric value.
        write_fixture(&root, "class/hwmon/hwmon1/power1_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        // GPU power degraded to None.
        assert_eq!(t.power.gpu, None);
        // CPU temp, GPU temp, fans, battery, AC remain intact.
        assert_eq!(t.cpu_temp, Some(TemperatureC::new(46).expect("c")));
        assert_eq!(t.gpu_temp, Some(TemperatureC::new(43).expect("c")));
        assert_eq!(t.fans.len(), 2);
        assert!(t.battery.is_some());
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_battery_cycle_count_does_not_kill_percent() {
        // Malformed cycle_count should degrade charge_cycles to None without
        // destroying percent, health, or state.
        let root = fixture_root();
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "80\n");
        write_fixture(&root, "class/power_supply/BAT1/status", "Discharging\n");
        write_fixture(
            &root,
            "class/power_supply/BAT1/cycle_count",
            "not-a-number\n",
        );
        write_fixture(&root, "class/power_supply/BAT1/charge_full", "4962000\n");
        write_fixture(
            &root,
            "class/power_supply/BAT1/charge_full_design",
            "5675000\n",
        );

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");
        let battery = t.battery.expect("battery must exist");

        // Percent and state are intact.
        assert_eq!(battery.percent, Percent::new(80).expect("pct"));
        assert_eq!(battery.state, "Discharging");
        // cycle_count degraded to None.
        assert_eq!(battery.charge_cycles, None);
        // Health calculation still works.
        assert_eq!(battery.capacity, Some(Percent::new(87).expect("pct")));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn ac_online_zero_is_some_false() {
        // AC online=0 → Some(false) (device exists, offline).
        // Not None (unavailable) — the source exists.
        let root = fixture_root();
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "0\n");
        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.ac_online, Some(false));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_ac_is_none() {
        // No power_supply with online attribute → ac_online = None.
        let root = fixture_root();
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "80\n");
        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.ac_online, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fan_rpm_zero_is_some_not_none() {
        // RPM 0 is a valid physical value (fan stopped), distinct from "no data".
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "0\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].rpm, Rpm::new(0).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_fan_rpm_is_not_zero() {
        // Malformed RPM → fan excluded (empty fans list), not RPM=0.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        // Malformed fan excluded, not RPM=0.
        assert!(t.fans.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn disappearing_selected_source_does_not_kill_snapshot() {
        // Source found during discovery but file disappears before read.
        // read_temp_c returns Ok(None) for NotFound → cpu_temp = None.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        // temp1_input does NOT exist → read_temp_c returns Ok(None).
        // Battery and AC should still work.
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "85\n");
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "1\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");
        assert_eq!(t.cpu_temp, None);
        assert!(t.battery.is_some());
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn overflow_metric_does_not_wrap_truncate() {
        // Oversized values (e.g., RPM > u16::MAX) degrade to None via
        // narrowing_error, not wrap to 0 or truncate.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "amdgpu\n");
        write_fixture(&root, "class/hwmon/hwmon0/power1_input", "9999999999999\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");
        // GPU power degraded to None, not truncated.
        assert_eq!(t.power.gpu, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_unrelated_discovery_entry_still_skipped() {
        // A malformed entry (non-numeric name file) should be skipped
        // without affecting other valid entries.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "not-a-number\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "50000\n");
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "90\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");
        // k10temp not matched (name is "not-a-number"), so cpu_temp = None.
        assert_eq!(t.cpu_temp, None);
        // Battery still works.
        assert_eq!(
            t.battery.expect("battery").percent,
            Percent::new(90).expect("pct")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    // -----------------------------------------------------------------------
    // Per-fan partial-failure regression tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn malformed_fan0_label_preserves_fan1() {
        // fan0 has malformed label → should be skipped, fan1 preserved.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        // fan0: label is empty (malformed), input exists but irrelevant.
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "9999\n");
        // fan1: valid.
        write_fixture(&root, "class/hwmon/hwmon0/fan2_label", "gpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_input", "3200\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        // fan1 (gpu_fan) preserved with correct identity.
        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].fan, FanId::Gpu);
        assert_eq!(t.fans[0].rpm, Rpm::new(3200).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_fan1_preserves_fan0() {
        // fan0 valid, fan1 malformed → fan0 preserved.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        // fan0: valid.
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "2600\n");
        // fan1: malformed input.
        write_fixture(&root, "class/hwmon/hwmon0/fan2_label", "gpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].fan, FanId::Cpu);
        assert_eq!(t.fans[0].rpm, Rpm::new(2600).expect("rpm"));
        // Partial fan data exists, so the surviving observation carries the
        // evidence; no aggregate gap is recorded.
        assert!(t.field_gaps.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fan_rpm_zero_with_malformed_neighbor() {
        // fan0 = 0 RPM (valid), fan1 malformed → fan0 preserved as 0.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "0\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_label", "gpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].fan, FanId::Cpu);
        assert_eq!(t.fans[0].rpm, Rpm::new(0).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fan_overflow_preserves_valid_neighbor() {
        // fan0 overflow (> u16::MAX), fan1 valid → fan1 preserved.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "999999\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_label", "gpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_input", "3200\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        // fan0 overflow → skipped, fan1 preserved.
        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].fan, FanId::Gpu);
        assert_eq!(t.fans[0].rpm, Rpm::new(3200).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_fan_input_preserves_valid_neighbor() {
        // fan0 missing input, fan1 valid → fan1 preserved with identity.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        // fan0: label exists but input missing.
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        // fan1: both present.
        write_fixture(&root, "class/hwmon/hwmon0/fan2_label", "gpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_input", "3200\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        assert_eq!(t.fans.len(), 1);
        assert_eq!(t.fans[0].fan, FanId::Gpu);
        assert_eq!(t.fans[0].rpm, Rpm::new(3200).expect("rpm"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_fan_does_not_break_cpu_battery_ac() {
        // Malformed fan input should not destroy independent telemetry.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "46375\n");
        write_fixture(&root, "class/hwmon/hwmon1/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon1/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon1/fan1_input", "not-a-number\n");
        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "80\n");
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "1\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");

        assert_eq!(t.cpu_temp, Some(TemperatureC::new(46).expect("c")));
        assert!(t.fans.is_empty());
        assert!(t.battery.is_some());
        assert_eq!(t.ac_online, Some(true));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn all_fans_malformed_snapshot_succeeds() {
        // All fan inputs malformed → fans empty, snapshot succeeds.
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan1_input", "not-a-number\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_label", "gpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon0/fan2_input", "overflow\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot must succeed");
        assert!(t.fans.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }
}
