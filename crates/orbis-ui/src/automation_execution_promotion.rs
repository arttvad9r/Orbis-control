//! Final strict permit between a worker-owned dry-run envelope and unattended
//! hardware execution.
//!
//! Shadow planning and serialization intentionally operate while Automation is
//! advertised as read-only so the pipeline can be proven before release
//! promotion. This module restores the stronger mutation invariant immediately
//! before any owner call: the current immutable capability snapshot must still
//! match the prepared generation, be fresh, and the normal strict Automation
//! preflight must pass. In particular `FeatureId::Automation.write` must be
//! `Supported` here.
//!
//! A permit is move-only and exposes no mutation method. It is evidence for one
//! prepared envelope under one current capability generation, not a global
//! capability upgrade.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::{AutomationPreflight, preflight_automation_plan};

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
    /// The normal strict execution preflight failed. This includes
    /// `Automation.write != Supported` and every action-level blocker.
    StrictPreflightBlocked(AutomationPreflight),
}

/// Move-only strict execution evidence for exactly one prepared envelope.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationExecutionPermit {
    generation: u64,
}

impl AutomationExecutionPermit {
    /// Generation that passed the final strict preflight.
    pub fn required_generation(&self) -> u64 {
        self.generation
    }
}

/// Perform the final pure promotion check immediately before a future owner
/// call. No I/O and no mutation occur here.
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

    // The prepared batch came from a private execution handoff, but the worker
    // envelope intentionally exposes only the typed batch. Reconstruct the
    // strict plan from the persisted policy is already guaranteed by the
    // worker driver's policy-revision/revalidation chain. The exact plan that
    // reached serialization is retained by the lease as actions; the final
    // execution gate therefore uses the prepared envelope's strict-plan helper.
    let plan = envelope
        .strict_plan()
        .clone();
    let preflight = preflight_automation_plan(plan, capabilities.device_capabilities());
    if !preflight.is_ready() {
        return Err(AutomationExecutionPromotionBlock::StrictPreflightBlocked(
            preflight,
        ));
    }

    Ok(AutomationExecutionPermit { generation: current })
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::{AutomationPolicy, DesiredPerformancePolicy};
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;
    use orbis_core::telemetry::Telemetry;

    use crate::automation_worker_driver::AutomationWorkerDriver;

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
            crate::automation_worker_driver::AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                capabilities,
                base + Duration::from_secs(1),
            ),
            crate::automation_worker_driver::AutomationWorkerObservation::NoConfirmedEvent
        ));
        let _ = driver.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(2)),
            capabilities,
            base + Duration::from_secs(2),
        );
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
        assert!(matches!(
            authorize_prepared_execution(
                &envelope,
                &capabilities,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            Err(AutomationExecutionPromotionBlock::StrictPreflightBlocked(_))
        ));
    }

    #[test]
    fn supported_runtime_and_exact_action_evidence_can_get_permit() {
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
    fn generation_or_freshness_drift_blocks_before_strict_preflight() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let capabilities = snapshot(9, base, CapabilityStatus::Supported);
        let (_driver, envelope) = prepared(&capabilities, base);
        let newer = snapshot(10, base + Duration::from_secs(1), CapabilityStatus::Supported);
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
