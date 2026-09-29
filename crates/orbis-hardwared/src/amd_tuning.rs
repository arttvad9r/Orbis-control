//! Availability evidence for the AMD Curve Optimizer mutation.

use std::sync::atomic::{AtomicBool, Ordering};

use orbis_providers::amd_tuning::{RyzenAdjRunner, probe};

use crate::cpu_tuning::{CpuTuningStatus, mutation_wire};

/// Set once the SMU refused a Curve Optimizer command: the firmware has none.
static SMU_REFUSED: AtomicBool = AtomicBool::new(false);

/// Remember an `Unsupported` answer so the status stops advertising the write.
pub fn note_result(result: &Result<i32, orbis_providers::error::ProviderError>) {
    if matches!(
        result,
        Err(orbis_providers::error::ProviderError::Unsupported(_))
    ) {
        SMU_REFUSED.store(true, Ordering::Relaxed);
    }
}

/// Wire status: supported while the SMU answers a read-only `--info` and has not
/// refused a Curve Optimizer command.
pub fn mutation_status(runner: &dyn RyzenAdjRunner) -> u8 {
    mutation_wire::to_wire(if !SMU_REFUSED.load(Ordering::Relaxed) && probe(runner) {
        CpuTuningStatus::Supported
    } else {
        CpuTuningStatus::Unsupported
    })
}
