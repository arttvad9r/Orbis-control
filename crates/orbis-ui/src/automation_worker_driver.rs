//! Worker-facing hardware-inert Automation driver.
//!
//! This is the narrow adapter intended for eventual ownership by `worker.rs`.
//! It keeps the last successfully loaded persisted Automation policy beside the
//! recovery-aware [`AutomationWorkerCoordinator`] and accepts only typed
//! lifecycle/telemetry/capability inputs. It performs no provider mutation and
//! does not know about Slint.
//!
//! Persisted policy is reloaded only through an explicit call. Unsaved UI draft
//! state can therefore never enter the worker lifecycle by accident. Every
//! authoritative policy replacement also advances an independent monotonic
//! policy revision so a prepared batch cannot survive a later Save/reload merely
//! because lifecycle revision and capability generation stayed unchanged.

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

/// Monotonic identity of the worker's authoritative persisted Automation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AutomationPolicyRevision(u64);

impl AutomationPolicyRevision {
    /// No authoritative policy has been accepted by this driver yet.
    pub const INITIAL: Self = Self(0);

    /// Raw monotonic value for diagnostics/audit correlation.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Failure to allocate another persisted-policy revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationPolicyRevisionError {
    /// Revision space is exhausted. The driver remains fail-closed rather than
    /// wrapping and making an ancient prepared batch current again.
    SequenceExhausted,
}

/// Result of explicitly reloading the persisted Automation policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPolicyReloadOutcome {
    /// Hardened desired-state loading succeeded and replaced the worker cache.
    Loaded {
        /// Whether the persisted policy is globally enabled.
        enabled: bool,
        /// New authoritative policy identity.
        revision: AutomationPolicyRevision,
    },
    /// The persisted source could not be treated as authoritative user intent.
    /// The previous cached policy is discarded rather than silently retained.
    Unavailable {
        /// Diagnostic-only error text. Execution remains unavailable.
        detail: String,
        /// Revision that identifies the transition to unavailable state.
        revision: AutomationPolicyRevision,
    },
    /// Policy revision space is exhausted. Cached intent is cleared permanently
    /// for the lifetime of this driver.
    RevisionExhausted,
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

/// Why the worker-facing dry-run boundary cannot produce a prepared envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationWorkerDriverPrepareBlock {
    /// No authoritative persisted policy is currently available.
    PolicyUnavailable,
    /// Policy revision ownership has exhausted and is permanently fail-closed.
    PolicyRevisionExhausted,
    /// Lower lifecycle/preflight/serialization/recovery preparation blocker.
    Coordinator(AutomationCoordinatorPrepareBlock),
}

/// Move-only prepared batch plus the exact persisted-policy identity that
/// authorized its preparation.
///
/// A future production executor must compare `required_policy_revision()` to the
/// driver's current revision immediately before its first mutation, alongside
/// lifecycle revision and capability generation. This wrapper exposes no
/// execution method.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerPreparedEnvelope {
    required_policy_revision: AutomationPolicyRevision,
    prepared: AutomationWorkerPrepared,
}

impl AutomationWorkerPreparedEnvelope {
    /// Persisted-policy identity that must still be current before mutation.
    pub fn required_policy_revision(&self) -> AutomationPolicyRevision {
        self.required_policy_revision
    }

    /// Explicit final policy-identity comparison for a future worker executor.
    pub fn policy_revision_still_matches(&self, current: AutomationPolicyRevision) -> bool {
        self.required_policy_revision == current
    }

    /// Borrow the lower-level lifecycle/capability prepared batch metadata.
    pub fn prepared(&self) -> &AutomationWorkerPrepared {
        &self.prepared
    }
}

/// Unique owner for persisted Automation intent plus lifecycle/recovery state.
///
/// Deliberately not `Clone`: duplicating this value would split cached policy,
/// policy revision, lifecycle revision, serialization and recovery histories.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerDriver {
    coordinator: AutomationWorkerCoordinator,
    persisted_policy: Option<AutomationPolicy>,
    policy_revision: AutomationPolicyRevision,
    policy_revision_exhausted: bool,
}

impl AutomationWorkerDriver {
    /// Construct an inert driver with no assumed persisted policy.
    pub fn new() -> Self {
        Self {
            coordinator: AutomationWorkerCoordinator::new(),
            persisted_policy: None,
            policy_revision: AutomationPolicyRevision::INITIAL,
            policy_revision_exhausted: false,
        }
    }

    /// Construct with an already-authoritative policy.
    ///
    /// Primarily useful for tests and for a future startup path that performs
    /// hardened loading before entering the worker loop. The first authoritative
    /// policy receives revision 1; revision zero always means "none accepted".
    pub fn with_policy(policy: AutomationPolicy) -> Self {
        Self {
            coordinator: AutomationWorkerCoordinator::new(),
            persisted_policy: Some(policy),
            policy_revision: AutomationPolicyRevision(1),
            policy_revision_exhausted: false,
        }
    }

    fn advance_policy_revision(
        &mut self,
    ) -> Result<AutomationPolicyRevision, AutomationPolicyRevisionError> {
        if self.policy_revision_exhausted {
            return Err(AutomationPolicyRevisionError::SequenceExhausted);
        }
        let Some(next) = self.policy_revision.0.checked_add(1) else {
            self.policy_revision_exhausted = true;
            self.persisted_policy = None;
            return Err(AutomationPolicyRevisionError::SequenceExhausted);
        };
        self.policy_revision = AutomationPolicyRevision(next);
        Ok(self.policy_revision)
    }

    /// Explicitly reload `desired-state.toml` through the shared hardened loader.
    ///
    /// Every attempt that can change authoritative worker intent advances policy
    /// revision. Failure clears the cached policy. Retaining an older policy
    /// after the persisted source became malformed would make disk intent and
    /// unattended runtime intent diverge silently.
    pub fn reload_persisted_policy(&mut self) -> AutomationPolicyReloadOutcome {
        let loaded = load_automation_policy();
        let revision = match self.advance_policy_revision() {
            Ok(revision) => revision,
            Err(AutomationPolicyRevisionError::SequenceExhausted) => {
                self.persisted_policy = None;
                return AutomationPolicyReloadOutcome::RevisionExhausted;
            }
        };

        match loaded {
            Ok(policy) => {
                let enabled = policy.enabled;
                self.persisted_policy = Some(policy);
                AutomationPolicyReloadOutcome::Loaded { enabled, revision }
            }
            Err(error) => {
                self.persisted_policy = None;
                AutomationPolicyReloadOutcome::Unavailable {
                    detail: error.to_string(),
                    revision,
                }
            }
        }
    }

    /// Replace the cached policy from an already-authoritative read-back.
    ///
    /// This method does not accept a UI draft type; callers must supply the same
    /// typed [`AutomationPolicy`] produced by hardened persistence/read-back.
    pub fn replace_persisted_policy(
        &mut self,
        policy: AutomationPolicy,
    ) -> Result<AutomationPolicyRevision, AutomationPolicyRevisionError> {
        let revision = self.advance_policy_revision()?;
        self.persisted_policy = Some(policy);
        Ok(revision)
    }

    /// Clear persisted intent explicitly, for example after a hardened reload
    /// reports a preserved/malformed source. Clearing is itself an authoritative
    /// state transition and invalidates every older prepared envelope.
    pub fn clear_persisted_policy(
        &mut self,
    ) -> Result<AutomationPolicyRevision, AutomationPolicyRevisionError> {
        let revision = self.advance_policy_revision()?;
        self.persisted_policy = None;
        Ok(revision)
    }

    /// Borrow the worker's authoritative cached policy.
    pub fn persisted_policy(&self) -> Option<&AutomationPolicy> {
        self.persisted_policy.as_ref()
    }

    /// Current persisted-policy revision.
    pub fn current_policy_revision(&self) -> AutomationPolicyRevision {
        self.policy_revision
    }

    /// Whether policy revision ownership is permanently exhausted.
    pub fn policy_revision_exhausted(&self) -> bool {
        self.policy_revision_exhausted
    }

    /// Current confirmed lifecycle revision.
    pub fn current_revision(&self) -> AutomationLifecycleRevision {
        self.coordinator.current_revision()
    }

    /// Whether serialization or unknown-outcome recovery blocks preparation.
    pub fn is_blocked(&self) -> bool {
        self.coordinator.is_blocked() || self.policy_revision_exhausted
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
        if self.policy_revision_exhausted {
            return AutomationWorkerObservation::PolicyUnavailable;
        }
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
    /// The returned move-only envelope captures the current persisted-policy
    /// revision in addition to the lower lifecycle/capability identities. It
    /// still performs no mutation.
    pub fn prepare_latest_dry_run(
        &mut self,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
        max_capability_age: Duration,
    ) -> Result<AutomationWorkerPreparedEnvelope, AutomationWorkerDriverPrepareBlock> {
        if self.policy_revision_exhausted {
            return Err(AutomationWorkerDriverPrepareBlock::PolicyRevisionExhausted);
        }
        let Some(policy) = self.persisted_policy.as_ref() else {
            return Err(AutomationWorkerDriverPrepareBlock::PolicyUnavailable);
        };
        if self.policy_revision == AutomationPolicyRevision::INITIAL {
            return Err(AutomationWorkerDriverPrepareBlock::PolicyUnavailable);
        }

        let prepared = self
            .coordinator
            .prepare_latest(policy, capabilities, now, max_capability_age)
            .map_err(AutomationWorkerDriverPrepareBlock::Coordinator)?;
        Ok(AutomationWorkerPreparedEnvelope {
            required_policy_revision: self.policy_revision,
            prepared,
        })
    }

    /// Release one hardware-inert prepared batch with a known/no-execution
    /// outcome. No success state is synthesized here.
    pub fn finish_dry_run(
        &mut self,
        envelope: AutomationWorkerPreparedEnvelope,
    ) -> Result<(), AutomationCoordinatorFinishError> {
        self.coordinator.finish_known(envelope.prepared)
    }

    /// Enter typed unknown-outcome recovery for a Performance envelope whose
    /// future mutation may have occurred but read-back could not establish state.
    /// This method itself performs no mutation.
    pub fn finish_performance_unknown(
        &mut self,
        envelope: AutomationWorkerPreparedEnvelope,
    ) -> Result<(), AutomationCoordinatorFinishError> {
        self.coordinator
            .finish_performance_unknown(envelope.prepared)
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

    fn snapshot(
        generation: u64,
        checked_at: SystemTime,
        automation_write: CapabilityStatus,
    ) -> CapabilityRegistrySnapshot {
        let mut builder = CapabilityRegistryBuilder::new(generation, checked_at);
        builder
            .add(
                FeatureId::Automation,
                capability(automation_write, CapabilityConstraints::None),
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

    fn confirm_battery(
        driver: &mut AutomationWorkerDriver,
        snapshot: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> AutomationConfirmedEvent {
        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), snapshot, base),
            AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationWorkerObservation::NoConfirmedEvent
        ));
        let outcome = driver.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(2)),
            snapshot,
            base + Duration::from_secs(2),
        );
        let AutomationWorkerObservation::Confirmed(event) = outcome else {
            panic!("expected confirmed transition");
        };
        event
    }

    #[test]
    fn no_policy_never_advances_lifecycle_or_policy_revision() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(7, base, CapabilityStatus::Unsupported);
        let mut driver = AutomationWorkerDriver::new();
        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), &snapshot, base),
            AutomationWorkerObservation::PolicyUnavailable
        ));
        assert_eq!(driver.current_revision(), AutomationLifecycleRevision::INITIAL);
        assert_eq!(
            driver.current_policy_revision(),
            AutomationPolicyRevision::INITIAL
        );
    }

    #[test]
    fn read_only_runtime_can_observe_event_but_never_produce_ready_candidate() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(9, base, CapabilityStatus::Unsupported);
        let mut driver = AutomationWorkerDriver::with_policy(policy());
        let event = confirm_battery(&mut driver, &snapshot, base);
        assert_eq!(event.revision().get(), 1);
        assert!(event.candidate().is_none());
        assert_eq!(driver.current_policy_revision().get(), 1);
        assert!(matches!(
            driver.prepare_latest_dry_run(
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            Err(AutomationWorkerDriverPrepareBlock::Coordinator(
                AutomationCoordinatorPrepareBlock::Runtime(
                    crate::automation_worker_runtime::AutomationWorkerPrepareBlock::NoReadyCandidate
                )
            ))
        ));
    }

    #[test]
    fn prepared_envelope_is_invalidated_by_authoritative_policy_replacement() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(11, base, CapabilityStatus::Supported);
        let policy = policy();
        let mut driver = AutomationWorkerDriver::with_policy(policy.clone());
        confirm_battery(&mut driver, &snapshot, base);

        let envelope = driver
            .prepare_latest_dry_run(
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            )
            .expect("prepared dry-run envelope");
        assert_eq!(envelope.required_policy_revision().get(), 1);
        assert!(envelope.policy_revision_still_matches(driver.current_policy_revision()));

        let replacement_revision = driver
            .replace_persisted_policy(policy)
            .expect("policy revision advances");
        assert_eq!(replacement_revision.get(), 2);
        assert!(!envelope.policy_revision_still_matches(driver.current_policy_revision()));

        // Policy replacement invalidates future execution identity but does not
        // forge/cancel the lower serialization lease. The owner must still
        // release/abort that exact move-only envelope.
        driver.finish_dry_run(envelope).unwrap();
        assert!(!driver.is_blocked());
    }

    #[test]
    fn clear_policy_advances_revision_and_blocks_new_observation() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(13, base, CapabilityStatus::Unsupported);
        let mut driver = AutomationWorkerDriver::with_policy(policy());
        assert_eq!(driver.current_policy_revision().get(), 1);
        assert_eq!(driver.clear_persisted_policy().unwrap().get(), 2);
        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), &snapshot, base),
            AutomationWorkerObservation::PolicyUnavailable
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
        assert!(source.contains("required_policy_revision"));
        assert!(source.contains("policy_revision_still_matches"));
    }
}
