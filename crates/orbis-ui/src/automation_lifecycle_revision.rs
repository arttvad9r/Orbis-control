//! Lifecycle-revision guard for Automation execution candidates.
//!
//! Capability generation and persisted policy are not sufficient to prove that
//! an Automation handoff is still current. A newer confirmed lifecycle event
//! (for example a resume arriving after an AC/Battery transition) must supersede
//! every older candidate even when policy and capability evidence are otherwise
//! unchanged. This module provides that independent monotonic revision barrier.
//!
//! It performs no I/O and exposes no mutation API.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::AutomationPolicy;
use orbis_core::automation::{AutomationAction, AutomationTrigger};

use crate::automation_execution_guard::{
    AutomationExecutionCandidate, AutomationExecutionGuardBlock, AutomationExecutionGuardOutcome,
    AutomationExecutionHandoff, revalidate_automation_candidate,
};
use crate::automation_shadow_runtime::AutomationShadowOutcome;

/// Monotonic identity of one confirmed Automation lifecycle event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AutomationLifecycleRevision(u64);

impl AutomationLifecycleRevision {
    /// Initial revision before any confirmed lifecycle event has been accepted.
    pub const INITIAL: Self = Self(0);

    /// Raw monotonic value for diagnostics/audit correlation.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Why the lifecycle revision clock cannot accept another event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationLifecycleClockError {
    /// The monotonic `u64` revision space is exhausted. The runtime must remain
    /// fail-closed instead of wrapping and making an ancient candidate current.
    SequenceExhausted,
}

/// Monotonic revision owner for confirmed lifecycle events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationLifecycleClock {
    current: AutomationLifecycleRevision,
    exhausted: bool,
}

impl AutomationLifecycleClock {
    /// Construct a clock before the first lifecycle event.
    pub fn new() -> Self {
        Self {
            current: AutomationLifecycleRevision::INITIAL,
            exhausted: false,
        }
    }

    /// Current revision. Revision zero means no event has been admitted yet.
    pub fn current(&self) -> AutomationLifecycleRevision {
        self.current
    }

    /// Advance exactly once for one confirmed lifecycle event.
    ///
    /// Once exhausted, the clock permanently refuses new revisions rather than
    /// wrapping to zero.
    pub fn advance(
        &mut self,
    ) -> Result<AutomationLifecycleRevision, AutomationLifecycleClockError> {
        if self.exhausted {
            return Err(AutomationLifecycleClockError::SequenceExhausted);
        }
        let Some(next) = self.current.0.checked_add(1) else {
            self.exhausted = true;
            return Err(AutomationLifecycleClockError::SequenceExhausted);
        };
        self.current = AutomationLifecycleRevision(next);
        Ok(self.current)
    }
}

impl Default for AutomationLifecycleClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Shadow-ready candidate bound to the lifecycle event that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRevisionCandidate {
    revision: AutomationLifecycleRevision,
    inner: AutomationExecutionCandidate,
}

impl AutomationRevisionCandidate {
    /// Bind a candidate only when the shadow outcome is explicitly ready.
    pub fn from_shadow(
        outcome: &AutomationShadowOutcome,
        revision: AutomationLifecycleRevision,
    ) -> Option<Self> {
        if revision == AutomationLifecycleRevision::INITIAL {
            return None;
        }
        AutomationExecutionCandidate::from_shadow(outcome).map(|inner| Self { revision, inner })
    }

    /// Lifecycle revision that produced this candidate.
    pub fn revision(&self) -> AutomationLifecycleRevision {
        self.revision
    }

    /// Capability generation that passed shadow preflight.
    pub fn generation(&self) -> u64 {
        self.inner.generation()
    }

    /// Lifecycle trigger represented by the candidate.
    pub fn trigger(&self) -> &AutomationTrigger {
        self.inner.trigger()
    }
}

/// Why a revision-bound candidate cannot proceed to serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationRevisionGuardBlock {
    /// A newer confirmed lifecycle event superseded this candidate.
    LifecycleRevisionChanged {
        /// Revision attached to the candidate.
        candidate: AutomationLifecycleRevision,
        /// Revision current at revalidation.
        current: AutomationLifecycleRevision,
    },
    /// One of the existing policy/capability/freshness guards blocked it.
    ExecutionGuard(AutomationExecutionGuardBlock),
}

/// Result of revision + existing execution-guard revalidation.
///
/// Deliberately not `Clone`: a ready handoff is move-only and can be consumed by
/// serialization exactly once.
#[derive(Debug, PartialEq, Eq)]
pub enum AutomationRevisionGuardOutcome {
    /// Candidate is stale or otherwise blocked.
    Blocked(AutomationRevisionGuardBlock),
    /// Candidate passed all hardware-inert guards. No hardware action has run.
    Ready(AutomationRevisionHandoff),
}

/// Revalidated handoff carrying both lifecycle and capability identities.
///
/// Deliberately not `Clone`: callers cannot retain a duplicate before
/// `AutomationSerializationCoordinator::admit()` and replay the same handoff
/// after the first lease is released.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationRevisionHandoff {
    revision: AutomationLifecycleRevision,
    inner: AutomationExecutionHandoff,
}

impl AutomationRevisionHandoff {
    /// Lifecycle revision the serialization/executor owner must still hold.
    pub fn required_revision(&self) -> AutomationLifecycleRevision {
        self.revision
    }

    /// Capability generation the serialization/executor owner must still hold.
    pub fn required_generation(&self) -> u64 {
        self.inner.required_generation()
    }

    /// Exact ordered action batch that passed both execution guards.
    pub fn actions(&self) -> &[AutomationAction] {
        self.inner.actions()
    }

    /// Lifecycle trigger for diagnostics/audit logging.
    pub fn trigger(&self) -> &AutomationTrigger {
        self.inner.trigger()
    }

    /// Final identity comparison for the future serialization owner.
    pub fn identities_still_match(
        &self,
        current_revision: AutomationLifecycleRevision,
        current_generation: u64,
    ) -> bool {
        self.revision == current_revision && self.inner.generation_still_matches(current_generation)
    }
}

/// Revalidate one revision-bound candidate against the current lifecycle
/// revision and all existing policy/capability/freshness guards.
///
/// Lifecycle identity is checked first. A superseded event cannot consume work
/// rebuilding plans or enter serialization even if every other input is equal.
pub fn revalidate_revision_candidate(
    candidate: &AutomationRevisionCandidate,
    current_revision: AutomationLifecycleRevision,
    current_policy: &AutomationPolicy,
    current_capabilities: &CapabilityRegistrySnapshot,
    now: SystemTime,
    max_capability_age: Duration,
) -> AutomationRevisionGuardOutcome {
    if candidate.revision != current_revision {
        return AutomationRevisionGuardOutcome::Blocked(
            AutomationRevisionGuardBlock::LifecycleRevisionChanged {
                candidate: candidate.revision,
                current: current_revision,
            },
        );
    }

    match revalidate_automation_candidate(
        &candidate.inner,
        current_policy,
        current_capabilities,
        now,
        max_capability_age,
    ) {
        AutomationExecutionGuardOutcome::Blocked(block) => AutomationRevisionGuardOutcome::Blocked(
            AutomationRevisionGuardBlock::ExecutionGuard(block),
        ),
        AutomationExecutionGuardOutcome::Ready(inner) => {
            AutomationRevisionGuardOutcome::Ready(AutomationRevisionHandoff {
                revision: current_revision,
                inner,
            })
        }
    }
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
        let mut policy = AutomationPolicy {
            enabled: true,
            on_ac_change: true,
            ..Default::default()
        };
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

    fn ready_outcome(
        policy: &AutomationPolicy,
        snapshot: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> AutomationShadowOutcome {
        let mut shadow = AutomationShadowRuntime::default();
        shadow.observe_telemetry(&telemetry(true, base), policy, snapshot, base);
        shadow.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(1)),
            policy,
            snapshot,
            base + Duration::from_secs(1),
        );
        shadow.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(2)),
            policy,
            snapshot,
            base + Duration::from_secs(2),
        )
    }

    #[test]
    fn clock_is_monotonic_and_never_uses_zero_for_an_event() {
        let mut clock = AutomationLifecycleClock::new();
        assert_eq!(clock.current(), AutomationLifecycleRevision::INITIAL);
        assert_eq!(clock.advance().unwrap().get(), 1);
        assert_eq!(clock.advance().unwrap().get(), 2);
    }

    #[test]
    fn newer_lifecycle_event_supersedes_candidate_before_base_guard() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(7, base);
        let outcome = ready_outcome(&policy, &snapshot, base);
        let mut clock = AutomationLifecycleClock::new();
        let candidate_revision = clock.advance().unwrap();
        let candidate = AutomationRevisionCandidate::from_shadow(&outcome, candidate_revision)
            .expect("revision-bound candidate");
        let newer = clock.advance().unwrap();

        assert_eq!(
            revalidate_revision_candidate(
                &candidate,
                newer,
                &policy,
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            AutomationRevisionGuardOutcome::Blocked(
                AutomationRevisionGuardBlock::LifecycleRevisionChanged {
                    candidate: candidate_revision,
                    current: newer,
                }
            )
        );
    }

    #[test]
    fn matching_revision_and_generation_produce_only_hardware_inert_handoff() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(9, base);
        let outcome = ready_outcome(&policy, &snapshot, base);
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
            panic!("expected revision handoff");
        };
        assert_eq!(handoff.required_revision(), revision);
        assert_eq!(handoff.required_generation(), 9);
        assert!(handoff.identities_still_match(revision, 9));
        assert!(!handoff.identities_still_match(revision, 10));
        assert_eq!(handoff.trigger(), &AutomationTrigger::OnBattery);
        assert_eq!(
            handoff.actions(),
            &[AutomationAction::SetProfile(PerformanceProfile::Balanced)]
        );
    }

    #[test]
    fn initial_revision_cannot_be_bound_to_a_candidate() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(11, base);
        let outcome = ready_outcome(&policy, &snapshot, base);
        assert!(
            AutomationRevisionCandidate::from_shadow(
                &outcome,
                AutomationLifecycleRevision::INITIAL,
            )
            .is_none()
        );
    }

    #[test]
    fn source_contains_no_execution_surface() {
        let source = include_str!("automation_lifecycle_revision.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "profile("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["set_", "charge_limit("].concat(),
            ["Command", "::new"].concat(),
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "unexpected execution token: {needle}"
            );
        }
        assert!(!source.contains(&["un", "safe"].concat()));
    }
}
