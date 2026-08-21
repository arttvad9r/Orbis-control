//! Hardware-inert Automation state machine intended for ownership by `worker`.
//!
//! The production worker already serializes application mutations and capability
//! registry replacement. Automation must eventually run under that same owner,
//! not on a second task. This module composes lifecycle observation, planning,
//! preflight, revision, revalidation, serialization and execution-scope layers
//! without performing hardware I/O itself.

use std::time::{Duration, SystemTime};

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::AutomationPolicy;
use orbis_core::lifecycle::{ResumeGateOutcome, ResumeTelemetryGate};
use orbis_core::telemetry::Telemetry;

use crate::automation_execution_scope::{
    AutomationExecutionScopeBlock, AutomationPreparedBatch, prepare_automation_execution_scope,
};
use crate::automation_lifecycle_revision::{
    AutomationLifecycleClock, AutomationLifecycleClockError, AutomationLifecycleRevision,
    AutomationRevisionCandidate, AutomationRevisionGuardBlock, AutomationRevisionGuardOutcome,
    revalidate_revision_candidate,
};
use crate::automation_serialization::{
    AutomationAdmissionBlock, AutomationAdmissionOutcome, AutomationDryRunLease,
    AutomationSerializationCoordinator,
};
use crate::automation_shadow_runtime::{AutomationShadowOutcome, AutomationShadowRuntime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationConfirmedEvent {
    revision: AutomationLifecycleRevision,
    outcome: AutomationShadowOutcome,
    candidate: Option<AutomationRevisionCandidate>,
}

impl AutomationConfirmedEvent {
    pub fn revision(&self) -> AutomationLifecycleRevision {
        self.revision
    }

    pub fn outcome(&self) -> &AutomationShadowOutcome {
        &self.outcome
    }

    pub fn candidate(&self) -> Option<&AutomationRevisionCandidate> {
        self.candidate.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationWorkerObservationError {
    LifecycleRevision(AutomationLifecycleClockError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationWorkerPrepareBlock {
    NoReadyCandidate,
    Revalidation(AutomationRevisionGuardBlock),
    Admission(AutomationAdmissionBlock),
    ExecutionScope(AutomationExecutionScopeBlock),
}

#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerPrepared {
    lease: AutomationDryRunLease,
    batch: AutomationPreparedBatch,
}

impl AutomationWorkerPrepared {
    pub fn lease(&self) -> &AutomationDryRunLease {
        &self.lease
    }

    pub fn batch(&self) -> &AutomationPreparedBatch {
        &self.batch
    }
}

/// Single-owner Automation state intended to live inside the production worker.
///
/// Deliberately not `Clone`: duplicating this value would duplicate the
/// serialization owner and lifecycle identity state.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerRuntime {
    shadow: AutomationShadowRuntime,
    resume: ResumeTelemetryGate,
    lifecycle: AutomationLifecycleClock,
    latest_candidate: Option<AutomationRevisionCandidate>,
    serialization: AutomationSerializationCoordinator,
}

impl AutomationWorkerRuntime {
    pub fn new() -> Self {
        Self {
            shadow: AutomationShadowRuntime::default(),
            resume: ResumeTelemetryGate::default(),
            lifecycle: AutomationLifecycleClock::new(),
            latest_candidate: None,
            serialization: AutomationSerializationCoordinator::new(),
        }
    }

    pub fn current_revision(&self) -> AutomationLifecycleRevision {
        self.lifecycle.current()
    }

    pub fn is_busy(&self) -> bool {
        self.serialization.is_busy()
    }

    pub fn latest_candidate(&self) -> Option<&AutomationRevisionCandidate> {
        self.latest_candidate.as_ref()
    }

    /// Consume one logind-style `PrepareForSleep(bool)` observation.
    ///
    /// Entering suspend is a telemetry sampling discontinuity. Any in-progress
    /// two-sample AC/Battery candidate is discarded before the resume gate is
    /// armed, while the last proven stable source remains intact. The signal
    /// itself never advances lifecycle revision.
    pub fn observe_prepare_for_sleep(
        &mut self,
        start: bool,
        observed_at: SystemTime,
    ) -> ResumeGateOutcome {
        if start {
            self.shadow.break_power_source_candidate();
        }
        self.resume.observe_prepare_for_sleep(start, observed_at)
    }

    /// Consume one authoritative telemetry snapshot under the worker owner.
    ///
    /// A paired resume has precedence only when the persisted policy actually
    /// enables OnResume. Otherwise the wake closes the resume gate but the same
    /// telemetry remains eligible for normal AC/Battery debounce.
    pub fn observe_telemetry(
        &mut self,
        telemetry: &Telemetry,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> Result<Option<AutomationConfirmedEvent>, AutomationWorkerObservationError> {
        let power_outcome = self
            .shadow
            .observe_telemetry(telemetry, policy, capabilities, now);
        let resume_gate = self
            .resume
            .observe_telemetry(telemetry.ac_online, telemetry.ts, now);

        let resume_enabled = policy.enabled && policy.on_resume;
        let selected = if resume_enabled && matches!(resume_gate, ResumeGateOutcome::Ready { .. }) {
            self.shadow
                .observe_resume_telemetry(telemetry, policy, capabilities, now)
        } else {
            power_outcome
        };

        if matches!(selected, AutomationShadowOutcome::Observation(_)) {
            return Ok(None);
        }

        let revision = self
            .lifecycle
            .advance()
            .map_err(AutomationWorkerObservationError::LifecycleRevision)?;
        let candidate = AutomationRevisionCandidate::from_shadow(&selected, revision);

        self.latest_candidate = candidate.clone();

        Ok(Some(AutomationConfirmedEvent {
            revision,
            outcome: selected,
            candidate,
        }))
    }

    /// Revalidate and admit the newest ready event into the single dry-run slot.
    /// Busy admission preserves the newest candidate for retry. Successful
    /// admission consumes it so one lifecycle event cannot be replayed.
    pub fn prepare_latest(
        &mut self,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
        max_capability_age: Duration,
    ) -> Result<AutomationWorkerPrepared, AutomationWorkerPrepareBlock> {
        let candidate = self
            .latest_candidate
            .as_ref()
            .ok_or(AutomationWorkerPrepareBlock::NoReadyCandidate)?;

        let handoff = match revalidate_revision_candidate(
            candidate,
            self.lifecycle.current(),
            policy,
            capabilities,
            now,
            max_capability_age,
        ) {
            AutomationRevisionGuardOutcome::Blocked(block) => {
                return Err(AutomationWorkerPrepareBlock::Revalidation(block));
            }
            AutomationRevisionGuardOutcome::Ready(handoff) => handoff,
        };

        let lease = match self.serialization.admit(
            handoff,
            self.lifecycle.current(),
            capabilities.generation(),
        ) {
            AutomationAdmissionOutcome::Blocked(block) => {
                return Err(AutomationWorkerPrepareBlock::Admission(block));
            }
            AutomationAdmissionOutcome::Admitted(lease) => {
                self.latest_candidate = None;
                lease
            }
        };

        match prepare_automation_execution_scope(&lease) {
            Ok(batch) => Ok(AutomationWorkerPrepared { lease, batch }),
            Err(block) => {
                let released = self.serialization.finish(lease);
                debug_assert!(
                    released,
                    "freshly admitted lease must own serialization slot"
                );
                Err(AutomationWorkerPrepareBlock::ExecutionScope(block))
            }
        }
    }

    pub fn finish(&mut self, prepared: AutomationWorkerPrepared) -> bool {
        self.serialization.finish(prepared.lease)
    }
}

impl Default for AutomationWorkerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_capabilities::CapabilityRegistryBuilder;
    use orbis_config::DesiredPerformancePolicy;
    use orbis_core::automation::{AutomationAction, AutomationTrigger};
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

    fn confirm_battery_event(
        runtime: &mut AutomationWorkerRuntime,
        policy: &AutomationPolicy,
        snapshot: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> AutomationConfirmedEvent {
        assert!(
            runtime
                .observe_telemetry(&telemetry(true, base), policy, snapshot, base)
                .unwrap()
                .is_none()
        );
        assert!(
            runtime
                .observe_telemetry(
                    &telemetry(false, base + Duration::from_secs(1)),
                    policy,
                    snapshot,
                    base + Duration::from_secs(1),
                )
                .unwrap()
                .is_none()
        );
        runtime
            .observe_telemetry(
                &telemetry(false, base + Duration::from_secs(2)),
                policy,
                snapshot,
                base + Duration::from_secs(2),
            )
            .unwrap()
            .expect("confirmed battery event")
    }

    #[test]
    fn confirmed_event_gets_revision_and_ready_candidate() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(7, base);
        let mut runtime = AutomationWorkerRuntime::new();
        let event = confirm_battery_event(&mut runtime, &policy, &snapshot, base);
        assert_eq!(event.revision().get(), 1);
        assert_eq!(
            event.candidate().unwrap().trigger(),
            &AutomationTrigger::OnBattery
        );
    }

    #[test]
    fn suspend_breaks_pre_sleep_candidate_continuity_when_resume_trigger_is_disabled() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        assert!(!policy.on_resume);
        let snapshot = snapshot(8, base);
        let mut runtime = AutomationWorkerRuntime::new();
        assert!(
            runtime
                .observe_telemetry(&telemetry(true, base), &policy, &snapshot, base)
                .unwrap()
                .is_none()
        );
        assert!(
            runtime
                .observe_telemetry(
                    &telemetry(false, base + Duration::from_secs(1)),
                    &policy,
                    &snapshot,
                    base + Duration::from_secs(1),
                )
                .unwrap()
                .is_none()
        );

        runtime.observe_prepare_for_sleep(true, base + Duration::from_secs(2));
        runtime.observe_prepare_for_sleep(false, base + Duration::from_secs(3));

        assert!(
            runtime
                .observe_telemetry(
                    &telemetry(false, base + Duration::from_secs(3)),
                    &policy,
                    &snapshot,
                    base + Duration::from_secs(3),
                )
                .unwrap()
                .is_none()
        );
        let event = runtime
            .observe_telemetry(
                &telemetry(false, base + Duration::from_secs(4)),
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
            )
            .unwrap()
            .expect("fresh post-resume battery edge");
        assert_eq!(event.revision().get(), 1);
        assert_eq!(
            event.candidate().unwrap().trigger(),
            &AutomationTrigger::OnBattery
        );
    }

    #[test]
    fn latest_candidate_prepares_once_and_is_consumed() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(9, base);
        let mut runtime = AutomationWorkerRuntime::new();
        let event = confirm_battery_event(&mut runtime, &policy, &snapshot, base);

        let prepared = runtime
            .prepare_latest(
                &policy,
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            )
            .expect("prepared dry-run");
        assert!(runtime.latest_candidate().is_none());
        assert_eq!(prepared.lease().required_revision(), event.revision());
        assert_eq!(prepared.lease().required_generation(), 9);
        assert_eq!(
            prepared.lease().actions(),
            &[AutomationAction::SetProfile(PerformanceProfile::Balanced)]
        );
        assert!(runtime.finish(prepared));
        assert_eq!(
            runtime.prepare_latest(
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
                Duration::from_secs(30),
            ),
            Err(AutomationWorkerPrepareBlock::NoReadyCandidate)
        );
    }

    #[test]
    fn newer_blocked_event_clears_older_ready_candidate() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut policy = policy();
        let snapshot = snapshot(11, base);
        let mut runtime = AutomationWorkerRuntime::new();
        assert!(
            confirm_battery_event(&mut runtime, &policy, &snapshot, base)
                .candidate()
                .is_some()
        );

        policy.on_ac_change = false;
        assert!(
            runtime
                .observe_telemetry(
                    &telemetry(true, base + Duration::from_secs(3)),
                    &policy,
                    &snapshot,
                    base + Duration::from_secs(3),
                )
                .unwrap()
                .is_none()
        );
        let second = runtime
            .observe_telemetry(
                &telemetry(true, base + Duration::from_secs(4)),
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
            )
            .unwrap()
            .expect("confirmed blocked AC event");
        assert_eq!(second.revision().get(), 2);
        assert!(second.candidate().is_none());
        assert!(runtime.latest_candidate().is_none());
    }

    #[test]
    fn prepared_old_lease_is_revision_stale_after_new_event() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let policy = policy();
        let snapshot = snapshot(13, base);
        let mut runtime = AutomationWorkerRuntime::new();
        confirm_battery_event(&mut runtime, &policy, &snapshot, base);
        let prepared = runtime
            .prepare_latest(
                &policy,
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            )
            .unwrap();
        let old_revision = prepared.lease().required_revision();

        runtime
            .observe_telemetry(
                &telemetry(true, base + Duration::from_secs(4)),
                &policy,
                &snapshot,
                base + Duration::from_secs(4),
            )
            .unwrap();
        let newer = runtime
            .observe_telemetry(
                &telemetry(true, base + Duration::from_secs(5)),
                &policy,
                &snapshot,
                base + Duration::from_secs(5),
            )
            .unwrap()
            .expect("new AC event");
        assert_ne!(old_revision, newer.revision());
        assert!(runtime.is_busy());
        assert!(runtime.finish(prepared));
    }

    #[test]
    fn source_is_hardware_inert() {
        let source = include_str!("automation_worker_runtime.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "performance("].concat(),
            ["set_", "profile("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
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
