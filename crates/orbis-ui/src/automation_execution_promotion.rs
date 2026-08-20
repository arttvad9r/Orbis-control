//! Final strict permit between a worker-owned dry-run envelope and unattended
//! hardware execution.
//!
//! Shadow planning/revalidation deliberately removes exactly one strict blocker:
//! the Automation runtime write bit, while preserving every action-level
//! write/target/requirement blocker. The serialized envelope is bound to one
//! immutable capability generation. Therefore the final execution delta is
//! intentionally narrow: the same generation must still be current/fresh and
//! `FeatureId::Automation.write` must now be exactly `Supported`.
//!
//! A permit is move-only and exposes no mutation method. It is evidence for one
//! prepared envelope under one current capability generation, not a global
//! capability upgrade.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_core::capability::{CapabilityStatus, FeatureId};

use crate::automation_worker_driver::AutomationWorkerPreparedEnvelope;

/// Why a prepared dry-run envelope cannot cross into unattended execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationExecutionPromotionBlock {
    /// Capability registry generation changed after preparation.
    CapabilityGenerationChanged { required: u64, current: u64 },
    /// Capability timestamp is in the future and therefore untrusted.
    CapabilitySnapshotFromFuture,
    /// Capability evidence is older than the execution freshness budget.
    CapabilitySnapshotStale,
    /// Automation runtime itself is not directly writable. Requirement-bearing,
    /// unsupported, unavailable and unknown states all fail closed.
    AutomationRuntimeWriteUnavailable(CapabilityStatus),
}

/// Move-only strict execution evidence for exactly one prepared envelope.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationExecutionPermit {
    generation: u64,
}

impl AutomationExecutionPermit {
    /// Generation that passed the final strict runtime-write gate.
    pub fn required_generation(&self) -> u64 {
        self.generation
    }
}

/// Perform the final pure promotion check immediately before a future owner
/// call. No I/O and no mutation occur here.
///
/// Action-level evidence is not recomputed from an incomplete serialized DTO:
/// it already passed dry-run preflight and revalidation under the exact same
/// immutable generation. A generation change forces the whole pipeline to plan
/// again before this function can succeed.
pub fn authorize_prepared_execution(
    envelope: &AutomationWorkerPreparedEnvelope,
    capabilities: &CapabilityRegistrySnapshot,
    now: SystemTime,
    max_capability_age: Duration,
) -> Result<AutomationExecutionPermit, AutomationExecutionPromotionBlock> {
    let required = envelope.prepared().lease().required_generation();
    let current = capabilities.generation();
    if current != required {
        return Err(AutomationExecutionPromotionBlock::CapabilityGenerationChanged {
            required,
            current,
        });
    }

    match now.duration_since(capabilities.checked_at()) {
        Err(_) => {
            return Err(AutomationExecutionPromotionBlock::CapabilitySnapshotFromFuture);
        }
        Ok(age) if age > max_capability_age => {
            return Err(AutomationExecutionPromotionBlock::CapabilitySnapshotStale);
        }
        Ok(_) => {}
    }

    let runtime_write = capabilities
        .device_capabilities()
        .features
        .get(&FeatureId::Automation)
        .map(|capability| capability.operations.write.status)
        .unwrap_or(CapabilityStatus::Unknown);
    if runtime_write != CapabilityStatus::Supported {
        return Err(
            AutomationExecutionPromotionBlock::AutomationRuntimeWriteUnavailable(runtime_write),
        );
    }

    Ok(AutomationExecutionPermit { generation: current })
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::{AutomationPolicy, DesiredPerformancePolicy};
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, FeatureId, OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;
    use orbis_core::telemetry::Telemetry;

    use crate::automation_worker_driver::{AutomationWorkerDriver, AutomationWorkerObservation};

    fn capability(
        status: CapabilityStatus,
        write: CapabilityStatus,
        constraints: CapabilityConstraints,
    ) -> Capability {
        Capability::new(status)
            .with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(write),
            })
            .with_constraints(constraints)
    }

    fn snapshot(
        generation: u64,
        checked_at: SystemTime,
        automation_write: CapabilityStatus,
    ) -> CapabilityRegistrySnapshot {
        let mut builder = CapabilityRegistryBuilder::new(generation, checked_at);
        builder
            .add(
                FeatureId::Automation,
                capability(
                    if automation_write == CapabilityStatus::Supported {
                        CapabilityStatus::Supported
                    } else {
                        CapabilityStatus::ReadOnly
                    },
                    automation_write,
                    CapabilityConstraints::None,
                ),
            )
            .unwrap();
        builder
            .add(
                FeatureId::Performance,
                capability(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityConstraints::PerformanceProfiles(vec![PerformanceProfile::Balanced]),
                ),
            )
            .unwrap();
        builder.build().unwrap()
    }

    fn policy() -> AutomationPolicy {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.battery.performance =
            DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced);
        policy
    }

    fn telemetry(ac: bool, ts: SystemTime) -> Telemetry {
        let mut telemetry = Telemetry::empty();
        telemetry.ac_online = Some(ac);
        telemetry.ts = ts;
        telemetry
    }

    fn prepared(
        capabilities: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> (AutomationWorkerDriver, AutomationWorkerPreparedEnvelope) {
        let mut driver = AutomationWorkerDriver::with_policy(policy());
        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), capabilities, base),
            AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                capabilities,
                base + Duration::from_secs(1),
            ),
            AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(2)),
                capabilities,
                base + Duration::from_secs(2),
            ),
            AutomationWorkerObservation::Confirmed(_)
        ));
        let envelope = driver
            .prepare_latest_dry_run(
                capabilities,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            )
            .expect("dry-run envelope");
        (driver, envelope)
    }

    #[test]
    fn read_only_automation_never_gets_execution_permit() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let capabilities = snapshot(7, base, CapabilityStatus::Unsupported);
        let (_driver, envelope) = prepared(&capabilities, base);
        assert_eq!(
            authorize_prepared_execution(
                &envelope,
                &capabilities,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            Err(AutomationExecutionPromotionBlock::AutomationRuntimeWriteUnavailable(
                CapabilityStatus::Unsupported,
            ))
        );
    }

    #[test]
    fn supported_runtime_under_same_generation_can_get_permit() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let capabilities = snapshot(8, base, CapabilityStatus::Supported);
        let (_driver, envelope) = prepared(&capabilities, base);
        let permit = authorize_prepared_execution(
            &envelope,
            &capabilities,
            base + Duration::from_secs(3),
            Duration::from_secs(30),
        )
        .expect("strict permit");
        assert_eq!(permit.required_generation(), 8);
    }

    #[test]
    fn requirement_bearing_runtime_is_not_direct_execution_authority() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let capabilities = snapshot(9, base, CapabilityStatus::SupportedWithRequirement);
        let (_driver, envelope) = prepared(&capabilities, base);
        assert!(matches!(
            authorize_prepared_execution(
                &envelope,
                &capabilities,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            Err(AutomationExecutionPromotionBlock::AutomationRuntimeWriteUnavailable(
                CapabilityStatus::SupportedWithRequirement
            ))
        ));
    }

    #[test]
    fn generation_or_freshness_drift_blocks() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let capabilities = snapshot(10, base, CapabilityStatus::Supported);
        let (_driver, envelope) = prepared(&capabilities, base);
        let newer = snapshot(11, base + Duration::from_secs(1), CapabilityStatus::Supported);
        assert!(matches!(
            authorize_prepared_execution(
                &envelope,
                &newer,
                base + Duration::from_secs(2),
                Duration::from_secs(30),
            ),
            Err(AutomationExecutionPromotionBlock::CapabilityGenerationChanged { .. })
        ));
        assert_eq!(
            authorize_prepared_execution(
                &envelope,
                &capabilities,
                base + Duration::from_secs(40),
                Duration::from_secs(30),
            ),
            Err(AutomationExecutionPromotionBlock::CapabilitySnapshotStale)
        );
    }
}
