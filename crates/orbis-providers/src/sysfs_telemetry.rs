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
//! - malformed/пустые значения → `ProviderError::Internal`;
//! - I/O ошибки (кроме `NotFound`) → `ProviderError::Io`;
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
use orbis_core::telemetry::{BatteryTelemetry, FanTelemetry, PowerTelemetry, Telemetry};

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

        let mut cpu_temp = None;
        let mut gpu_temp = None;
        let mut gpu_power = None;
        let mut fans = Vec::new();

        for dir in read_dir_optional(&hwmon_dir)? {
            let name = read_string(&dir.join("name"))?;
            match name.as_deref() {
                Some("k10temp") => {
                    cpu_temp = read_temp_c(&dir.join("temp1_input"))?;
                }
                Some("amdgpu") => {
                    gpu_temp = read_amdgpu_edge_temp(&dir)?;
                    gpu_power = read_milli_watt(&dir.join("power1_input"))?;
                }
                Some("asus") => {
                    fans = read_asus_fans(&dir)?;
                }
                _ => {}
            }
        }

        let mut battery = None;
        let mut ac_online = None;
        for dir in read_dir_optional(&power_dir)? {
            let supply_type = read_string(&dir.join("type"))?;
            match supply_type.as_deref() {
                Some("Battery") => {
                    if battery.is_none() {
                        battery = read_battery(&dir)?;
                    }
                }
                // External supplies use several kernel type names (Mains,
                // USB*, Wireless, ...). Require an explicit non-Battery type
                // plus the standard `online` attribute instead of selecting
                // the first arbitrary power_supply that happens to have it.
                Some(_) if ac_online.is_none() && dir.join("online").exists() => {
                    ac_online = read_online(&dir)?;
                }
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
    let celsius = i16::try_from(raw / 1000).map_err(|_| narrowing_error("температура", raw, path))?;
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
    let milliwatt = u32::try_from(raw / 1000).map_err(|_| narrowing_error("мощность", raw, path))?;
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

/// Прочитать CPU/GPU (и другие) вентиляторы ASUS hwmon по labels.
///
/// `FanTelemetry.percent` всегда `None` (нет доказанного источника max RPM).
fn read_asus_fans(dir: &Path) -> Result<Vec<FanTelemetry>, ProviderError> {
    let mut fans = Vec::new();
    for i in 1..=4 {
        let label = read_string(&dir.join(format!("fan{i}_label")))?;
        let Some(rpm) = read_rpm(&dir.join(format!("fan{i}_input")))? else {
            continue;
        };
        let fan = match label.as_deref() {
            Some("cpu_fan") => FanId::Cpu,
            Some("gpu_fan") => FanId::Gpu,
            Some("mid_fan") => FanId::Mid,
            Some("system_fan") => FanId::System,
            Some(other) => FanId::Other(other.to_string()),
            // Без label не знаем, какой это вентилятор — пропускаем.
            None => continue,
        };
        fans.push(FanTelemetry {
            fan,
            rpm,
            percent: None,
        });
    }
    Ok(fans)
}

/// Прочитать battery telemetry из power_supply директории.
///
/// `energy_now`/`energy_full` не вычисляются (на многих машинах отсутствуют);
/// health вычисляется из `charge_full`/`charge_full_design` (доказанное
/// стандартное отношение), при `design == 0` → `None`.
fn read_battery(dir: &Path) -> Result<Option<BatteryTelemetry>, ProviderError> {
    let Some(percent) = read_percent(&dir.join("capacity"))? else {
        return Ok(None);
    };
    let status = read_string(&dir.join("status"))?.unwrap_or_default();
    let charge_cycles = match read_u64(&dir.join("cycle_count"))? {
        Some(raw) => Some(
            u32::try_from(raw)
                .map_err(|_| narrowing_error("cycle_count", raw, &dir.join("cycle_count")))?,
        ),
        None => None,
    };
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
            let clamped = u8::try_from(pct.min(100)).map_err(|_| {
                narrowing_error("battery health", pct, &dir.join("charge_full"))
            })?;
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
        write_fixture(root, &format!("class/power_supply/{name}/type"), "Battery\n");
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

        assert_eq!(t.fans.len(), 2);
        let cpu = t
            .fans
            .iter()
            .find(|f| f.fan == FanId::Cpu)
            .expect("cpu fan");
        assert_eq!(cpu.rpm, Rpm::new(2600).expect("rpm"));
        assert_eq!(cpu.percent, None);
        let gpu = t
            .fans
            .iter()
            .find(|f| f.fan == FanId::Gpu)
            .expect("gpu fan");
        assert_eq!(gpu.rpm, Rpm::new(2100).expect("rpm"));
        assert_eq!(gpu.percent, None);

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

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malformed_value_is_internal_error() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "not-a-number\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let err = provider.snapshot().await.expect_err("malformed must fail");
        assert!(matches!(err, ProviderError::Internal(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn empty_file_is_internal_error() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let err = provider.snapshot().await.expect_err("empty must fail");
        assert!(matches!(err, ProviderError::Internal(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn out_of_range_values_are_internal_error() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon0/name", "k10temp\n");
        // 999999 м°C = 999 °C — вне диапазона TemperatureC.
        write_fixture(&root, "class/hwmon/hwmon0/temp1_input", "999999\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let err = provider
            .snapshot()
            .await
            .expect_err("out of range must fail");
        assert!(matches!(err, ProviderError::Internal(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn narrowing_never_wraps_large_sysfs_values() {
        let root = fixture_root();
        write_fixture(&root, "class/hwmon/hwmon2/name", "asus\n");
        write_fixture(&root, "class/hwmon/hwmon2/fan1_label", "cpu_fan\n");
        write_fixture(&root, "class/hwmon/hwmon2/fan1_input", "65536\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let err = provider.snapshot().await.expect_err("RPM narrowing must fail");
        assert!(matches!(err, ProviderError::Internal(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn power_supply_discovery_requires_explicit_type() {
        let root = fixture_root();
        // Decoys deliberately expose familiar attributes under the wrong type.
        external_type(&root, "NOT_A_BATTERY");
        write_fixture(
            &root,
            "class/power_supply/NOT_A_BATTERY/capacity",
            "1\n",
        );
        battery_type(&root, "BAT_WITH_ONLINE");
        write_fixture(&root, "class/power_supply/BAT_WITH_ONLINE/online", "0\n");

        battery_type(&root, "BAT1");
        write_fixture(&root, "class/power_supply/BAT1/capacity", "77\n");
        external_type(&root, "ACAD");
        write_fixture(&root, "class/power_supply/ACAD/online", "1\n");

        let provider = SysfsTelemetryProvider::new(root.clone());
        let t = provider.snapshot().await.expect("snapshot");
        assert_eq!(t.battery.expect("battery").percent, Percent::new(77).unwrap());
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
}
