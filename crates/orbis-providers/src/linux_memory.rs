//! Linux read-only RAM/swap/PSI/zram/zswap provider.
//!
//! Sources are fixed kernel interfaces chosen by the provider. No caller-
//! supplied path is accepted by the production constructor and no writes are
//! performed.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::system_telemetry::{
    MemoryPressureTelemetry, PsiPressureLine, SystemMemoryTelemetry, ZramTelemetry, ZswapTelemetry,
};

use crate::error::ProviderError;
use crate::traits::{Provider, ProviderHealth};

/// Read-only system memory telemetry capability.
#[async_trait]
pub trait SystemMemoryProvider: Provider {
    /// Read RAM/swap and optional memory PSI.
    async fn memory_snapshot(&self) -> Result<SystemMemoryTelemetry, ProviderError>;

    /// Read discovered zram devices.
    async fn zram_snapshot(&self) -> Result<Vec<ZramTelemetry>, ProviderError>;

    /// Read zswap enabled state.
    async fn zswap_snapshot(&self) -> Result<ZswapTelemetry, ProviderError>;
}

/// Fixed paths for Linux system memory telemetry.
#[derive(Debug, Clone)]
struct LinuxMemoryPaths {
    meminfo: PathBuf,
    pressure_memory: PathBuf,
    sys_block: PathBuf,
    zswap_enabled: PathBuf,
}

impl Default for LinuxMemoryPaths {
    fn default() -> Self {
        Self {
            meminfo: PathBuf::from("/proc/meminfo"),
            pressure_memory: PathBuf::from("/proc/pressure/memory"),
            sys_block: PathBuf::from("/sys/block"),
            zswap_enabled: PathBuf::from("/sys/module/zswap/parameters/enabled"),
        }
    }
}

/// Linux read-only memory telemetry provider.
#[derive(Debug, Clone, Default)]
pub struct LinuxSystemMemoryProvider {
    paths: LinuxMemoryPaths,
}

impl LinuxSystemMemoryProvider {
    /// Construct the production provider using canonical kernel paths.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for LinuxSystemMemoryProvider {
    fn id(&self) -> &'static str {
        "linux-system-memory"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("linux-proc-sys-memory")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("Linux memory telemetry source unavailable for '{feature}'")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.linux-system-memory",
            "read-only /proc and /sys memory telemetry",
        )]
    }
}

#[async_trait]
impl SystemMemoryProvider for LinuxSystemMemoryProvider {
    async fn memory_snapshot(&self) -> Result<SystemMemoryTelemetry, ProviderError> {
        let meminfo = fs::read_to_string(&self.paths.meminfo).map_err(ProviderError::Io)?;
        let mut snapshot = parse_meminfo(&meminfo)?;

        snapshot.pressure = match fs::read_to_string(&self.paths.pressure_memory) {
            Ok(raw) => Some(parse_memory_pressure(&raw)?),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(ProviderError::Io(error)),
        };

        Ok(snapshot)
    }

    async fn zram_snapshot(&self) -> Result<Vec<ZramTelemetry>, ProviderError> {
        read_zram_devices(&self.paths.sys_block)
    }

    async fn zswap_snapshot(&self) -> Result<ZswapTelemetry, ProviderError> {
        match fs::read_to_string(&self.paths.zswap_enabled) {
            Ok(raw) => Ok(ZswapTelemetry {
                enabled: parse_bool_parameter(&raw)?,
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(ZswapTelemetry { enabled: None })
            }
            Err(error) => Err(ProviderError::Io(error)),
        }
    }
}

/// Parse Linux `/proc/meminfo`.
pub fn parse_meminfo(raw: &str) -> Result<SystemMemoryTelemetry, ProviderError> {
    fn kib_value(line: &str) -> Result<u64, ProviderError> {
        let mut fields = line.split_whitespace();
        let value = fields
            .next()
            .ok_or_else(|| ProviderError::Internal("meminfo value missing".into()))?
            .parse::<u64>()
            .map_err(|error| {
                ProviderError::Internal(format!("invalid meminfo integer: {error}"))
            })?;
        if let Some(unit) = fields.next() {
            if unit != "kB" {
                return Err(ProviderError::Internal(format!(
                    "unexpected meminfo unit '{unit}'"
                )));
            }
        }
        Ok(value)
    }

    let mut total = None;
    let mut available = None;
    let mut swap_total = None;
    let mut swap_free = None;

    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "MemTotal" => total = Some(kib_value(value)?),
            "MemAvailable" => available = Some(kib_value(value)?),
            "SwapTotal" => swap_total = Some(kib_value(value)?),
            "SwapFree" => swap_free = Some(kib_value(value)?),
            _ => {}
        }
    }

    Ok(SystemMemoryTelemetry {
        total_kib: total
            .ok_or_else(|| ProviderError::Internal("MemTotal missing from meminfo".into()))?,
        available_kib: available,
        swap_total_kib: swap_total
            .ok_or_else(|| ProviderError::Internal("SwapTotal missing from meminfo".into()))?,
        swap_free_kib: swap_free
            .ok_or_else(|| ProviderError::Internal("SwapFree missing from meminfo".into()))?,
        pressure: None,
    })
}

fn parse_percent_basis_points(raw: &str) -> Result<u16, ProviderError> {
    let (whole, fractional) = raw.split_once('.').unwrap_or((raw, ""));
    let whole = whole
        .parse::<u16>()
        .map_err(|error| ProviderError::Internal(format!("invalid PSI percent: {error}")))?;
    if whole > 100 {
        return Err(ProviderError::Internal(format!(
            "PSI percentage out of range: {raw}"
        )));
    }

    if fractional.len() > 2 || !fractional.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProviderError::Internal(format!(
            "invalid PSI fractional percentage: {raw}"
        )));
    }
    let fraction = match fractional.len() {
        0 => 0,
        1 => {
            fractional
                .parse::<u16>()
                .map_err(|error| ProviderError::Internal(error.to_string()))?
                * 10
        }
        2 => fractional
            .parse::<u16>()
            .map_err(|error| ProviderError::Internal(error.to_string()))?,
        _ => unreachable!(),
    };
    let basis_points = whole
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(|| ProviderError::Internal("PSI percentage overflow".into()))?;
    if basis_points > 10_000 {
        return Err(ProviderError::Internal(format!(
            "PSI percentage out of range: {raw}"
        )));
    }
    Ok(basis_points)
}

fn parse_psi_line(line: &str, expected_kind: &str) -> Result<PsiPressureLine, ProviderError> {
    let mut fields = line.split_whitespace();
    let kind = fields
        .next()
        .ok_or_else(|| ProviderError::Internal("PSI line is empty".into()))?;
    if kind != expected_kind {
        return Err(ProviderError::Internal(format!(
            "expected PSI '{expected_kind}' line, got '{kind}'"
        )));
    }

    let mut avg10 = None;
    let mut avg60 = None;
    let mut avg300 = None;
    let mut total = None;
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        match key {
            "avg10" => avg10 = Some(parse_percent_basis_points(value)?),
            "avg60" => avg60 = Some(parse_percent_basis_points(value)?),
            "avg300" => avg300 = Some(parse_percent_basis_points(value)?),
            "total" => {
                total = Some(value.parse::<u64>().map_err(|error| {
                    ProviderError::Internal(format!("invalid PSI total: {error}"))
                })?)
            }
            _ => {}
        }
    }

    Ok(PsiPressureLine {
        avg10_basis_points: avg10
            .ok_or_else(|| ProviderError::Internal("PSI avg10 missing".into()))?,
        avg60_basis_points: avg60
            .ok_or_else(|| ProviderError::Internal("PSI avg60 missing".into()))?,
        avg300_basis_points: avg300
            .ok_or_else(|| ProviderError::Internal("PSI avg300 missing".into()))?,
        total_us: total.ok_or_else(|| ProviderError::Internal("PSI total missing".into()))?,
    })
}

/// Parse Linux `/proc/pressure/memory`.
pub fn parse_memory_pressure(raw: &str) -> Result<MemoryPressureTelemetry, ProviderError> {
    let mut some = None;
    let mut full = None;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        if line.starts_with("some ") {
            some = Some(parse_psi_line(line, "some")?);
        } else if line.starts_with("full ") {
            full = Some(parse_psi_line(line, "full")?);
        }
    }

    Ok(MemoryPressureTelemetry {
        some: some.ok_or_else(|| ProviderError::Internal("PSI some line missing".into()))?,
        full,
    })
}

fn parse_bool_parameter(raw: &str) -> Result<Option<bool>, ProviderError> {
    match raw.trim() {
        "Y" | "y" | "1" => Ok(Some(true)),
        "N" | "n" | "0" => Ok(Some(false)),
        "" => Ok(None),
        other => Err(ProviderError::Internal(format!(
            "unexpected boolean kernel parameter '{other}'"
        ))),
    }
}

fn read_optional_u64(path: &Path) -> Result<Option<u64>, ProviderError> {
    match fs::read_to_string(path) {
        Ok(raw) => raw
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|error| ProviderError::Internal(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProviderError::Io(error)),
    }
}

fn read_zram_devices(sys_block: &Path) -> Result<Vec<ZramTelemetry>, ProviderError> {
    let entries = match fs::read_dir(sys_block) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ProviderError::Io(error)),
    };

    let mut devices = Vec::new();
    for entry in entries {
        let entry = entry.map_err(ProviderError::Io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(index) = name.strip_prefix("zram") else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }

        let path = entry.path();
        let disk_size_bytes = read_optional_u64(&path.join("disksize"))?;
        let (original_data_bytes, compressed_data_bytes, memory_used_bytes) =
            match fs::read_to_string(path.join("mm_stat")) {
                Ok(raw) => {
                    let values: Vec<u64> = raw
                        .split_whitespace()
                        .take(3)
                        .map(|value| {
                            value.parse::<u64>().map_err(|error| {
                                ProviderError::Internal(format!(
                                    "invalid {name}/mm_stat value: {error}"
                                ))
                            })
                        })
                        .collect::<Result<_, _>>()?;
                    if values.len() < 3 {
                        return Err(ProviderError::Internal(format!(
                            "{name}/mm_stat has fewer than 3 fields"
                        )));
                    }
                    (Some(values[0]), Some(values[1]), Some(values[2]))
                }
                Err(error) if error.kind() == ErrorKind::NotFound => (None, None, None),
                Err(error) => return Err(ProviderError::Io(error)),
            };

        devices.push(ZramTelemetry {
            device: name,
            disk_size_bytes,
            original_data_bytes,
            compressed_data_bytes,
            memory_used_bytes,
        });
    }
    devices.sort_by(|left, right| left.device.cmp(&right.device));
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_parser_preserves_kib_values_and_optional_available() {
        let snapshot = parse_meminfo(
            "MemTotal:       16384 kB\nMemAvailable:    8192 kB\nSwapTotal:       4096 kB\nSwapFree:        1024 kB\n",
        )
        .unwrap();
        assert_eq!(snapshot.total_kib, 16_384);
        assert_eq!(snapshot.available_kib, Some(8_192));
        assert_eq!(snapshot.swap_total_kib, 4_096);
        assert_eq!(snapshot.swap_free_kib, 1_024);
        assert!(snapshot.pressure.is_none());
    }

    #[test]
    fn meminfo_missing_required_field_is_error() {
        assert!(parse_meminfo("MemTotal: 1000 kB\n").is_err());
    }

    #[test]
    fn psi_parser_uses_integer_basis_points() {
        let pressure = parse_memory_pressure(
            "some avg10=12.34 avg60=1.20 avg300=0.01 total=1234\nfull avg10=0.10 avg60=0.00 avg300=0.00 total=10\n",
        )
        .unwrap();
        assert_eq!(pressure.some.avg10_basis_points, 1234);
        assert_eq!(pressure.some.avg60_basis_points, 120);
        assert_eq!(pressure.full.unwrap().avg10_basis_points, 10);
    }

    #[test]
    fn malformed_psi_is_fail_closed() {
        assert!(parse_memory_pressure("some avg10=101.00 avg60=0.00 avg300=0.00 total=1").is_err());
    }

    #[test]
    fn kernel_bool_parser_is_strict() {
        assert_eq!(parse_bool_parameter("Y\n").unwrap(), Some(true));
        assert_eq!(parse_bool_parameter("N\n").unwrap(), Some(false));
        assert!(parse_bool_parameter("maybe").is_err());
    }
}
