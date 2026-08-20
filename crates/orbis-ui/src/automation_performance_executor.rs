//! Test-only proof of the first Automation mutation executor.
//!
//! This module is compiled only for tests from `lib.rs`. It exercises the
//! intended Performance-only execution semantics against the already-existing
//! application owner contract without making Automation reachable in production.
//! Promotion to production requires executable Rust tests plus wiring under the
//! worker/lifecycle/capability serialization owner.

use async_trait::async_trait;
use orbis_application::{
    CommandError, PerformanceCommandOutcome, SetPerformanceError,
};
use orbis_core::action::ApplyResult;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;

use crate::automation_execution_scope::{AutomationPreparedBatch, AutomationPreparedKind};
use crate::automation_lifecycle_revision::AutomationLifecycleRevision;
use crate::automation_serialization::AutomationDryRunLease;
use crate::composition::PerformanceServiceRuntime;

/// Fail-closed error emitted by the proof executor.
#[derive(Debug)]
enum AutomationPerformanceExecutionError {
    /// Prepared metadata does not belong to the supplied serialization lease.
    LeaseMismatch,
    /// A newer lifecycle event superseded the lease before the owner call.
    LifecycleRevisionChanged {
        required: AutomationLifecycleRevision,
        current: AutomationLifecycleRevision,
    },
    /// Capability generation changed before the mutation call.
    CapabilityGenerationChanged { required: u64, current: u64 },
    /// Provider mutation failed before a successful command result existed.
    Command(ProviderError),
    /// Provider mutation returned a result, but mandatory authoritative
    /// read-back failed. Mutation outcome is therefore unknown for automation.
    ReadBackAfterMutation {
        result: ApplyResult,
        source: ProviderError,
    },
    /// Application returned a non-Applied result. Unattended automation does
    /// not accept config-only Accepted or Pending/Failed/RolledBack outcomes.
    UnexpectedApplyResult {
        result: ApplyResult,
        observed: PerformanceProfile,
    },
    /// Authoritative read-back completed but did not match the requested target.
    ReadBackMismatch {
        requested: PerformanceProfile,
        observed: PerformanceProfile,
        result: ApplyResult,
    },
}

/// Result of the Performance-only proof executor.
#[derive(Debug)]
enum AutomationPerformanceExecutionOutcome {
    /// No mutation was required.
    NoOp,
    /// Performance mutation was confirmed by `ApplyResult::Applied` and exact
    /// authoritative read-back.
    Applied {
        lease_id: u64,
        revision: AutomationLifecycleRevision,
        generation: u64,
        profile: PerformanceProfile,
    },
    /// Execution was refused or failed without claiming success.
    Failed(AutomationPerformanceExecutionError),
}

#[async_trait]
trait PerformanceAutomationOwner: Send + Sync {
    async fn set_performance_for_automation(
        &self,
        profile: PerformanceProfile,
    ) -> Result<PerformanceCommandOutcome, SetPerformanceError>;
}

#[async_trait]
impl<T> PerformanceAutomationOwner for T
where
    T: PerformanceServiceRuntime + Send + Sync + ?Sized,
{
    async fn set_performance_for_automation(
        &self,
        profile: PerformanceProfile,
    ) -> Result<PerformanceCommandOutcome, SetPerformanceError> {
        self.set_performance(profile).await
    }
}

/// Execute one already-scoped Performance-only batch in the proof environment.
///
/// The caller must still own the serialization lease. Lifecycle revision and
/// capability generation are compared immediately before the owner call. This
/// proof does not solve the remaining production TOCTOU requirement: lifecycle
/// admission, capability refresh and this call must later be serialized by the
/// same worker/runtime owner.
async fn run_performance_proof<O>(
    owner: &O,
    lease: &AutomationDryRunLease,
    prepared: &AutomationPreparedBatch,
    current_revision: AutomationLifecycleRevision,
    current_generation: u64,
) -> AutomationPerformanceExecutionOutcome
where
    O: PerformanceAutomationOwner + ?Sized,
{
    if prepared.lease_id() != lease.id()
        || prepared.required_revision() != lease.required_revision()
        || prepared.required_generation() != lease.required_generation()
        || prepared.trigger() != lease.trigger()
    {
        return AutomationPerformanceExecutionOutcome::Failed(
            AutomationPerformanceExecutionError::LeaseMismatch,
        );
    }

    let required_revision = lease.required_revision();
    if current_revision != required_revision {
        return AutomationPerformanceExecutionOutcome::Failed(
            AutomationPerformanceExecutionError::LifecycleRevisionChanged {
                required: required_revision,
                current: current_revision,
            },
        );
    }

    let required_generation = lease.required_generation();
    if current_generation != required_generation {
        return AutomationPerformanceExecutionOutcome::Failed(
            AutomationPerformanceExecutionError::CapabilityGenerationChanged {
                required: required_generation,
                current: current_generation,
            },
        );
    }

    let profile = match prepared.kind() {
        AutomationPreparedKind::NoOp => return AutomationPerformanceExecutionOutcome::NoOp,
        AutomationPreparedKind::Performance(profile) => *profile,
    };

    let outcome = match owner.set_performance_for_automation(profile).await {
        Ok(outcome) => outcome,
        Err(CommandError::Command(source)) => {
            return AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::Command(source),
            );
        }
        Err(CommandError::ReadBack { result, source }) => {
            return AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::ReadBackAfterMutation { result, source },
            );
        }
    };

    if !outcome.result.is_applied() {
        return AutomationPerformanceExecutionOutcome::Failed(
            AutomationPerformanceExecutionError::UnexpectedApplyResult {
                result: outcome.result,
                observed: outcome.state.current,
            },
        );
    }

    if outcome.state.current != profile {
        return AutomationPerformanceExecutionOutcome::Failed(
            AutomationPerformanceExecutionError::ReadBackMismatch {
                requested: profile,
                observed: outcome.state.current,
                result: outcome.result,
            },
        );
    }

    AutomationPerformanceExecutionOutcome::Applied {
        lease_id: lease.id(),
        revision: required_revision,
        generation: required_generation,
        profile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    use orbis_capabilities::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot};
    use orbis_config::{AutomationPolicy, DesiredPerformancePolicy};
    use orbis_core::automation::AutomationAction;
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };

    use crate::automation_execution_scope::prepare_automation_execution_scope;
    use crate::automation_lifecycle_revision::{
        AutomationLifecycleClock, AutomationRevisionCandidate, AutomationRevisionGuardOutcome,
        revalidate_revision_candidate,
    };
    use crate::automation_serialization::{
        AutomationAdmissionOutcome, AutomationSerializationCoordinator,
    };
    use crate::automation_shadow_runtime::AutomationShadowRuntime;

    struct FakeOwner {
        calls: Mutex<Vec<PerformanceProfile>>,
        result: Mutex<Option<Result<PerformanceCommandOutcome, SetPerformanceError>>>,
    }

    impl FakeOwner {
        fn new(result: Result<PerformanceCommandOutcome, SetPerformanceError>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                result: Mutex::new(Some(result)),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl PerformanceAutomationOwner for FakeOwner {
        async fn set_performance_for_automation(
            &self,
            profile: PerformanceProfile,
        ) -> Result<PerformanceCommandOutcome, SetPerformanceError> {
            self.calls.lock().unwrap().push(profile);
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("one fake result")
        }
    }

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

    fn telemetry(ac_online: bool, ts: SystemTime) -> orbis_core::telemetry::Telemetry {
        let mut telemetry = orbis_core::telemetry::Telemetry::empty();
        telemetry.ac_online = Some(ac_online);
        telemetry.ts = ts;
        telemetry
    }

    fn lease_and_prepared(
        generation: u64,
    ) -> (
        AutomationSerializationCoordinator,
        AutomationDryRunLease,
        AutomationPreparedBatch,
    ) {
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
            panic!("expected revision handoff");
        };
        let mut coordinator = AutomationSerializationCoordinator::new();
        let admission = coordinator.admit(handoff, revision, generation);
        let AutomationAdmissionOutcome::Admitted(lease) = admission else {
            panic!("expected lease");
        };
        assert_eq!(
            lease.actions(),
            &[AutomationAction::SetProfile(PerformanceProfile::Balanced)]
        );
        let prepared = prepare_automation_execution_scope(&lease).unwrap();
        (coordinator, lease, prepared)
    }

    fn command_outcome(
        result: ApplyResult,
        current: PerformanceProfile,
    ) -> PerformanceCommandOutcome {
        orbis_application::CommandOutcome {
            result,
            state: orbis_application::PerformanceState {
                current,
                available: PerformanceProfile::ALL.to_vec(),
            },
        }
    }

    #[tokio::test]
    async fn applied_requires_exact_authoritative_readback() {
        let (_coordinator, lease, prepared) = lease_and_prepared(7);
        let owner = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Applied,
            PerformanceProfile::Balanced,
        )));

        let outcome = run_performance_proof(
            &owner,
            &lease,
            &prepared,
            lease.required_revision(),
            7,
        )
        .await;
        assert!(matches!(
            outcome,
            AutomationPerformanceExecutionOutcome::Applied {
                lease_id: 1,
                generation: 7,
                profile: PerformanceProfile::Balanced,
                ..
            }
        ));
        assert_eq!(owner.call_count(), 1);
    }

    #[tokio::test]
    async fn newer_lifecycle_revision_blocks_before_owner_call() {
        let (_coordinator, lease, prepared) = lease_and_prepared(8);
        let owner = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Applied,
            PerformanceProfile::Balanced,
        )));
        let mut clock = AutomationLifecycleClock::new();
        assert_eq!(clock.advance().unwrap(), lease.required_revision());
        let newer = clock.advance().unwrap();

        let outcome = run_performance_proof(&owner, &lease, &prepared, newer, 8).await;
        assert!(matches!(
            outcome,
            AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::LifecycleRevisionChanged { .. }
            )
        ));
        assert_eq!(owner.call_count(), 0);
    }

    #[tokio::test]
    async fn generation_drift_blocks_before_owner_call() {
        let (_coordinator, lease, prepared) = lease_and_prepared(9);
        let owner = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Applied,
            PerformanceProfile::Balanced,
        )));
        let outcome = run_performance_proof(
            &owner,
            &lease,
            &prepared,
            lease.required_revision(),
            10,
        )
        .await;
        assert!(matches!(
            outcome,
            AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::CapabilityGenerationChanged {
                    required: 9,
                    current: 10,
                }
            )
        ));
        assert_eq!(owner.call_count(), 0);
    }

    #[tokio::test]
    async fn accepted_or_mismatched_readback_never_claims_applied() {
        let (_coordinator, lease, prepared) = lease_and_prepared(11);
        let accepted = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Accepted,
            PerformanceProfile::Balanced,
        )));
        assert!(matches!(
            run_performance_proof(
                &accepted,
                &lease,
                &prepared,
                lease.required_revision(),
                11,
            )
            .await,
            AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::UnexpectedApplyResult { .. }
            )
        ));

        let (_coordinator, lease, prepared) = lease_and_prepared(12);
        let mismatch = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Applied,
            PerformanceProfile::Silent,
        )));
        assert!(matches!(
            run_performance_proof(
                &mismatch,
                &lease,
                &prepared,
                lease.required_revision(),
                12,
            )
            .await,
            AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::ReadBackMismatch { .. }
            )
        ));
    }

    #[tokio::test]
    async fn command_and_post_mutation_readback_failures_remain_distinct() {
        let (_coordinator, lease, prepared) = lease_and_prepared(13);
        let command = FakeOwner::new(Err(CommandError::Command(
            ProviderError::Unsupported("test".into()),
        )));
        assert!(matches!(
            run_performance_proof(
                &command,
                &lease,
                &prepared,
                lease.required_revision(),
                13,
            )
            .await,
            AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::Command(_)
            )
        ));

        let (_coordinator, lease, prepared) = lease_and_prepared(14);
        let readback = FakeOwner::new(Err(CommandError::ReadBack {
            result: ApplyResult::Applied,
            source: ProviderError::Timeout("test".into()),
        }));
        assert!(matches!(
            run_performance_proof(
                &readback,
                &lease,
                &prepared,
                lease.required_revision(),
                14,
            )
            .await,
            AutomationPerformanceExecutionOutcome::Failed(
                AutomationPerformanceExecutionError::ReadBackAfterMutation { .. }
            )
        ));
    }
}