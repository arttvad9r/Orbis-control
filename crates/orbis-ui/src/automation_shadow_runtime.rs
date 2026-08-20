//! Hardware-inert Automation runtime observation.
//!
//! This module connects already-typed pieces without crossing the mutation
//! boundary:
//! - debounced authoritative AC/Battery observations from `Telemetry`;
//! - matched resume lifecycle observations that are paired with fresh
//!   post-resume telemetry by a higher-layer gate;
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

const RESUME_TELEMETRY_MAX_AGE: Duration = Duration::from_secs(3);

/// Why a confirmed lifecycle transition cannot proceed even to an executor
/// handoff candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationShadowBlock {
    /// Capability metadata is older than the configured maximum age.
    CapabilitySnapshotStale,
    /// Capability metadata appears to come from the future relative to the
    /// caller-supplied clock and therefore cannot be trusted for execution.
    CapabilitySnapshotFromFuture,
    /// A resume was reported, but the post-resume telemetry sample did not carry
    /// an authoritative AC/Battery source needed to choose the policy branch.
    ResumePowerSourceUnknown,
    /// A resume was reported, but the supplied telemetry sample was older than
    /// the shadow runtime's resume freshness ceiling.
    ResumeTelemetryStale,
    /// A resume was reported, but the telemetry timestamp was in the future
    /// relative to the caller-supplied clock.
    ResumeTelemetryFromFuture,
}

/// Result of consuming one lifecycle observation in the hardware-inert shadow
/// runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationShadowOutcome {
    /// No confirmed AC/Battery transition was produced. This includes baseline,
    /// stable, candidate, unknown and stale telemetry outcomes.
    Observation(PowerSourceObservationOutcome),
    /// A transition was proven, but freshness/evidence failed before
    /// planning/preflight could be considered executable.
    Blocked {
        /// Confirmed lifecycle trigger.
        trigger: AutomationTrigger,
        /// Capability generation used for the decision.
        generation: u64,
        /// Freshness/evidence blocker.
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

    /// Consume one authoritative telemetry sample for AC/Battery transition
    /// detection.
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

        let source = match &trigger {
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

        self.evaluate_trigger(trigger, source, policy, capabilities, now)
    }

    /// Evaluate a previously matched resume cycle using one fresh authoritative
    /// post-resume telemetry sample.
    ///
    /// The higher layer must pair `PrepareForSleep(true/false)` before calling
    /// this function. The shadow runtime independently re-checks telemetry
    /// freshness and requires an explicit AC/Battery observation before choosing
    /// the resume policy branch. No action is executed here.
    ///
    /// When persisted Automation is enabled and `on_resume` is configured, the
    /// accepted post-resume power source also becomes the edge detector's new
    /// baseline. This coalesces a source change that happened during sleep into
    /// `OnResume` instead of emitting the same state again as `OnAc`/`OnBattery`
    /// one poll later. When `on_resume` is disabled, no rebaseline occurs and the
    /// normal AC/Battery trigger remains eligible to detect the sleeping change.
    pub fn observe_resume_telemetry(
        &mut self,
        telemetry: &Telemetry,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> AutomationShadowOutcome {
        let trigger = AutomationTrigger::OnResume;
        let generation = capabilities.generation();

        let ac_online = match telemetry.ac_online {
            Some(value) => value,
            None => {
                return AutomationShadowOutcome::Blocked {
                    trigger,
                    generation,
                    block: AutomationShadowBlock::ResumePowerSourceUnknown,
                };
            }
        };
        let source = if ac_online {
            AutomationPowerSource::Ac
        } else {
            AutomationPowerSource::Battery
        };

        match now.duration_since(telemetry.ts) {
            Err(_) => {
                return AutomationShadowOutcome::Blocked {
                    trigger,
                    generation,
                    block: AutomationShadowBlock::ResumeTelemetryFromFuture,
                };
            }
            Ok(age) if age > RESUME_TELEMETRY_MAX_AGE => {
                return AutomationShadowOutcome::Blocked {
                    trigger,
                    generation,
                    block: AutomationShadowBlock::ResumeTelemetryStale,
                };
            }
            Ok(_) => {}
        }

        if policy.enabled && policy.on_resume {
            self.power_source.rebaseline(ac_online);
        }

        self.evaluate_trigger(trigger, source, policy, capabilities, now)
    }

    fn evaluate_trigger(
        &self,
        trigger: AutomationTrigger,
        source: AutomationPowerSource,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> AutomationShadowOutcome {
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

    fn resume_policy() -> AutomationPolicy {
        let mut policy = enabled_policy();
        policy.on_resume = true;
        policy.ac.performance =
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
    fn matched_resume_uses_fresh_post_resume_power_source_and_stays_execution_disabled() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(10, base);
        let policy = resume_policy();
        let mut runtime = AutomationShadowRuntime::default();

        let outcome = runtime.observe_resume_telemetry(
            &telemetry(Some(true), base + Duration::from_secs(1)),
            &policy,
            &snapshot,
            base + Duration::from_secs(1),
        );
        let AutomationShadowOutcome::ReadyButExecutionDisabled {
            trigger,
            generation,
            preflight,
        } = outcome
        else {
            panic!("expected ready-but-disabled resume outcome");
        };
        assert_eq!(trigger, AutomationTrigger::OnResume);
        assert_eq!(generation, 10);
        assert_eq!(
            preflight.actions_if_ready(),
            Some(&[AutomationAction::SetProfile(PerformanceProfile::Balanced)][..])
        );
    }

    #[test]
    fn enabled_resume_rebaselines_sleeping_power_change_and_avoids_duplicate_edge() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(14, base);
        let policy = resume_policy();
        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);

        // First post-resume Battery sample would normally be the first edge
        // candidate. The configured OnResume path accepts the same authoritative
        // source and consumes it as the new baseline.
        assert_eq!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(1)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationShadowOutcome::Observation(PowerSourceObservationOutcome::Candidate)
        );
        let resume = runtime.observe_resume_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(1)),
            &policy,
            &snapshot,
            base + Duration::from_secs(1),
        );
        assert!(matches!(
            resume,
            AutomationShadowOutcome::ReadyButExecutionDisabled {
                trigger: AutomationTrigger::OnResume,
                ..
            }
        ));
        assert_eq!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(2)),
                &policy,
                &snapshot,
                base + Duration::from_secs(2),
            ),
            AutomationShadowOutcome::Observation(PowerSourceObservationOutcome::Stable)
        );
    }

    #[test]
    fn disabled_resume_does_not_consume_ac_battery_change() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(15, base);
        let policy = enabled_policy();
        assert!(!policy.on_resume);
        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);

        let resume = runtime.observe_resume_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(1)),
            &policy,
            &snapshot,
            base + Duration::from_secs(1),
        );
        assert!(matches!(
            resume,
            AutomationShadowOutcome::PreflightBlocked {
                trigger: AutomationTrigger::OnResume,
                ..
            }
        ));
        assert_eq!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(1)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationShadowOutcome::Observation(PowerSourceObservationOutcome::Candidate)
        );
        let power = runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(2)),
            &policy,
            &snapshot,
            base + Duration::from_secs(2),
        );
        assert!(matches!(
            power,
            AutomationShadowOutcome::ReadyButExecutionDisabled {
                trigger: AutomationTrigger::OnBattery,
                ..
            }
        ));
    }

    #[test]
    fn resume_never_guesses_power_source_or_accepts_stale_telemetry() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = ready_snapshot(12, base);
        let policy = resume_policy();
        let mut runtime = AutomationShadowRuntime::default();

        assert_eq!(
            runtime.observe_resume_telemetry(
                &telemetry(None, base + Duration::from_secs(1)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationShadowOutcome::Blocked {
                trigger: AutomationTrigger::OnResume,
                generation: 12,
                block: AutomationShadowBlock::ResumePowerSourceUnknown,
            }
        );
        assert_eq!(
            runtime.observe_resume_telemetry(
                &telemetry(Some(false), base),
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
            ),
            AutomationShadowOutcome::Blocked {
                trigger: AutomationTrigger::OnResume,
                generation: 12,
                block: AutomationShadowBlock::ResumeTelemetryStale,
            }
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
