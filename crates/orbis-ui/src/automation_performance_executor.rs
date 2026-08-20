//! Production-ready Performance-only Automation execution boundary.
//!
//! This module contains the only unattended mutation call currently modeled for
//! Automation. It does not decide whether Automation is enabled: callers must
//! first obtain a move-only [`AutomationWorkerPreparedEnvelope`] from the
//! worker-owned driver. Immediately before the owner call this executor checks
//! persisted-policy revision, lifecycle revision and capability generation again.
//!
//! Promotion of `FeatureId::Automation.write` remains a separate release/evidence
//! decision. Keeping this module compiled in production does not make it
//! reachable while that capability is `Unsupported`.

use async_trait::async_trait;
use orbis_application::{CommandError, PerformanceCommandOutcome, SetPerformanceError};
use orbis_core::action::ApplyResult;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;

use crate::automation_execution_scope::AutomationPreparedKind;
use crate::automation_lifecycle_revision::AutomationLifecycleRevision;
use crate::automation_worker_driver::{
    AutomationPolicyRevision, AutomationWorkerPreparedEnvelope,
};
use crate::composition::PerformanceServiceRuntime;

/// Fail-closed error detected before a mutation or a definite command failure.
#[derive(Debug)]
pub enum AutomationPerformanceExecutionError {
    /// Prepared batch metadata does not match the serialization lease.
    EnvelopeMetadataMismatch,
    /// Persisted policy changed after preparation.
    PolicyRevisionChanged {
        required: AutomationPolicyRevision,
        current: AutomationPolicyRevision,
    },
    /// A newer lifecycle event superseded the prepared batch.
    LifecycleRevisionChanged {
        required: AutomationLifecycleRevision,
        current: AutomationLifecycleRevision,
    },
    /// Capability registry changed after preparation.
    CapabilityGenerationChanged { required: u64, current: u64 },
    /// The application owner rejected/failed the command before a successful
    /// post-mutation read-back result existed.
    Command(ProviderError),
}

/// Why the worker must enter typed Performance recovery after an owner call.
#[derive(Debug)]
pub enum AutomationPerformanceRecoveryReason {
    /// Mutation returned a result but mandatory authoritative read-back failed.
    ReadBackAfterMutation {
        result: ApplyResult,
        source: ProviderError,
    },
    /// Application returned a non-`Applied` result. Unattended Automation never
    /// treats `Accepted`, `Pending`, `Failed` or `RolledBack` as confirmed.
    UnexpectedApplyResult {
        result: ApplyResult,
        observed: PerformanceProfile,
    },
    /// Application claimed `Applied`, but authoritative state did not equal the
    /// requested target.
    ReadBackMismatch {
        requested: PerformanceProfile,
        observed: PerformanceProfile,
        result: ApplyResult,
    },
}

/// Result of one serialized Performance-only execution attempt.
#[derive(Debug)]
pub enum AutomationPerformanceExecutionOutcome {
    /// Prepared batch intentionally contains no hardware action.
    NoOp,
    /// Exactly one Performance mutation was confirmed by `Applied` plus exact
    /// authoritative read-back.
    Applied {
        lease_id: u64,
        revision: AutomationLifecycleRevision,
        generation: u64,
        profile: PerformanceProfile,
    },
    /// No unattended success is claimed and the worker may release the lease as
    /// a known failure without entering unknown-outcome recovery.
    DefiniteFailure(AutomationPerformanceExecutionError),
    /// A mutation call happened and final state is not authoritatively known.
    /// The worker must call `finish_performance_unknown` and reconcile through a
    /// fresh typed Performance read before admitting another unattended batch.
    RecoveryRequired {
        requested: PerformanceProfile,
        reason: AutomationPerformanceRecoveryReason,
    },
}

/// Narrow application owner required by the Automation executor.
#[async_trait]
pub trait PerformanceAutomationOwner: Send + Sync {
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

/// Execute one already-prepared Performance-only batch.
///
/// All identity checks occur synchronously immediately before the first owner
/// call. The caller must serialize this function with lifecycle observations,
/// capability replacement and other application mutations under the same worker
/// owner. This function never performs retries and never executes more than one
/// hardware mutation.
pub async fn execute_prepared_performance<O>(
    owner: &O,
    envelope: &AutomationWorkerPreparedEnvelope,
    current_policy_revision: AutomationPolicyRevision,
    current_lifecycle_revision: AutomationLifecycleRevision,
    current_generation: u64,
) -> AutomationPerformanceExecutionOutcome
where
    O: PerformanceAutomationOwner + ?Sized,
{
    let prepared = envelope.prepared();
    let lease = prepared.lease();
    let batch = prepared.batch();

    if batch.lease_id() != lease.id()
        || batch.required_revision() != lease.required_revision()
        || batch.required_generation() != lease.required_generation()
        || batch.trigger() != lease.trigger()
    {
        return AutomationPerformanceExecutionOutcome::DefiniteFailure(
            AutomationPerformanceExecutionError::EnvelopeMetadataMismatch,
        );
    }

    let required_policy_revision = envelope.required_policy_revision();
    if current_policy_revision != required_policy_revision {
        return AutomationPerformanceExecutionOutcome::DefiniteFailure(
            AutomationPerformanceExecutionError::PolicyRevisionChanged {
                required: required_policy_revision,
                current: current_policy_revision,
            },
        );
    }

    let required_revision = lease.required_revision();
    if current_lifecycle_revision != required_revision {
        return AutomationPerformanceExecutionOutcome::DefiniteFailure(
            AutomationPerformanceExecutionError::LifecycleRevisionChanged {
                required: required_revision,
                current: current_lifecycle_revision,
            },
        );
    }

    let required_generation = lease.required_generation();
    if current_generation != required_generation {
        return AutomationPerformanceExecutionOutcome::DefiniteFailure(
            AutomationPerformanceExecutionError::CapabilityGenerationChanged {
                required: required_generation,
                current: current_generation,
            },
        );
    }

    let profile = match batch.kind() {
        AutomationPreparedKind::NoOp => return AutomationPerformanceExecutionOutcome::NoOp,
        AutomationPreparedKind::Performance(profile) => *profile,
    };

    let outcome = match owner.set_performance_for_automation(profile).await {
        Ok(outcome) => outcome,
        Err(CommandError::Command(source)) => {
            return AutomationPerformanceExecutionOutcome::DefiniteFailure(
                AutomationPerformanceExecutionError::Command(source),
            );
        }
        Err(CommandError::ReadBack { result, source }) => {
            return AutomationPerformanceExecutionOutcome::RecoveryRequired {
                requested: profile,
                reason: AutomationPerformanceRecoveryReason::ReadBackAfterMutation {
                    result,
                    source,
                },
            };
        }
    };

    if !outcome.result.is_applied() {
        return AutomationPerformanceExecutionOutcome::RecoveryRequired {
            requested: profile,
            reason: AutomationPerformanceRecoveryReason::UnexpectedApplyResult {
                result: outcome.result,
                observed: outcome.state.current,
            },
        };
    }

    if outcome.state.current != profile {
        return AutomationPerformanceExecutionOutcome::RecoveryRequired {
            requested: profile,
            reason: AutomationPerformanceRecoveryReason::ReadBackMismatch {
                requested: profile,
                observed: outcome.state.current,
                result: outcome.result,
            },
        };
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
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, FeatureId,
        OperationCapability,
    };
    use orbis_core::telemetry::Telemetry;

    use crate::automation_worker_driver::AutomationWorkerDriver;

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

    fn telemetry(ac_online: bool, ts: SystemTime) -> Telemetry {
        let mut telemetry = Telemetry::empty();
        telemetry.ac_online = Some(ac_online);
        telemetry.ts = ts;
        telemetry
    }

    fn prepared(
        generation: u64,
    ) -> (AutomationWorkerDriver, CapabilityRegistrySnapshot, AutomationWorkerPreparedEnvelope) {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let snapshot = snapshot(generation, base);
        let mut driver = AutomationWorkerDriver::with_policy(policy());
        assert!(matches!(
            driver.observe_telemetry(&telemetry(true, base), &snapshot, base),
            crate::automation_worker_driver::AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(1)),
                &snapshot,
                base + Duration::from_secs(1),
            ),
            crate::automation_worker_driver::AutomationWorkerObservation::NoConfirmedEvent
        ));
        assert!(matches!(
            driver.observe_telemetry(
                &telemetry(false, base + Duration::from_secs(2)),
                &snapshot,
                base + Duration::from_secs(2),
            ),
            crate::automation_worker_driver::AutomationWorkerObservation::Confirmed(_)
        ));
        let envelope = driver
            .prepare_latest_dry_run(
                &snapshot,
                base + Duration::from_secs(3),
                Duration::from_secs(30),
            )
            .expect("prepared envelope");
        (driver, snapshot, envelope)
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
    async fn exact_applied_readback_is_the_only_success() {
        let (driver, snapshot, envelope) = prepared(7);
        let owner = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Applied,
            PerformanceProfile::Balanced,
        )));
        let outcome = execute_prepared_performance(
            &owner,
            &envelope,
            driver.current_policy_revision(),
            driver.current_revision(),
            snapshot.generation(),
        )
        .await;
        assert!(matches!(
            outcome,
            AutomationPerformanceExecutionOutcome::Applied {
                generation: 7,
                profile: PerformanceProfile::Balanced,
                ..
            }
        ));
        assert_eq!(owner.call_count(), 1);
    }

    #[tokio::test]
    async fn identity_drift_blocks_before_owner_call() {
        let (driver, snapshot, envelope) = prepared(9);
        let owner = FakeOwner::new(Ok(command_outcome(
            ApplyResult::Applied,
            PerformanceProfile::Balanced,
        )));
        let outcome = execute_prepared_performance(
            &owner,
            &envelope,
            AutomationPolicyRevision::INITIAL,
            driver.current_revision(),
            snapshot.generation(),
        )
        .await;
        assert!(matches!(
            outcome,
            AutomationPerformanceExecutionOutcome::DefiniteFailure(
                AutomationPerformanceExecutionError::PolicyRevisionChanged { .. }
            )
        ));
        assert_eq!(owner.call_count(), 0);
    }

    #[tokio::test]
    async fn readback_failure_enters_recovery_classification() {
        let (driver, snapshot, envelope) = prepared(11);
        let owner = FakeOwner::new(Err(CommandError::ReadBack {
            result: ApplyResult::Applied,
            source: ProviderError::Timeout("test".into()),
        }));
        let outcome = execute_prepared_performance(
            &owner,
            &envelope,
            driver.current_policy_revision(),
            driver.current_revision(),
            snapshot.generation(),
        )
        .await;
        assert!(matches!(
            outcome,
            AutomationPerformanceExecutionOutcome::RecoveryRequired { .. }
        ));
        assert_eq!(owner.call_count(), 1);
    }

    #[test]
    fn executor_source_has_only_performance_owner_surface() {
        let source = include_str!("automation_performance_executor.rs");
        for forbidden in [
            ["set_", "gpu_mode"].concat(),
            ["set_", "fan_curve"].concat(),
            ["set_", "charge_limit"].concat(),
            ["Command", "::new"].concat(),
        ] {
            assert!(!source.contains(&forbidden), "unexpected executor surface: {forbidden}");
        }
    }
}
