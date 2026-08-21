//! Hardware-inert Automation runtime observation.
//!
//! This module connects typed lifecycle evidence, persisted policy and one
//! immutable capability generation without crossing the mutation boundary.
//! A fully dry-run-ready plan is reported only as `ReadyButExecutionDisabled`;
//! production execution must still pass the strict Automation write gate later.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::{
    AutomationPolicy, AutomationPowerSource, AutomationPreflight,
    preflight_automation_plan_for_dry_run,
};
use orbis_core::automation::{
    AutomationTrigger, PowerSourceEdgeDetector, PowerSourceObservationOutcome,
};
use orbis_core::telemetry::Telemetry;

const RESUME_TELEMETRY_MAX_AGE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationShadowBlock {
    CapabilitySnapshotStale,
    CapabilitySnapshotFromFuture,
    ResumePowerSourceUnknown,
    ResumeTelemetryStale,
    ResumeTelemetryFromFuture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationShadowOutcome {
    Observation(PowerSourceObservationOutcome),
    Blocked {
        trigger: AutomationTrigger,
        generation: u64,
        block: AutomationShadowBlock,
    },
    PreflightBlocked {
        trigger: AutomationTrigger,
        generation: u64,
        preflight: AutomationPreflight,
    },
    ReadyButExecutionDisabled {
        trigger: AutomationTrigger,
        generation: u64,
        preflight: AutomationPreflight,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationShadowRuntime {
    power_source: PowerSourceEdgeDetector,
    max_capability_age: Duration,
}

impl AutomationShadowRuntime {
    pub fn new(power_source: PowerSourceEdgeDetector, max_capability_age: Duration) -> Self {
        Self {
            power_source,
            max_capability_age,
        }
    }

    /// Break an in-progress AC/Battery debounce sequence at a sampling
    /// discontinuity such as suspend while retaining the last proven stable
    /// power source. This emits no lifecycle event.
    pub fn break_power_source_candidate(&mut self) {
        self.power_source.break_candidate_continuity();
    }

    pub fn observe_telemetry(
        &mut self,
        telemetry: &Telemetry,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> AutomationShadowOutcome {
        let observation = self
            .power_source
            .observe(telemetry.ac_online, telemetry.ts, now);
        let trigger = match observation {
            PowerSourceObservationOutcome::Trigger(ref trigger) => trigger.clone(),
            _ => return AutomationShadowOutcome::Observation(observation),
        };

        let source = match &trigger {
            AutomationTrigger::OnAc => AutomationPowerSource::Ac,
            AutomationTrigger::OnBattery => AutomationPowerSource::Battery,
            _ => {
                return AutomationShadowOutcome::Observation(
                    PowerSourceObservationOutcome::IgnoredUnknown,
                );
            }
        };

        self.evaluate_trigger(trigger, source, policy, capabilities, now)
    }

    /// Evaluate one already-paired resume using a fresh authoritative
    /// post-resume telemetry snapshot. The resume signal itself never plans.
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

        // If OnResume owns reconciliation, the accepted post-resume source is
        // also the new AC/Battery baseline so the same physical sleeping change
        // cannot be emitted again on the next telemetry poll.
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
            preflight_automation_plan_for_dry_run(plan, capabilities.device_capabilities());

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
        Self::new(PowerSourceEdgeDetector::default(), Duration::from_secs(300))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::{DesiredPerformancePolicy, preflight_automation_plan};
    use orbis_core::automation::AutomationAction;
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;

    fn capability(write: CapabilityStatus, constraints: CapabilityConstraints) -> Capability {
        Capability::new(if write == CapabilityStatus::Supported {
            CapabilityStatus::Supported
        } else {
            CapabilityStatus::ReadOnly
        })
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

    fn policy(on_resume: bool) -> AutomationPolicy {
        let mut policy = AutomationPolicy {
            enabled: true,
            on_ac_change: true,
            on_resume,
            ..Default::default()
        };
        policy.ac.performance = DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced);
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
    fn first_sample_only_establishes_baseline() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(1, now, CapabilityStatus::Supported);
        let mut runtime = AutomationShadowRuntime::default();
        assert_eq!(
            runtime.observe_telemetry(&telemetry(Some(true), now), &policy(false), &snapshot, now),
            AutomationShadowOutcome::Observation(
                PowerSourceObservationOutcome::BaselineEstablished
            )
        );
    }

    #[test]
    fn confirmed_transition_stops_at_execution_disabled_boundary() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(2, base, CapabilityStatus::Supported);
        let policy = policy(false);
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
        let AutomationShadowOutcome::ReadyButExecutionDisabled { preflight, .. } = outcome else {
            panic!("expected ready-but-disabled outcome");
        };
        assert_eq!(
            preflight.actions_if_ready(),
            Some(&[AutomationAction::SetProfile(PerformanceProfile::Balanced)][..])
        );
    }

    #[test]
    fn read_only_runtime_can_be_dry_run_ready_but_strict_execution_stays_blocked() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(20, base, CapabilityStatus::Unsupported);
        let policy = policy(false);
        let plan = policy.plan_for(AutomationTrigger::OnBattery, AutomationPowerSource::Battery);
        assert!(!preflight_automation_plan(plan, snapshot.device_capabilities()).is_ready());

        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);
        runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(1)),
            &policy,
            &snapshot,
            base + Duration::from_secs(1),
        );
        assert!(matches!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(2)),
                &policy,
                &snapshot,
                base + Duration::from_secs(2),
            ),
            AutomationShadowOutcome::ReadyButExecutionDisabled {
                trigger: AutomationTrigger::OnBattery,
                ..
            }
        ));
    }

    #[test]
    fn suspend_breaks_candidate_continuity_without_losing_stable_source() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(3, base, CapabilityStatus::Supported);
        let policy = policy(false);
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

        runtime.break_power_source_candidate();

        assert_eq!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(2)),
                &policy,
                &snapshot,
                base + Duration::from_secs(2),
            ),
            AutomationShadowOutcome::Observation(PowerSourceObservationOutcome::Candidate)
        );
        assert!(matches!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(3)),
                &policy,
                &snapshot,
                base + Duration::from_secs(3),
            ),
            AutomationShadowOutcome::ReadyButExecutionDisabled {
                trigger: AutomationTrigger::OnBattery,
                ..
            }
        ));
    }

    #[test]
    fn enabled_resume_rebaselines_sleeping_power_change() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(4, base, CapabilityStatus::Supported);
        let policy = policy(true);
        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);
        runtime.break_power_source_candidate();

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
    fn disabled_resume_does_not_consume_sleeping_power_change() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(5, base, CapabilityStatus::Supported);
        let policy = policy(false);
        let mut runtime = AutomationShadowRuntime::default();
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &snapshot, base);
        runtime.break_power_source_candidate();

        assert!(matches!(
            runtime.observe_resume_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(1)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
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
    }

    #[test]
    fn resume_rejects_unknown_stale_and_future_telemetry() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(6, base, CapabilityStatus::Supported);
        let policy = policy(true);
        let mut runtime = AutomationShadowRuntime::default();

        assert!(matches!(
            runtime.observe_resume_telemetry(
                &telemetry(None, base + Duration::from_secs(1)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationShadowOutcome::Blocked {
                block: AutomationShadowBlock::ResumePowerSourceUnknown,
                ..
            }
        ));
        assert!(matches!(
            runtime.observe_resume_telemetry(
                &telemetry(Some(true), base),
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
            ),
            AutomationShadowOutcome::Blocked {
                block: AutomationShadowBlock::ResumeTelemetryStale,
                ..
            }
        ));
        assert!(matches!(
            runtime.observe_resume_telemetry(
                &telemetry(Some(true), base + Duration::from_secs(2)),
                &policy,
                &snapshot,
                base + Duration::from_secs(1),
            ),
            AutomationShadowOutcome::Blocked {
                block: AutomationShadowBlock::ResumeTelemetryFromFuture,
                ..
            }
        ));
    }

    #[test]
    fn stale_capabilities_block_handoff() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let stale = snapshot(7, base, CapabilityStatus::Supported);
        let mut runtime = AutomationShadowRuntime::new(
            PowerSourceEdgeDetector::default(),
            Duration::from_secs(1),
        );
        let policy = policy(false);
        runtime.observe_telemetry(&telemetry(Some(true), base), &policy, &stale, base);
        runtime.observe_telemetry(
            &telemetry(Some(false), base + Duration::from_secs(1)),
            &policy,
            &stale,
            base + Duration::from_secs(1),
        );
        assert!(matches!(
            runtime.observe_telemetry(
                &telemetry(Some(false), base + Duration::from_secs(2)),
                &policy,
                &stale,
                base + Duration::from_secs(2),
            ),
            AutomationShadowOutcome::Blocked {
                block: AutomationShadowBlock::CapabilitySnapshotStale,
                ..
            }
        ));
    }
}
