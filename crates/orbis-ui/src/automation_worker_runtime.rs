//! Hardware-inert Automation state machine intended for ownership by `worker`.
//!
//! The production worker already serializes application mutations and capability
//! registry replacement. Automation must eventually run under that same owner,
//! not on a second task. This module composes the existing pure lifecycle,
//! planning, preflight, revision, revalidation, serialization and execution-scope
//! layers without performing hardware I/O itself.
//!
//! No method in this module executes a prepared batch.

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

/// One confirmed lifecycle event after shadow evaluation and revision binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationConfirmedEvent {
    revision: AutomationLifecycleRevision,
    outcome: AutomationShadowOutcome,
    candidate: Option<AutomationRevisionCandidate>,
}

impl AutomationConfirmedEvent {
    /// Monotonic lifecycle identity assigned to this event.
    pub fn revision(&self) -> AutomationLifecycleRevision {
        self.revision
    }

    /// Full shadow result for audit/status publication.
    pub fn outcome(&self) -> &AutomationShadowOutcome {
        &self.outcome
    }

    /// Ready execution candidate, only when shadow preflight succeeded.
    pub fn candidate(&self) -> Option<&AutomationRevisionCandidate> {
        self.candidate.as_ref()
    }
}

/// Failure to record a confirmed event in the monotonic lifecycle domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationWorkerObservationError {
    /// Lifecycle revision space was exhausted. The runtime must remain
    /// fail-closed rather than wrap to an old revision.
    LifecycleRevision(AutomationLifecycleClockError),
}

/// Why the worker-owned dry-run path cannot produce a prepared batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationWorkerPrepareBlock {
    /// No current shadow-ready event exists. A blocked/newer event clears any
    /// older candidate by design.
    NoReadyCandidate,
    /// Lifecycle/policy/capability revalidation failed.
    Revalidation(AutomationRevisionGuardBlock),
    /// Single-slot admission failed.
    Admission(AutomationAdmissionBlock),
    /// The batch contains an action outside the proven Performance-only scope.
    ExecutionScope(AutomationExecutionScopeBlock),
}

/// Exclusive dry-run preparation owned by the future worker executor path.
///
/// This object contains only a serialization lease and scoped metadata. It has
/// no execute/apply method and must be consumed by [`AutomationWorkerRuntime::finish`]
/// after a future executor completes or aborts.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerPrepared {
    lease: AutomationDryRunLease,
    batch: AutomationPreparedBatch,
}

impl AutomationWorkerPrepared {
    /// Serialization lease; future executor proof uses this for identity checks.
    pub fn lease(&self) -> &AutomationDryRunLease {
        &self.lease
    }

    /// Performance-only/no-op scoped batch metadata.
    pub fn batch(&self) -> &AutomationPreparedBatch {
        &self.batch
    }
}

/// Worker-owned hardware-inert Automation state.
///
/// Deliberately not `Clone`: duplicating this value would duplicate the
/// serialization owner and could make two copies believe they own the same
/// lifecycle/capability state.
#[derive(Debug, PartialEq, Eq)]
pub struct AutomationWorkerRuntime {
    shadow: AutomationShadowRuntime,
    resume: ResumeTelemetryGate,
    lifecycle: AutomationLifecycleClock,
    latest_candidate: Option<AutomationRevisionCandidate>,
    serialization: AutomationSerializationCoordinator,
}

impl AutomationWorkerRuntime {
    /// Construct an inert worker runtime before any lifecycle event.
    pub fn new() -> Self {
        Self {
            shadow: AutomationShadowRuntime::default(),
            resume: ResumeTelemetryGate::default(),
            lifecycle: AutomationLifecycleClock::new(),
            latest_candidate: None,
            serialization: AutomationSerializationCoordinator::new(),
        }
    }

    /// Current lifecycle revision. Zero means no confirmed event has occurred.
    pub fn current_revision(&self) -> AutomationLifecycleRevision {
        self.lifecycle.current()
    }

    /// Whether a dry-run lease currently owns the one serialization slot.
    pub fn is_busy(&self) -> bool {
        self.serialization.is_busy()
    }

    /// Last shadow-ready candidate. Any newer confirmed blocked event clears it,
    /// and successful serialization admission consumes it to prevent replay of
    /// the same lifecycle event.
    pub fn latest_candidate(&self) -> Option<&AutomationRevisionCandidate> {
        self.latest_candidate.as_ref()
    }

    /// Consume one logind-style `PrepareForSleep(bool)` observation.
    ///
    /// This never advances lifecycle revision by itself. A paired resume must be
    /// followed by fresh post-resume telemetry before becoming a confirmed event.
    pub fn observe_prepare_for_sleep(
        &mut self,
        start: bool,
        observed_at: SystemTime,
    ) -> ResumeGateOutcome {
        self.resume.observe_prepare_for_sleep(start, observed_at)
    }

    /// Consume one authoritative telemetry snapshot under the worker owner.
    ///
    /// AC/Battery and pending resume are evaluated from the same typed snapshot.
    /// A ready resume has precedence over an AC/Battery outcome from that same
    /// sample, matching the established sleep-source coalescing contract. Only a
    /// non-observation shadow result advances lifecycle revision.
    pub fn observe_telemetry(
        &mut self,
        telemetry: &Telemetry,
        policy: &AutomationPolicy,
        capabilities: &CapabilityRegistrySnapshot,
        now: SystemTime,
    ) -> Result<Option<AutomationConfirmedEvent>, AutomationWorkerObservationError> {
        let power_outcome =
            self.shadow
                .observe_telemetry(telemetry, policy, capabilities, now);
        let resume_gate = self
            .resume
            .observe_telemetry(telemetry.ac_online, telemetry.ts, now);

        let selected = if matches!(resume_gate, ResumeGateOutcome::Ready { .. }) {
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

        // Every confirmed event supersedes the previous candidate, including a
        // newer event that is itself blocked by capability/policy evidence.
        self.latest_candidate = candidate.clone();

        Ok(Some(AutomationConfirmedEvent {
            revision,
            outcome: selected,
            candidate,
        }))
    }

    /// Revalidate and admit the latest candidate into the one dry-run slot, then
    /// restrict it to the proven Performance-only execution scope.
    ///
    /// No mutation occurs. Busy admission preserves the candidate so the newest
    /// event can be retried after the old lease is released. Successful
    /// admission consumes the candidate immediately, preventing replay of one
    /// lifecycle event after the lease is finished. Scope failure also consumes
    /// the event and releases the just-acquired lease fail-closed.
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
                debug_assert!(released, "freshly admitted lease must own serialization slot");
                Err(AutomationWorkerPrepareBlock::ExecutionScope(block))
            }
        }
    }

    /// Release the exact serialization lease after future execution/abort.
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
    use orbis_core::automation::AutomationAction;
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

    fn confirm_battery_event(
        runtime: &mut AutomationWorkerRuntime,
        policy: &AutomationPolicy,
        snapshot: &CapabilityRegistrySnapshot,
        base: SystemTime,
    ) -> AutomationConfirmedEvent {
        assert!(runtime
            .observe_telemetry(&telemetry(true, base), policy, snapshot, base)
            .unwrap()
            .is_none());
        assert!(runtime
            .observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                policy,
                snapshot,
                base + Duration::from_secs(1),
            )
            .unwrap()
            .is_none());
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
        assert!(event.candidate().is_some());
        assert_eq!(runtime.current_revision(), event.revision());
        assert_eq!(
            event.candidate().unwrap().trigger(),
            &orbis_core::automation::AutomationTrigger::OnBattery
        );
    }

    #[test]
    fn latest_ready_candidate_prepares_performance_only_dry_run_once() {
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
        assert!(runtime.is_busy());
        assert!(runtime.latest_candidate().is_none());
        assert_eq!(prepared.lease().required_revision(), event.revision());
        assert_eq!(prepared.lease().required_generation(), 9);
        assert_eq!(
            prepared.lease().actions(),
            &[AutomationAction::SetProfile(PerformanceProfile::Balanced)]
        );
        assert!(runtime.finish(prepared));
        assert!(!runtime.is_busy());
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
    fn newer_confirmed_blocked_event_clears_older_ready_candidate() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut policy = policy();
        let snapshot = snapshot(11, base);
        let mut runtime = AutomationWorkerRuntime::new();
        let first = confirm_battery_event(&mut runtime, &policy, &snapshot, base);
        assert!(first.candidate().is_some());

        policy.on_ac_change = false;
        assert!(runtime
            .observe_telemetry(
                &telemetry(true, base + Duration::from_secs(3)),
                &policy,
                &snapshot,
                base + Duration::from_secs(3),
            )
            .unwrap()
            .is_none());
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
        assert_eq!(
            runtime.prepare_latest(
                &policy,
                &snapshot,
                base + Duration::from_secs(5),
                Duration::from_secs(30),
            ),
            Err(AutomationWorkerPrepareBlock::NoReadyCandidate)
        );
    }

    #[test]
    fn prepared_old_lease_becomes_revision_stale_after_new_event() {
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
        assert_eq!(runtime.current_revision(), newer.revision());

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
            assert!(!source.contains(&needle), "unexpected execution token: {needle}");
        }
        assert!(!source.contains("unsafe"));
    }
}