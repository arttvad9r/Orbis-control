//! Read-only system memory and pressure telemetry types.

use serde::{Deserialize, Serialize};

/// One Linux PSI pressure line.
///
/// `avg*_basis_points` stores percentage * 100, so `12.34%` is `1234`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsiPressureLine {
    /// 10-second average in basis points of percent.
    pub avg10_basis_points: u16,
    /// 60-second average in basis points of percent.
    pub avg60_basis_points: u16,
    /// 300-second average in basis points of percent.
    pub avg300_basis_points: u16,
    /// Cumulative stalled time in microseconds.
    pub total_us: u64,
}

/// Memory PSI snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPressureTelemetry {
    /// `some` pressure line.
    pub some: PsiPressureLine,
    /// `full` pressure line when exposed by the kernel.
    pub full: Option<PsiPressureLine>,
}

/// RAM/swap telemetry from Linux `/proc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemMemoryTelemetry {
    /// Total RAM in KiB.
    pub total_kib: u64,
    /// Available RAM in KiB when reported by the kernel.
    pub available_kib: Option<u64>,
    /// Total swap in KiB.
    pub swap_total_kib: u64,
    /// Free swap in KiB.
    pub swap_free_kib: u64,
    /// Memory pressure snapshot when PSI is available.
    pub pressure: Option<MemoryPressureTelemetry>,
}

/// One discovered zram device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZramTelemetry {
    /// Kernel device name such as `zram0`.
    pub device: String,
    /// Configured disk size in bytes, when readable.
    pub disk_size_bytes: Option<u64>,
    /// Original uncompressed bytes stored, when readable.
    pub original_data_bytes: Option<u64>,
    /// Compressed data bytes, when readable.
    pub compressed_data_bytes: Option<u64>,
    /// Total memory used by zram, when readable.
    pub memory_used_bytes: Option<u64>,
}

/// Optional zswap state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZswapTelemetry {
    /// Whether zswap is enabled when the kernel parameter is readable.
    pub enabled: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_memory_serde_preserves_optional_pressure() {
        let telemetry = SystemMemoryTelemetry {
            total_kib: 1024,
            available_kib: Some(512),
            swap_total_kib: 256,
            swap_free_kib: 128,
            pressure: None,
        };
        let json = serde_json::to_string(&telemetry).unwrap();
        let back: SystemMemoryTelemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, telemetry);
    }
}
