//! Hardware-inert Automation runtime observation.
//!
//! This module connects three already-typed pieces without crossing the
//! mutation boundary:
//! - debounced authoritative power-source observations from `Telemetry`;
//! - persisted Automation policy planning;
//! - immutable capability preflight.
//!
//! It deliberately has no provider/worker mutation API. A fully ready plan is
//! reported as `ReadyButExecutionDisabled`; a future executor must be a separate
//! component with its own generation/freshness guard and authoritative read-back.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::{
    AutomationPolicy, AutomationPowerSource, AutomationPreflight, preflight_automation_plan,
};
use orbis_core::automation::{
    AutomationTrigger, PowerSourceEdgeDetector, PowerSourceObservationOutcome,
};
use orbis_core::telemetry::Telemetry;

/// Why a confirmed lifecycle transition cannot proceed even to an executor
/// handoff candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationShadowBlock {
    /// Capability metadata is older than the configured maximum age.
    CapabilitySnapshotStale,
    /// Capability metadata appears to come from the future relative to the
    /// caller-supplied clock and therefore cannot be trusted for execution.
    CapabilitySnapshotFromFuture,
}

/// Result of consuming one telemetry sample in the hardware-inert shadow
/// runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationShadowOutcome {
    /// No confirmed transition was produced. This includes baseline, stable,
    /// candidate, unknown and stale telemetry outcomes.
    Observation(PowerSourceObservationOutcome),
    /// A transition was proven, but capability snapshot freshness failed before
    /// planning/preflight could be considered executable.
    Blocked {
        /// Confirmed lifecycle trigger.
        trigger: AutomationTrigger,
        /// Capability generation used for the decision.
        generation: u64,
        /// Freshness blocker.
        block: AutomationShadowBlock,
    },
    /// A transition was proven and planned, but typed plan/capability preflight
    /// found one or more blockers.
    PreflightBlocked {
        /// Confirmed lifecycle trigger.
        trigger: AutomationTrigger,
        /// Capability generation used for preflight.
        generation: u64,
        /// Complete fail-closed preflight result.
        preflight: AutomationPreflight,
    },
    /// The policy and one fresh immutable capability generation passed pure
    /// preflight. No action is executed by this module.
    ReadyButExecutionDisabled {
        /// Confirmed lifecycle trigger.
        trigger: AutomationTrigger,
        /// Capability generation that passed preflight.
        generation: u64,
        /// Ready preflight result. `actions_if_ready()` is non-None here.
        preflight: AutomationPreflight,
    },
}

/// Stateful power-source observer plus pure plan/preflight bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationShadowRuntime {
    power_source: PowerSourceEdgeDetector,
    max_capability_age: Duration,
}

impl AutomationShadowRuntime {
    /// Create a shadow runtime with explicit edge detector and capability
    /// freshness policy.
    pub fn new(power_source: PowerSourceEdgeDetector, max_capability_age: Duration) -> Self {
        Self {
            power_source,
            max_capability_age,
        }
    }

    /// Consume one authoritative telemetry sample.
    ///
    /// `now` is supplied by the caller so freshness behavior remains
    /// deterministic in tests. The capability snapshot is immutable and one
    /// generation is used for the entire planning/preflight decision.
    pub fn observe_telemetry(
        &mut self,
        telemetry: &Telemetry,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> AutomationShadowOutcome {
        let observation =
            self.power_source
                .observe(telemetry.ac_online, telemetry.ts, now);
        let trigger = match observation {
            PowerSourceObservationOutcome::Trigger(ref trigger) => trigger.clone(),
            _ => return AutomationShadowOutcome::Observation(observation),
        };

        let generation = capabilities.generation();
        match now.duration_since(capabilities.checked_at()) {
            Err(_) => {
                return AutomationShadowOutcome::Blocked {
                    trigger,
                    generation,
                    block: AutomationShadowBlock::CapabilitySnapshotFromFuture,
                };
            }
            Ok(age) if age > self.max_capability_age => {
                return AutomationShadowOutcome::Blocked {
                    trigger,
                    generation,
                    block: AutomationShadowBlock::CapabilitySnapshotStale,
                };
            }
            Ok(_) => {}
        }

        let source = match trigger {
            AutomationTrigger::OnAc => AutomationPowerSource::Ac,
            AutomationTrigger::OnBattery => AutomationPowerSource::Battery,
            // `PowerSourceEdgeDetector` can only emit the two variants above.
            // Keep this fallback fail-closed if its contract ever expands.
            _ => {
                return AutomationShadowOutcome::Observation(
                    PowerSourceObservationOutcome::IgnoredUnknown,
                );
            }
        };

        let plan = policy.plan_for(trigger.clone(), source);
        let preflight =
            preflight_automation_plan(plan, capabilities.device_capabilities());

        if preflight.is_ready() {
            AutomationShadowOutcome::ReadyButExecutionDisabled {
                trigger,
                generation,
                preflight,
            }
        } else {
            AutomationShadowOutcome::PreflightBlocked {
                trigger,
                generation,
                preflight,
            }
        }
    }
}

impl Default for AutomationShadowRuntime {
    fn default() -> Self {
        // Capability refresh is much slower-moving than 1s telemetry. Five
        // minutes is intentionally conservative for shadow observation; a
        // future executor must re-check the exact generation immediately before
        // mutation rather than relying on this age alone.
        Self::new(PowerSourceEdgeDetector::default(), Duration::from_secs(300))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::DesiredPerformancePolicy;
    use orbis_core::automation::AutomationAction;
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;

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

    fn ready_snapshot(generation: u64, checked_at: SystemTime) -> CapabilityRegistrySnapshot {
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

    fn enabled_policy() -> AutomationPolicy {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.battery.performance =
            DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced);
        policy
    }

    fn telemetry(ac_online: Option<bool>, ts: SystemTime) -> Telemetry {
        let mut telemetry = Telemetry::empty();
        telemetry.ac_online = ac_online;
        telemetry.ts = ts;
        telemetry
    }

    #[test]
    fn first_sample_never_plans_or_preflights() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(7, now);
        let mut runtime = AutomationShadowRuntime::default();
        assert_eq!(
            runtime.observe_telemetry(&telemetry(Some(true), now), &enabled_policy(), &snapshot, now),
            AutomationShadowOutcome::Observation(
                PowerSourceObservationOutcome::BaselineEstablished
            )
        );
    }

    #[test]
    fn confirmed_fresh_transition_can_only_reach_execution_disabled_boundary() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(9, base);
        let policy = enabled_policy();
        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);
        assert_eq!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(1)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationShadowOutcome::Observation(PowerSourceObservationOutcome::Candidate)
        );

        let outcome = runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(2)),
            &policy,
            &snapshot,
            base + Duration::from_secs(2),
        );
        let AutomationShadowOutcome::ReadyButExecutionDisabled {
            trigger,
            generation,
            preflight,
        } = outcome
        else {
            panic!("expected ready-but-disabled shadow outcome");
        };
        assert_eq!(trigger, AutomationTrigger::OnBattery);
        assert_eq!(generation, 9);
        assert_eq!(
            preflight.actions_if_ready(),
            Some(&[AutomationAction::SetProfile(PerformanceProfile::Balanced)][..])
        );
    }

    #[test]
    fn stale_capability_generation_blocks_before_preflight_handoff() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(11, base);
        let policy = enabled_policy();
        let mut runtime = AutomationShadowRuntime::new(
            PowerSourceEdgeDetector::default(),
            Duration::from_secs(2),
        );
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);
        runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(3)),
            &policy,
            &snapshot,
            base + Duration::from_secs(3),
        );
        let outcome = runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(4)),
            &policy,
            &snapshot,
            base + Duration::from_secs(4),
        );
        assert_eq!(
            outcome,
            AutomationShadowOutcome::Blocked {
                trigger: AutomationTrigger::OnBattery,
                generation: 11,
                block: AutomationShadowBlock::CapabilitySnapshotStale,
            }
        );
    }

    #[test]
    fn unsupported_runtime_write_stays_preflight_blocked() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut builder = CapabilityRegistryBuilder::new(13, base);
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
        let snapshot = builder.build().unwrap();
        let policy = enabled_policy();
        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);
        runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(1)),
            &policy,
            &snapshot,
            base + Duration::from_secs(1),
        );
        let outcome = runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(2)),
            &policy,
            &snapshot,
            base + Duration::from_secs(2),
        );
        assert!(matches!(
            outcome,
            AutomationShadowOutcome::PreflightBlocked {
                trigger: AutomationTrigger::OnBattery,
                generation: 13,
                ..
            }
        ));
    }
}
