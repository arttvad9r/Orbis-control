//! Worker-owned Automation coordinator including unknown-outcome recovery.
//!
//! `AutomationWorkerRuntime` owns lifecycle/revision/preflight/serialization
//! state. `AutomationRecoveryBarrier` owns the stronger invariant required after
//! a mutation may have happened but authoritative read-back failed. This wrapper
//! composes both under one non-cloneable owner without adding hardware I/O.

use std::time::{Duration, SystemTime};

use orbis_application::PerformanceState;
use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::AutomationPolicy;
use orbis_core::lifecycle::ResumeGateOutcome;
use orbis_core::profile::PerformanceProfile;
use orbis_core::telemetry::Telemetry;

use crate::automation_execution_scope::AutomationPreparedKind;
use crate::automation_lifecycle_revision::AutomationLifecycleRevision;
use crate::automation_recovery::{
    AutomationPerformanceRecoveryOutcome, AutomationRecoveryBarrier, AutomationRecoveryRecord,
};
use crate::automation_worker_runtime::{
    AutomationConfirmedEvent, AutomationWorkerObservationError, AutomationWorkerPrepareBlock,
    AutomationWorkerPrepared, AutomationWorkerRuntime,
};

/// Why the coordinator refuses to prepare another unattended batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationCoordinatorPrepareBlock {
    /// A previous mutation outcome is still unknown and requires an
    /// authoritative typed reconciliation read.
    RecoveryRequired {
        /// Immutable audit record for the unknown mutation attempt.
        record: AutomationRecoveryRecord,
    },
    /// Normal lifecycle/preflight/serialization/scope preparation blocker.
    Runtime(AutomationWorkerPrepareBlock),
}

/// Why an execution owner could not finish a prepared batch in the requested
/// state transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationCoordinatorFinishError {
    /// The supplied prepared batch is not a Performance mutation and therefore
    /// cannot enter Performance recovery.
    NotPerformanceBatch,
    /// Recovery was already active. A second unknown result cannot replace the
    /// first record.
    RecoveryAlreadyRequired,
    /// The inner serialization owner did not recognize the supplied lease.
    LeaseReleaseFailed,
}

/// Unique future worker owner for Automation preparation + recovery state.
///
/// Deliberately not `Clone`: cloning would split serialization and recovery
/// histories. No method performs provider/worker mutation.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerCoordinator {
    runtime: AutomationWorkerRuntime,
    recovery: AutomationRecoveryBarrier,
}

impl AutomationWorkerCoordinator {
    /// Construct a clear, hardware-inert coordinator.
    pub fn new() -> Self {
        Self {
            runtime: AutomationWorkerRuntime::new(),
            recovery: AutomationRecoveryBarrier::new(),
        }
    }

    /// Current monotonic lifecycle revision.
    pub fn current_revision(&self) -> AutomationLifecycleRevision {
        self.runtime.current_revision()
    }

    /// Whether the serialization slot or unknown-outcome barrier blocks new
    /// execution preparation.
    pub fn is_blocked(&self) -> bool {
        self.runtime.is_busy() || self.recovery.is_blocked()
    }

    /// Current unknown-outcome record, if reconciliation is required.
    pub fn recovery_record(&self) -> Option<&AutomationRecoveryRecord> {
        self.recovery.pending()
    }

    /// Pass a logind sleep/resume observation to the lifecycle runtime.
    pub fn observe_prepare_for_sleep(
        &mut self,
        start: bool,
        observed_at: SystemTime,
    ) -> ResumeGateOutcome {
        self.runtime.observe_prepare_for_sleep(start, observed_at)
    }

    /// Pass one authoritative telemetry snapshot to lifecycle observation.
    ///
    /// Recovery does not suppress observation or lifecycle revision advancement:
    /// newer physical events may still supersede older candidates while state is
    /// unknown. Admission remains blocked until reconciliation succeeds.
    pub fn observe_telemetry(
        &mut self,
        telemetry: &Telemetry,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> Result<Option<AutomationConfirmedEvent>, AutomationWorkerObservationError> {
        self.runtime
            .observe_telemetry(telemetry, policy, capabilities, now)
    }

    /// Prepare the newest eligible event only when no unknown-outcome recovery
    /// is pending.
    pub fn prepare_latest(
        &mut self,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
        max_capability_age: Duration,
    ) -> Result<AutomationWorkerPrepared, AutomationCoordinatorPrepareBlock> {
        if let Some(record) = self.recovery.pending() {
            return Err(AutomationCoordinatorPrepareBlock::RecoveryRequired {
                record: record.clone(),
            });
        }
        self.runtime
            .prepare_latest(policy, capabilities, now, max_capability_age)
            .map_err(AutomationCoordinatorPrepareBlock::Runtime)
    }

    /// Finish a batch whose outcome is already authoritatively known.
    ///
    /// This only releases serialization ownership. It does not mark success;
    /// success semantics belong to the typed executor/read-back result.
    pub fn finish_known(
        &mut self,
        prepared: AutomationWorkerPrepared,
    ) -> Result<(), AutomationCoordinatorFinishError> {
        if self.runtime.finish(prepared) {
            Ok(())
        } else {
            Err(AutomationCoordinatorFinishError::LeaseReleaseFailed)
        }
    }

    /// Finish a Performance batch whose mutation may have occurred but whose
    /// authoritative read-back is unknown.
    ///
    /// Recovery is marked *before* releasing the serialization lease. If lease
    /// release unexpectedly fails, the coordinator remains fail-closed with both
    /// recovery and serialization ownership rather than permitting another
    /// unattended operation.
    pub fn finish_performance_unknown(
        &mut self,
        prepared: AutomationWorkerPrepared,
    ) -> Result<(), AutomationCoordinatorFinishError> {
        let requested = match prepared.batch().kind() {
            AutomationPreparedKind::Performance(profile) => *profile,
            AutomationPreparedKind::NoOp => {
                return Err(AutomationCoordinatorFinishError::NotPerformanceBatch);
            }
        };

        if !self
            .recovery
            .mark_performance_unknown(prepared.lease(), requested)
        {
            return Err(AutomationCoordinatorFinishError::RecoveryAlreadyRequired);
        }

        if self.runtime.finish(prepared) {
            Ok(())
        } else {
            Err(AutomationCoordinatorFinishError::LeaseReleaseFailed)
        }
    }

    /// Reconcile a pending Performance unknown outcome from one successful
    /// authoritative read.
    pub fn reconcile_performance(
        &mut self,
        state: &PerformanceState,
    ) -> AutomationPerformanceRecoveryOutcome {
        self.recovery.reconcile_performance(state)
    }
}

impl Default for AutomationWorkerCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::DesiredPerformancePolicy;
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };

    fn capability(write: CapabilityStatus, constraints: CapabilityConstraints) -> Capability {
        Capability::new(CapabilityStatus::Supported)
            .with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(write),
            })
            .with_constraints(constraints)
    }

    fn snapshot(generation: u64, checked_at: SystemTime) -> CapabilityRegistrySnapshot {
        let mut builder = CapabilityRegistryBuilder::new(generation, checked_at);
        builder
            .add(
                FeatureId::Automation,
                capability(CapabilityStatus::Supported, CapabilityConstraints::None),
            )
            .unwrap();
        builder
            .add(
                FeatureId::Performance,
                capability(
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

    fn telemetry(ac_online: bool, ts: SystemTime) -> Telemetry {
        let mut telemetry = Telemetry::empty();
        telemetry.ac_online = Some(ac_online);
        telemetry.ts = ts;
        telemetry
    }

    fn prepare(
        coordinator: &mut AutomationWorkerCoordinator,
        policy: &AutomationPolicy,
        snapshot: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> AutomationWorkerPrepared {
        assert!(coordinator
            .observe_telemetry(&telemetry(true, base), policy, snapshot, base)
            .unwrap()
            .is_none());
        assert!(coordinator
            .observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                policy,
                snapshot,
                base + Duration::from_secs(1),
            )
            .unwrap()
            .is_none());
        coordinator
            .observe_telemetry(
                &telemetry(false, base + Duration::from_secs(2)),
                policy,
                snapshot,
                base + Duration::from_secs(2),
            )
            .unwrap()
            .expect("confirmed event");
        coordinator
            .prepare_latest(
                policy,
                snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            )
            .expect("prepared")
    }

    fn state(current: PerformanceProfile) -> PerformanceState {
        PerformanceState {
            current,
            available: PerformanceProfile::ALL.to_vec(),
        }
    }

    #[test]
    fn unknown_performance_outcome_releases_slot_but_blocks_next_admission() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(7, base);
        let mut coordinator = AutomationWorkerCoordinator::new();
        let prepared = prepare(&mut coordinator, &policy, &snapshot, base);

        coordinator.finish_performance_unknown(prepared).unwrap();
        assert!(coordinator.is_blocked());
        assert!(coordinator.recovery_record().is_some());
        assert!(matches!(
            coordinator.prepare_latest(
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
                Duration::from_secs(30),
            ),
            Err(AutomationCoordinatorPrepareBlock::RecoveryRequired { .. })
        ));
    }

    #[test]
    fn successful_authoritative_read_clears_unknown_even_on_target_mismatch() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(9, base);
        let mut coordinator = AutomationWorkerCoordinator::new();
        let prepared = prepare(&mut coordinator, &policy, &snapshot, base);
        coordinator.finish_performance_unknown(prepared).unwrap();

        assert_eq!(
            coordinator.reconcile_performance(&state(PerformanceProfile::Silent)),
            AutomationPerformanceRecoveryOutcome::RecoveredAtDifferent {
                requested: PerformanceProfile::Balanced,
                current: PerformanceProfile::Silent,
            }
        );
        assert!(!coordinator.is_blocked());
    }

    #[test]
    fn newer_lifecycle_event_does_not_clear_recovery_barrier() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(11, base);
        let mut coordinator = AutomationWorkerCoordinator::new();
        let prepared = prepare(&mut coordinator, &policy, &snapshot, base);
        coordinator.finish_performance_unknown(prepared).unwrap();
        let record = coordinator.recovery_record().cloned().unwrap();

        coordinator
            .observe_telemetry(
                &telemetry(true, base + Duration::from_secs(4)),
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
            )
            .unwrap();
        coordinator
            .observe_telemetry(
                &telemetry(true, base + Duration::from_secs(5)),
                &policy,
                &snapshot,
                base + Duration::from_secs(5),
            )
            .unwrap();

        assert_eq!(coordinator.recovery_record(), Some(&record));
        assert!(coordinator.is_blocked());
    }

    #[test]
    fn known_finish_releases_slot_without_creating_recovery() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(13, base);
        let mut coordinator = AutomationWorkerCoordinator::new();
        let prepared = prepare(&mut coordinator, &policy, &snapshot, base);
        coordinator.finish_known(prepared).unwrap();
        assert!(!coordinator.is_blocked());
        assert!(coordinator.recovery_record().is_none());
    }

    #[test]
    fn source_is_hardware_inert_and_has_no_generic_recovery_clear() {
        let source = include_str!("automation_worker_coordinator.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "performance("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["Command", "::new"].concat(),
            ["pub fn ", "clear_recovery"].concat(),
            ["pub fn ", "reset_recovery"].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected coordinator escape: {needle}");
        }
        assert!(!source.contains("unsafe"));
    }
}
