//! Hardware-inert serialization boundary for future Automation execution.
//!
//! This module accepts only a fully revalidated revision-bound Automation
//! handoff and models exclusive ownership of one future mutation batch. It
//! deliberately contains no provider, worker, D-Bus, sysfs or process execution
//! surface. Admission is fail-closed on lifecycle-revision drift,
//! capability-generation drift, or an already active lease. Completing a lease
//! only releases the serialization slot.

use orbis_core::automation::{AutomationAction, AutomationTrigger};

use crate::automation_lifecycle_revision::{
    AutomationLifecycleRevision, AutomationRevisionHandoff,
};

/// Why a revalidated handoff cannot enter the serialized execution slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationAdmissionBlock {
    /// A newer confirmed lifecycle event superseded this handoff after
    /// revalidation and before serialization admission.
    LifecycleRevisionChanged {
        /// Lifecycle revision required by the revalidated handoff.
        required: AutomationLifecycleRevision,
        /// Lifecycle revision current at admission.
        current: AutomationLifecycleRevision,
    },
    /// Capability generation changed after revalidation and before admission.
    CapabilityGenerationChanged {
        /// Generation required by the revalidated handoff.
        required: u64,
        /// Generation current at admission.
        current: u64,
    },
    /// Another handoff already owns the single serialization slot.
    Busy {
        /// Opaque identifier of the active lease.
        active_lease: u64,
    },
    /// The monotonic lease identifier space was exhausted.
    SequenceExhausted,
}

/// Result of trying to enter the hardware-inert serialized slot.
#[derive(Debug, PartialEq, Eq)]
pub enum AutomationAdmissionOutcome {
    /// Admission was rejected without any side effect outside this coordinator.
    Blocked(AutomationAdmissionBlock),
    /// The handoff owns the one dry-run slot. No hardware action has run.
    Admitted(AutomationDryRunLease),
}

/// Exclusive hardware-inert lease for one future Automation batch.
///
/// Fields are private and the type is not `Clone`, so external code cannot
/// forge or duplicate a lease. The action slice is diagnostic intent only; this
/// type exposes no execution method.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationDryRunLease {
    id: u64,
    required_revision: AutomationLifecycleRevision,
    required_generation: u64,
    trigger: AutomationTrigger,
    actions: Vec<AutomationAction>,
}

impl AutomationDryRunLease {
    /// Opaque monotonic identifier used only for serialization/audit state.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Lifecycle revision that must still be current for a future executor.
    pub fn required_revision(&self) -> AutomationLifecycleRevision {
        self.required_revision
    }

    /// Capability generation that must still be current for a future executor.
    pub fn required_generation(&self) -> u64 {
        self.required_generation
    }

    /// Lifecycle trigger represented by this batch.
    pub fn trigger(&self) -> &AutomationTrigger {
        &self.trigger
    }

    /// Exact ordered action intent that passed both preflight stages.
    pub fn actions(&self) -> &[AutomationAction] {
        &self.actions
    }
}

/// Single-slot coordinator that models future executor serialization.
///
/// The coordinator performs no asynchronous work. It only ensures at most one
/// revalidated handoff owns the slot and checks lifecycle + capability identity
/// again at admission. The real executor will later need to hold the same
/// serialization ownership across mutation and authoritative read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationSerializationCoordinator {
    next_lease: u64,
    active_lease: Option<u64>,
}

impl AutomationSerializationCoordinator {
    /// Construct an empty coordinator.
    pub fn new() -> Self {
        Self {
            next_lease: 1,
            active_lease: None,
        }
    }

    /// Whether one handoff currently owns the dry-run slot.
    pub fn is_busy(&self) -> bool {
        self.active_lease.is_some()
    }

    /// Admit one already-revalidated handoff into the exclusive dry-run slot.
    ///
    /// Both current identities must be sampled by the future serialization
    /// owner immediately before this call. Drift or an occupied slot blocks the
    /// whole batch. No partial admission exists.
    pub fn admit(
        &mut self,
        handoff: AutomationRevisionHandoff,
        current_revision: AutomationLifecycleRevision,
        current_generation: u64,
    ) -> AutomationAdmissionOutcome {
        let required_revision = handoff.required_revision();
        if required_revision != current_revision {
            return AutomationAdmissionOutcome::Blocked(
                AutomationAdmissionBlock::LifecycleRevisionChanged {
                    required: required_revision,
                    current: current_revision,
                },
            );
        }

        let required_generation = handoff.required_generation();
        if required_generation != current_generation {
            return AutomationAdmissionOutcome::Blocked(
                AutomationAdmissionBlock::CapabilityGenerationChanged {
                    required: required_generation,
                    current: current_generation,
                },
            );
        }

        if let Some(active_lease) = self.active_lease {
            return AutomationAdmissionOutcome::Blocked(AutomationAdmissionBlock::Busy {
                active_lease,
            });
        }

        if self.next_lease == 0 {
            return AutomationAdmissionOutcome::Blocked(
                AutomationAdmissionBlock::SequenceExhausted,
            );
        }

        let id = self.next_lease;
        self.next_lease = self.next_lease.checked_add(1).unwrap_or(0);
        self.active_lease = Some(id);

        AutomationAdmissionOutcome::Admitted(AutomationDryRunLease {
            id,
            required_revision,
            required_generation,
            trigger: handoff.trigger().clone(),
            actions: handoff.actions().to_vec(),
        })
    }

    /// Release the slot by consuming the exact lease object that owns it.
    ///
    /// Returning `false` means the supplied lease no longer matches the active
    /// slot. No other state is changed in that case.
    pub fn finish(&mut self, lease: AutomationDryRunLease) -> bool {
        if self.active_lease != Some(lease.id) {
            return false;
        }
        self.active_lease = None;
        true
    }
}

impl Default for AutomationSerializationCoordinator {
    fn default() -> Self {
        Self::new()
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
    use orbis_core::profile::PerformanceProfile;
    use orbis_core::telemetry::Telemetry;

    use crate::automation_lifecycle_revision::{
        AutomationLifecycleClock, AutomationRevisionCandidate, AutomationRevisionGuardOutcome,
        revalidate_revision_candidate,
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

    fn handoff(
        generation: u64,
    ) -> (AutomationLifecycleRevision, AutomationRevisionHandoff) {
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
        let mut clock = AutomationLifecycleClock::new();
        let revision = clock.advance().unwrap();
        let candidate = AutomationRevisionCandidate::from_shadow(&outcome, revision).unwrap();
        let guard = revalidate_revision_candidate(
            &candidate,
            revision,
            &policy,
            &snapshot,
            base + Duration::from_secs(3),
            Duration::from_secs(30),
        );
        let AutomationRevisionGuardOutcome::Ready(handoff) = guard else {
            panic!("expected revalidated revision handoff");
        };
        (revision, handoff)
    }

    #[test]
    fn matching_identities_admit_only_one_hardware_inert_lease() {
        let mut coordinator = AutomationSerializationCoordinator::new();
        let (revision, handoff) = handoff(7);
        let outcome = coordinator.admit(handoff, revision, 7);
        let AutomationAdmissionOutcome::Admitted(lease) = outcome else {
            panic!("expected dry-run lease");
        };
        assert!(coordinator.is_busy());
        assert_eq!(lease.id(), 1);
        assert_eq!(lease.required_revision(), revision);
        assert_eq!(lease.required_generation(), 7);
        assert_eq!(lease.trigger(), &AutomationTrigger::OnBattery);
        assert_eq!(
            lease.actions(),
            &[AutomationAction::SetProfile(PerformanceProfile::Balanced)]
        );
        assert!(coordinator.finish(lease));
        assert!(!coordinator.is_busy());
    }

    #[test]
    fn lifecycle_revision_drift_blocks_before_slot_ownership() {
        let mut coordinator = AutomationSerializationCoordinator::new();
        let (revision, handoff) = handoff(8);
        let mut clock = AutomationLifecycleClock::new();
        assert_eq!(clock.advance().unwrap(), revision);
        let newer = clock.advance().unwrap();
        assert_eq!(
            coordinator.admit(handoff, newer, 8),
            AutomationAdmissionOutcome::Blocked(
                AutomationAdmissionBlock::LifecycleRevisionChanged {
                    required: revision,
                    current: newer,
                }
            )
        );
        assert!(!coordinator.is_busy());
    }

    #[test]
    fn generation_drift_blocks_before_slot_ownership() {
        let mut coordinator = AutomationSerializationCoordinator::new();
        let (revision, handoff) = handoff(9);
        assert_eq!(
            coordinator.admit(handoff, revision, 10),
            AutomationAdmissionOutcome::Blocked(
                AutomationAdmissionBlock::CapabilityGenerationChanged {
                    required: 9,
                    current: 10,
                }
            )
        );
        assert!(!coordinator.is_busy());
    }

    #[test]
    fn occupied_slot_rejects_second_batch_without_partial_state() {
        let mut coordinator = AutomationSerializationCoordinator::new();
        let (revision, first_handoff) = handoff(11);
        let first = coordinator.admit(first_handoff, revision, 11);
        let AutomationAdmissionOutcome::Admitted(first_lease) = first else {
            panic!("expected first lease");
        };
        let (_, second_handoff) = handoff(11);
        assert_eq!(
            coordinator.admit(second_handoff, revision, 11),
            AutomationAdmissionOutcome::Blocked(AutomationAdmissionBlock::Busy {
                active_lease: first_lease.id(),
            })
        );
        assert!(coordinator.is_busy());
        assert!(coordinator.finish(first_lease));
    }

    #[test]
    fn source_has_no_execution_surface() {
        let source = include_str!("automation_serialization.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "profile("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["set_", "charge_limit("].concat(),
            ["Command", "::new"].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected execution token: {needle}");
        }
        assert!(!source.contains("unsafe"));
    }
}