//! Worker-facing hardware-inert Automation driver.
//!
//! This is the narrow adapter intended for eventual ownership by `worker.rs`.
//! It keeps the last successfully loaded persisted Automation policy beside the
//! recovery-aware [`AutomationWorkerCoordinator`] and accepts only typed
//! lifecycle/telemetry/capability inputs. It performs no provider mutation and
//! does not know about Slint.
//!
//! Persisted policy is reloaded only through an explicit call. Unsaved UI draft
//! state can therefore never enter the worker lifecycle by accident.

use std::time::{Duration, SystemTime};

use orbis_application::PerformanceState;
use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::{AutomationPolicy, load_automation_policy};
use orbis_core::lifecycle::ResumeGateOutcome;
use orbis_core::telemetry::Telemetry;

use crate::automation_lifecycle_revision::AutomationLifecycleRevision;
use crate::automation_recovery::AutomationPerformanceRecoveryOutcome;
use crate::automation_worker_coordinator::{
    AutomationCoordinatorFinishError, AutomationCoordinatorPrepareBlock,
    AutomationWorkerCoordinator,
};
use crate::automation_worker_runtime::{
    AutomationConfirmedEvent, AutomationWorkerObservationError, AutomationWorkerPrepared,
};

/// Result of explicitly reloading the persisted Automation policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPolicyReloadOutcome {
    /// Hardened desired-state loading succeeded and replaced the worker cache.
    Loaded {
        /// Whether the persisted policy is globally enabled.
        enabled: bool,
    },
    /// The persisted source could not be treated as authoritative user intent.
    /// The previous cached policy is discarded rather than silently retained.
    Unavailable {
        /// Diagnostic-only error text. Execution remains unavailable.
        detail: String,
    },
}

/// Result of one telemetry observation in the worker-facing driver.
#[derive(Debug)]
pub enum AutomationWorkerObservation {
    /// No authoritative persisted policy is currently available.
    PolicyUnavailable,
    /// The sample did not confirm a lifecycle event.
    NoConfirmedEvent,
    /// One lifecycle event reached shadow planning/preflight and received a
    /// monotonic revision. This still performs no mutation.
    Confirmed(AutomationConfirmedEvent),
    /// Lifecycle revision advancement failed closed.
    Failed(AutomationWorkerObservationError),
}

/// Unique owner for persisted Automation intent plus lifecycle/recovery state.
///
/// Deliberately not `Clone`: duplicating this value would split cached policy,
/// lifecycle revision, serialization and recovery histories.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerDriver {
    coordinator: AutomationWorkerCoordinator,
    persisted_policy: Option<AutomationPolicy>,
}

impl AutomationWorkerDriver {
    /// Construct an inert driver with no assumed persisted policy.
    pub fn new() -> Self {
        Self {
            coordinator: AutomationWorkerCoordinator::new(),
            persisted_policy: None,
        }
    }

    /// Construct with an already-authoritative policy.
    ///
    /// Primarily useful for tests and for a future startup path that performs
    /// hardened loading before entering the worker loop.
    pub fn with_policy(policy: AutomationPolicy) -> Self {
        Self {
            coordinator: AutomationWorkerCoordinator::new(),
            persisted_policy: Some(policy),
        }
    }

    /// Explicitly reload `desired-state.toml` through the shared hardened loader.
    ///
    /// Failure clears the cached policy. Retaining an older policy after the
    /// persisted source became malformed would make disk intent and unattended
    /// runtime intent diverge silently.
    pub fn reload_persisted_policy(&mut self) -> AutomationPolicyReloadOutcome {
        match load_automation_policy() {
            Ok(policy) => {
                let enabled = policy.enabled;
                self.persisted_policy = Some(policy);
                AutomationPolicyReloadOutcome::Loaded { enabled }
            }
            Err(error) => {
                self.persisted_policy = None;
                AutomationPolicyReloadOutcome::Unavailable {
                    detail: error.to_string(),
                }
            }
        }
    }

    /// Replace the cached policy from an already-authoritative read-back.
    ///
    /// This method does not accept a UI draft type; callers must supply the same
    /// typed [`AutomationPolicy`] produced by hardened persistence/read-back.
    pub fn replace_persisted_policy(&mut self, policy: AutomationPolicy) {
        self.persisted_policy = Some(policy);
    }

    /// Clear persisted intent explicitly, for example after a hardened reload
    /// reports a preserved/malformed source.
    pub fn clear_persisted_policy(&mut self) {
        self.persisted_policy = None;
    }

    /// Borrow the worker's authoritative cached policy.
    pub fn persisted_policy(&self) -> Option<&AutomationPolicy> {
        self.persisted_policy.as_ref()
    }

    /// Current confirmed lifecycle revision.
    pub fn current_revision(&self) -> AutomationLifecycleRevision {
        self.coordinator.current_revision()
    }

    /// Whether serialization or unknown-outcome recovery blocks preparation.
    pub fn is_blocked(&self) -> bool {
        self.coordinator.is_blocked()
    }

    /// Feed one logind `PrepareForSleep(bool)` observation.
    ///
    /// Sleep/resume tracking is allowed even when policy is temporarily
    /// unavailable; no plan can be produced until a policy exists.
    pub fn observe_prepare_for_sleep(
        &mut self,
        start: bool,
        observed_at: SystemTime,
    ) -> ResumeGateOutcome {
        self.coordinator
            .observe_prepare_for_sleep(start, observed_at)
    }

    /// Feed one authoritative telemetry snapshot under one immutable capability
    /// generation.
    pub fn observe_telemetry(
        &mut self,
        telemetry: &Telemetry,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> AutomationWorkerObservation {
        let Some(policy) = self.persisted_policy.as_ref() else {
            return AutomationWorkerObservation::PolicyUnavailable;
        };

        match self
            .coordinator
            .observe_telemetry(telemetry, policy, capabilities, now)
        {
            Ok(None) => AutomationWorkerObservation::NoConfirmedEvent,
            Ok(Some(event)) => AutomationWorkerObservation::Confirmed(event),
            Err(error) => AutomationWorkerObservation::Failed(error),
        }
    }

    /// Hardware-inert preparation of the latest ready event.
    ///
    /// This exposes the existing revalidation/serialization/scope proof to the
    /// future worker integration. It still performs no mutation. Production
    /// Automation capability currently advertises write=Unsupported, so real
    /// snapshots cannot pass to a mutation executor through this driver.
    pub fn prepare_latest_dry_run(
        &mut self,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
        max_capability_age: Duration,
    ) -> Result<AutomationWorkerPrepared, AutomationCoordinatorPrepareBlock> {
        let Some(policy) = self.persisted_policy.as_ref() else {
            return Err(AutomationCoordinatorPrepareBlock::Runtime(
                crate::automation_worker_runtime::AutomationWorkerPrepareBlock::NoReadyCandidate,
            ));
        };
        self.coordinator
            .prepare_latest(policy, capabilities, now, max_capability_age)
    }

    /// Release one hardware-inert prepared batch with a known/no-execution
    /// outcome. No success state is synthesized here.
    pub fn finish_dry_run(
        &mut self,
        prepared: AutomationWorkerPrepared,
    ) -> Result<(), AutomationCoordinatorFinishError> {
        self.coordinator.finish_known(prepared)
    }

    /// Reconcile a pending unknown Performance mutation from one authoritative
    /// typed read. This method itself performs no read or write.
    pub fn reconcile_performance(
        &mut self,
        state: &PerformanceState,
    ) -> AutomationPerformanceRecoveryOutcome {
        self.coordinator.reconcile_performance(state)
    }
}

impl Default for AutomationWorkerDriver {
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
    use orbis_core::profile::PerformanceProfile;

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
                Capability::new(CapabilityStatus::ReadOnly).with_operations(
                    CapabilityOperations {
                        read: OperationCapability::new(CapabilityStatus::Supported),
                        write: OperationCapability::new(CapabilityStatus::Unsupported),
                    },
                ),
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

    #[test]
    fn no_policy_never_advances_lifecycle() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(7, base);
        let mut driver = AutomationWorkerDriver::new();
        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), &snapshot, base),
            AutomationWorkerObservation::PolicyUnavailable
        ));
        assert_eq!(driver.current_revision(), AutomationLifecycleRevision::INITIAL);
    }

    #[test]
    fn read_only_runtime_can_observe_event_but_never_produce_ready_candidate() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(9, base);
        let mut driver = AutomationWorkerDriver::with_policy(policy());

        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), &snapshot, base),
            AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationWorkerObservation::NoConfirmedEvent
        ));
        let confirmed = driver.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(2)),
            &snapshot,
            base + Duration::from_secs(2),
        );
        let AutomationWorkerObservation::Confirmed(event) = confirmed else {
            panic!("expected confirmed transition");
        };
        assert_eq!(event.revision().get(), 1);
        assert!(event.candidate().is_none());
        assert!(matches!(
            driver.prepare_latest_dry_run(
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            Err(AutomationCoordinatorPrepareBlock::Runtime(
                crate::automation_worker_runtime::AutomationWorkerPrepareBlock::NoReadyCandidate
            ))
        ));
    }

    #[test]
    fn source_is_worker_facing_and_hardware_inert() {
        let source = include_str!("automation_worker_driver.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "performance("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["set_", "charge_limit("].concat(),
            ["Command", "::new("].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected mutation token: {needle}");
        }
        assert!(!source.contains("unsafe"));
        assert!(!source.contains("slint::"));
    }
}
