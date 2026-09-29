//! Availability evidence for the NVIDIA tuning mutation.

use std::path::Path;

use orbis_providers::nvidia_tuning::{GpuPresence, gpu_presence};

use crate::cpu_tuning::{CpuTuningStatus, mutation_wire};

/// Wire status: supported while a GPU is bound to the NVIDIA driver, whether
/// awake or asleep (a sleeping GPU is not woken just to answer this).
pub fn mutation_status(sysfs_root: &Path) -> u8 {
    mutation_wire::to_wire(match gpu_presence(sysfs_root) {
        GpuPresence::Absent => CpuTuningStatus::Unsupported,
        GpuPresence::Asleep | GpuPresence::Awake => CpuTuningStatus::Supported,
    })
}
