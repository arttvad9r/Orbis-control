//! Hardware-inert recovery barrier for Automation mutation outcomes that cannot
//! be authoritatively confirmed.
//!
//! A provider mutation followed by read-back failure is qualitatively different
//! from a command failure: hardware may already have changed. The Automation
//! runtime must therefore stop admitting further unattended work until a fresh
//! authoritative read establishes a known state again. This module models that
//! barrier without performing I/O or mutations itself.

use orbis_application::PerformanceState;
use orbis_core::profile::PerformanceProfile;

use crate::automation_lifecycle_revision::AutomationLifecycleRevision;
use crate::automation_serialization::AutomationDryRunLease;

/// Mutation family whose state is currently unknown after an Automation write.
///
/// The first executor scope is Performance-only. Future families must add their
/// own typed reconciliation evidence rather than sharing a generic "clear"
/// operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationRecoveryKind {
    /// A Performance mutation returned a result, but its mandatory read-back
    /// failed or could not prove the requested state.
    Performance {
        /// Profile that the unknown-outcome mutation attempted to apply.
        requested: PerformanceProfile,
    },
}

/// Immutable audit identity of the mutation that forced recovery mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRecoveryRecord {
    lease_id: u64,
    revision: AutomationLifecycleRevision,
    generation: u64,
    kind: AutomationRecoveryKind,
}

impl AutomationRecoveryRecord {
    /// Serialization lease that owned the mutation attempt.
    pub fn lease_id(&self) -> u64 {
        self.lease_id
    }

    /// Lifecycle revision that produced the mutation attempt.
    pub fn revision(&self) -> AutomationLifecycleRevision {
        self.revision
    }

    /// Capability generation current when the mutation attempt was admitted.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Typed recovery family and requested value.
    pub fn kind(&self) -> &AutomationRecoveryKind {
        &self.kind
    }
}

/// Result of reconciling one authoritative Performance read while recovery is
/// required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPerformanceRecoveryOutcome {
    /// No Performance recovery was pending; state was left unchanged.
    NotRequired,
    /// Fresh authoritative state is known and equals the originally requested
    /// profile. Recovery barrier is cleared.
    RecoveredAtRequested {
        /// Authoritative current profile.
        current: PerformanceProfile,
    },
    /// Fresh authoritative state is known but differs from the requested
    /// profile. Recovery barrier is still cleared because state is no longer
    /// unknown; a future lifecycle plan may decide whether to reconcile it.
    RecoveredAtDifferent {
        /// Originally requested profile.
        requested: PerformanceProfile,
        /// Authoritative current profile.
        current: PerformanceProfile,
    },
}

/// Single global Automation unknown-outcome barrier.
///
/// Deliberately not `Clone`: production must have one owner for recovery state,
/// colocated with the worker serialization owner. A pending record cannot be
/// cleared by lifecycle changes, policy edits or capability-generation changes.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct AutomationRecoveryBarrier {
    pending: Option<AutomationRecoveryRecord>,
}

impl AutomationRecoveryBarrier {
    /// Construct a clear recovery barrier.
    pub fn new() -> Self {
        Self { pending: None }
    }

    /// Whether unattended Automation admission must remain blocked.
    pub fn is_blocked(&self) -> bool {
        self.pending.is_some()
    }

    /// Current unknown-outcome record, if any.
    pub fn pending(&self) -> Option<&AutomationRecoveryRecord> {
        self.pending.as_ref()
    }

    /// Enter Performance recovery after a mutation result existed but the final
    /// authoritative state was not proven.
    ///
    /// The exact lease identity is retained for diagnostics. A second unknown
    /// outcome cannot replace an existing record because no new unattended
    /// mutation should be admitted while this barrier is already active.
    pub fn mark_performance_unknown(
        &mut self,
        lease: &AutomationDryRunLease,
        requested: PerformanceProfile,
    ) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some(AutomationRecoveryRecord {
            lease_id: lease.id(),
            revision: lease.required_revision(),
            generation: lease.required_generation(),
            kind: AutomationRecoveryKind::Performance { requested },
        });
        true
    }

    /// Reconcile a pending Performance unknown outcome from one successful
    /// authoritative `PerformanceState` read.
    ///
    /// Any successfully read current profile is sufficient to leave the
    /// *unknown* state. A mismatch is not silently called successful application;
    /// it is returned distinctly so the caller may publish diagnostics and let a
    /// later lifecycle event re-plan against the now-known state.
    pub fn reconcile_performance(
        &mut self,
        state: &PerformanceState,
    ) -> AutomationPerformanceRecoveryOutcome {
        let Some(record) = self.pending.as_ref() else {
            return AutomationPerformanceRecoveryOutcome::NotRequired;
        };
        let AutomationRecoveryKind::Performance { requested } = record.kind;
        let requested = requested;
        let current = state.current;

        self.pending = None;
        if current == requested {
            AutomationPerformanceRecoveryOutcome::RecoveredAtRequested { current }
        } else {
            AutomationPerformanceRecoveryOutcome::RecoveredAtDifferent {
                requested,
                current,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    use orbis_capabilities::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot};
    use orbis_config::{AutomationPolicy, DesiredPerformancePolicy};
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };
    use orbis_core::telemetry::Telemetry;

    use crate::automation_execution_guard::{
        AutomationExecutionCandidate, AutomationExecutionGuardOutcome,
        revalidate_automation_candidate,
    };
    use crate::automation_lifecycle_revision::{
        AutomationLifecycleClock, AutomationRevisionCandidate, AutomationRevisionGuardOutcome,
        revalidate_revision_candidate,
    };
    use crate::automation_serialization::{
        AutomationAdmissionOutcome, AutomationSerializationCoordinator,
    };
    use crate::automation_shadow_runtime::AutomationShadowRuntime;

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

    fn lease(generation: u64) -> AutomationDryRunLease {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(generation, base);
        let policy = policy();
        let mut shadow = AutomationShadowRuntime::default();
        shadow.observe_telemetry(&telemetry(true, base), &policy, &snapshot, base);
        shadow.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(1)),
            &policy,
            &snapshot,
            base + Duration::from_secs(1),
        );
        let outcome = shadow.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(2)),
            &policy,
            &snapshot,
            base + Duration::from_secs(2),
        );
        let inner = AutomationExecutionCandidate::from_shadow(&outcome).unwrap();
        let mut clock = AutomationLifecycleClock::new();
        let revision = clock.advance().unwrap();
        let candidate = AutomationRevisionCandidate::from_shadow(&outcome, revision).unwrap();
        assert_eq!(candidate.generation(), inner.generation());
        let guard = revalidate_revision_candidate(
            &candidate,
            revision,
            &policy,
            &snapshot,
            base + Duration::from_secs(3),
            Duration::from_secs(30),
        );
        let AutomationRevisionGuardOutcome::Ready(handoff) = guard else {
            panic!("expected revision handoff");
        };
        let mut serialization = AutomationSerializationCoordinator::new();
        let admission = serialization.admit(handoff, revision, generation);
        let AutomationAdmissionOutcome::Admitted(lease) = admission else {
            panic!("expected lease");
        };
        lease
    }

    fn state(current: PerformanceProfile) -> PerformanceState {
        PerformanceState {
            current,
            available: PerformanceProfile::ALL.to_vec(),
        }
    }

    #[test]
    fn unknown_outcome_blocks_until_authoritative_state_is_read() {
        let lease = lease(7);
        let mut barrier = AutomationRecoveryBarrier::new();
        assert!(barrier.mark_performance_unknown(&lease, PerformanceProfile::Balanced));
        assert!(barrier.is_blocked());
        let record = barrier.pending().unwrap();
        assert_eq!(record.lease_id(), lease.id());
        assert_eq!(record.revision(), lease.required_revision());
        assert_eq!(record.generation(), 7);

        assert_eq!(
            barrier.reconcile_performance(&state(PerformanceProfile::Balanced)),
            AutomationPerformanceRecoveryOutcome::RecoveredAtRequested {
                current: PerformanceProfile::Balanced,
            }
        );
        assert!(!barrier.is_blocked());
    }

    #[test]
    fn mismatching_authoritative_state_clears_unknown_but_reports_difference() {
        let lease = lease(9);
        let mut barrier = AutomationRecoveryBarrier::new();
        barrier.mark_performance_unknown(&lease, PerformanceProfile::Balanced);
        assert_eq!(
            barrier.reconcile_performance(&state(PerformanceProfile::Silent)),
            AutomationPerformanceRecoveryOutcome::RecoveredAtDifferent {
                requested: PerformanceProfile::Balanced,
                current: PerformanceProfile::Silent,
            }
        );
        assert!(!barrier.is_blocked());
    }

    #[test]
    fn lifecycle_or_capability_changes_cannot_clear_pending_record() {
        let lease = lease(11);
        let mut barrier = AutomationRecoveryBarrier::new();
        barrier.mark_performance_unknown(&lease, PerformanceProfile::Balanced);
        let before = barrier.pending().cloned();
        // The barrier intentionally exposes no lifecycle/policy/capability clear
        // method. Merely inspecting different numbers cannot change it.
        let _newer_revision = AutomationLifecycleRevision::INITIAL;
        let _newer_generation = 99_u64;
        assert_eq!(barrier.pending(), before.as_ref());
        assert!(barrier.is_blocked());
    }

    #[test]
    fn second_unknown_cannot_overwrite_first_record() {
        let first = lease(13);
        let second = lease(14);
        let mut barrier = AutomationRecoveryBarrier::new();
        assert!(barrier.mark_performance_unknown(&first, PerformanceProfile::Balanced));
        assert!(!barrier.mark_performance_unknown(&second, PerformanceProfile::Turbo));
        assert_eq!(barrier.pending().unwrap().lease_id(), first.id());
    }

    #[test]
    fn recovery_source_has_no_mutation_or_generic_clear_surface() {
        let source = include_str!("automation_recovery.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "performance("].concat(),
            ["set_", "profile("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["Command", "::new"].concat(),
            ["pub fn ", "clear("].concat(),
            ["pub fn ", "reset("].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected recovery escape: {needle}");
        }
        assert!(!source.contains("unsafe"));
    }
}
