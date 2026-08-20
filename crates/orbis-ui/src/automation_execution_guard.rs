//! Fail-closed revalidation boundary between Automation shadow planning and a
//! future serialized executor.
//!
//! This module deliberately performs no hardware I/O. A shadow plan that looked
//! ready is treated only as a candidate. Before a future executor may consider
//! it, the candidate is rebuilt from the current persisted policy and checked
//! against the current immutable capability generation again.
//!
//! Even a successful [`AutomationExecutionHandoff`] is not authority to write on
//! its own: the future executor must own serialization with capability refresh
//! and verify `required_generation()` immediately before the first mutation.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::{
    AutomationPlan, AutomationPolicy, AutomationPreflight, preflight_automation_plan,
};
use orbis_core::automation::{AutomationAction, AutomationTrigger};

use crate::automation_shadow_runtime::{AutomationShadowOutcome, AutomationShadowRuntime};

/// Snapshot of one shadow-ready plan that still requires revalidation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationExecutionCandidate {
    plan: AutomationPlan,
    generation: u64,
}

impl AutomationExecutionCandidate {
    /// Extract a candidate only from the shadow runtime's explicit
    /// `ReadyButExecutionDisabled` boundary.
    pub fn from_shadow(outcome: &AutomationShadowOutcome) -> Option<Self> {
        match outcome {
            AutomationShadowOutcome::ReadyButExecutionDisabled {
                generation,
                preflight,
                ..
            } => Some(Self {
                plan: preflight.plan.clone(),
                generation: *generation,
            }),
            _ => None,
        }
    }

    /// Capability generation that originally passed shadow preflight.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Trigger represented by the candidate.
    pub fn trigger(&self) -> &AutomationTrigger {
        &self.plan.trigger
    }
}

/// Why a shadow-ready candidate is no longer eligible for executor handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationExecutionGuardBlock {
    /// Capability registry changed after shadow preflight.
    CapabilityGenerationChanged {
        /// Generation observed by shadow preflight.
        candidate: u64,
        /// Generation current at revalidation.
        current: u64,
    },
    /// The current capability snapshot is too old for an execution handoff.
    CapabilitySnapshotStale,
    /// The current capability timestamp is in the future relative to the
    /// supplied clock and cannot be trusted.
    CapabilitySnapshotFromFuture,
    /// Persisted policy no longer produces exactly the same plan.
    PolicyChanged,
    /// Current operation-level capability evidence no longer passes preflight.
    PreflightBlocked(AutomationPreflight),
}

/// Result of revalidating one shadow candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationExecutionGuardOutcome {
    /// Handoff remains blocked.
    Blocked(AutomationExecutionGuardBlock),
    /// Candidate passed pure revalidation. No hardware action has run.
    Ready(AutomationExecutionHandoff),
}

/// Revalidated, still hardware-inert executor handoff.
///
/// Fields are private so callers cannot forge a handoff without passing
/// [`revalidate_automation_candidate`]. This object intentionally exposes no
/// execute/apply/provider/worker method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationExecutionHandoff {
    generation: u64,
    preflight: AutomationPreflight,
}

impl AutomationExecutionHandoff {
    /// Capability generation the future serialized executor must still own at
    /// the instant it begins mutation.
    pub fn required_generation(&self) -> u64 {
        self.generation
    }

    /// Exact ordered action batch that passed the second preflight.
    pub fn actions(&self) -> &[AutomationAction] {
        self.preflight
            .actions_if_ready()
            .expect("execution handoff is constructed only from a ready preflight")
    }

    /// Desired lifecycle trigger for diagnostics/audit logging.
    pub fn trigger(&self) -> &AutomationTrigger {
        &self.preflight.plan.trigger
    }

    /// Explicit final generation comparison for a future executor that owns the
    /// registry serialization boundary.
    pub fn generation_still_matches(&self, current_generation: u64) -> bool {
        self.generation == current_generation
    }
}

/// Revalidate a shadow-ready Automation candidate against current policy and
/// current immutable capability evidence.
///
/// This function is pure and hardware-inert. It intentionally rebuilds the plan
/// instead of trusting the candidate's earlier copy. Any change to policy,
/// generation, freshness or operation-level capability status blocks the whole
/// batch; there is no partial handoff.
pub fn revalidate_automation_candidate(
    candidate: &AutomationExecutionCandidate,
    current_policy: &AutomationPolicy,
    current_capabilities: &CapabilityRegistrySnapshot,
    now: SystemTime,
    max_capability_age: Duration,
) -> AutomationExecutionGuardOutcome {
    let current_generation = current_capabilities.generation();
    if current_generation != candidate.generation {
        return AutomationExecutionGuardOutcome::Blocked(
            AutomationExecutionGuardBlock::CapabilityGenerationChanged {
                candidate: candidate.generation,
                current: current_generation,
            },
        );
    }

    match now.duration_since(current_capabilities.checked_at()) {
        Err(_) => {
            return AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::CapabilitySnapshotFromFuture,
            );
        }
        Ok(age) if age > max_capability_age => {
            return AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::CapabilitySnapshotStale,
            );
        }
        Ok(_) => {}
    }

    let rebuilt = current_policy.plan_for(
        candidate.plan.trigger.clone(),
        candidate.plan.power_source,
    );
    if rebuilt != candidate.plan {
        return AutomationExecutionGuardOutcome::Blocked(
            AutomationExecutionGuardBlock::PolicyChanged,
        );
    }

    let preflight =
        preflight_automation_plan(rebuilt, current_capabilities.device_capabilities());
    if !preflight.is_ready() {
        return AutomationExecutionGuardOutcome::Blocked(
            AutomationExecutionGuardBlock::PreflightBlocked(preflight),
        );
    }

    AutomationExecutionGuardOutcome::Ready(AutomationExecutionHandoff {
        generation: current_generation,
        preflight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::{AutomationPowerSource, DesiredPerformancePolicy};
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;
    use orbis_core::telemetry::Telemetry;

    fn capability(
        write: CapabilityStatus,
        constraints: CapabilityConstraints,
    ) -> Capability {
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
        performance_write: CapabilityStatus,
    ) -> CapabilityRegistrySnapshot {
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
                    performance_write,
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

    fn ready_candidate(
        policy: &AutomationPolicy,
        snapshot: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> AutomationExecutionCandidate {
        let mut shadow = AutomationShadowRuntime::default();
        shadow.observe_telemetry(&telemetry(true, base), policy, snapshot, base);
        shadow.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(1)),
            policy,
            snapshot,
            base + Duration::from_secs(1),
        );
        let outcome = shadow.observe_telemetry(
            &telemetry(false, base + Duration::from_secs(2)),
            policy,
            snapshot,
            base + Duration::from_secs(2),
        );
        AutomationExecutionCandidate::from_shadow(&outcome).expect("shadow candidate")
    }

    #[test]
    fn unchanged_policy_and_generation_produce_only_hardware_inert_handoff() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(7, base, CapabilityStatus::Supported);
        let candidate = ready_candidate(&policy, &snapshot, base);

        let outcome = revalidate_automation_candidate(
            &candidate,
            &policy,
            &snapshot,
            base + Duration::from_secs(3),
            Duration::from_secs(30),
        );
        let AutomationExecutionGuardOutcome::Ready(handoff) = outcome else {
            panic!("expected ready handoff");
        };
        assert_eq!(handoff.required_generation(), 7);
        assert!(handoff.generation_still_matches(7));
        assert!(!handoff.generation_still_matches(8));
        assert_eq!(handoff.trigger(), &AutomationTrigger::OnBattery);
        assert_eq!(
            handoff.actions(),
            &[AutomationAction::SetProfile(PerformanceProfile::Balanced)]
        );
    }

    #[test]
    fn generation_change_blocks_whole_batch_before_second_preflight() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let old = snapshot(7, base, CapabilityStatus::Supported);
        let candidate = ready_candidate(&policy, &old, base);
        let current = snapshot(8, base + Duration::from_secs(2), CapabilityStatus::Supported);

        assert_eq!(
            revalidate_automation_candidate(
                &candidate,
                &policy,
                &current,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::CapabilityGenerationChanged {
                    candidate: 7,
                    current: 8,
                }
            )
        );
    }

    #[test]
    fn policy_change_blocks_even_when_capability_generation_is_unchanged() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let original = policy();
        let snapshot = snapshot(9, base, CapabilityStatus::Supported);
        let candidate = ready_candidate(&original, &snapshot, base);
        let mut changed = original;
        changed.battery.performance = DesiredPerformancePolicy::KeepCurrent;

        assert_eq!(
            revalidate_automation_candidate(
                &candidate,
                &changed,
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::PolicyChanged
            )
        );
    }

    #[test]
    fn write_evidence_regression_blocks_second_preflight() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let ready = snapshot(11, base, CapabilityStatus::Supported);
        let candidate = ready_candidate(&policy, &ready, base);

        // Same generation is deliberately used to model a contract violation in
        // a caller. Even then, the second preflight detects regressed operation
        // evidence and refuses the batch.
        let blocked = snapshot(11, base, CapabilityStatus::Unsupported);
        assert!(matches!(
            revalidate_automation_candidate(
                &candidate,
                &policy,
                &blocked,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            ),
            AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::PreflightBlocked(_)
            )
        ));
    }

    #[test]
    fn stale_or_future_capability_metadata_never_produces_handoff() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let ready = snapshot(13, base, CapabilityStatus::Supported);
        let candidate = ready_candidate(&policy, &ready, base);

        assert_eq!(
            revalidate_automation_candidate(
                &candidate,
                &policy,
                &ready,
                base + Duration::from_secs(40),
                Duration::from_secs(30),
            ),
            AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::CapabilitySnapshotStale
            )
        );

        let future = snapshot(13, base + Duration::from_secs(10), CapabilityStatus::Supported);
        assert_eq!(
            revalidate_automation_candidate(
                &candidate,
                &policy,
                &future,
                base + Duration::from_secs(5),
                Duration::from_secs(30),
            ),
            AutomationExecutionGuardOutcome::Blocked(
                AutomationExecutionGuardBlock::CapabilitySnapshotFromFuture
            )
        );
    }

    #[test]
    fn guard_source_has_no_execution_surface() {
        let source = include_str!("automation_execution_guard.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "profile("].concat(),
            ["set_", "mode("].concat(),
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
