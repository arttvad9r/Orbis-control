//! Reconciliation scheduling and lifecycle triggers.
//!
//! This module decides when a setting should be reconsidered. It does not
//! decide the hardware target and never performs a mutation.

use serde::{Deserialize, Serialize};

use crate::{MutationPhase, ReconcileDecision};

/// Reason a reconciliation pass was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileTrigger {
    /// Initial startup after configuration was loaded.
    Startup,
    /// Desired intent changed due to direct user action.
    DesiredChanged,
    /// System resumed from sleep.
    Resume,
    /// Capability/readiness evidence changed.
    CapabilityChanged,
    /// Authoritative observation changed.
    ObservationChanged,
    /// Explicit user/manual refresh.
    ManualRefresh,
}

/// Whether a reconciliation pass may dispatch a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchPermission {
    /// Pass may only observe/compare.
    ObserveOnly,
    /// A single typed mutation may be dispatched if the reconciliation
    /// decision independently calls for `Apply`.
    MayMutate,
}

/// Determine dispatch permission for a trigger.
///
/// Startup is observe-only by default: merely loading persisted/default config
/// must not apply hardware state. Resume and capability changes also start
/// observe-only so changed hardware/backend state is learned before any write.
/// A direct desired change may mutate after normal capability/reconciliation
/// checks.
pub fn dispatch_permission(trigger: ReconcileTrigger) -> DispatchPermission {
    match trigger {
        ReconcileTrigger::DesiredChanged => DispatchPermission::MayMutate,
        ReconcileTrigger::Startup
        | ReconcileTrigger::Resume
        | ReconcileTrigger::CapabilityChanged
        | ReconcileTrigger::ObservationChanged
        | ReconcileTrigger::ManualRefresh => DispatchPermission::ObserveOnly,
    }
}

/// Whether an existing transaction phase suppresses another mutation attempt.
///
/// Pending external requirements, observation wait and unknown outcomes all
/// suppress dispatch. This prevents duplicate writes while state is unresolved.
pub fn transaction_blocks_dispatch(phase: &MutationPhase) -> bool {
    matches!(
        phase,
        MutationPhase::AwaitingObservation
            | MutationPhase::AwaitingConfirmation
            | MutationPhase::PendingRequirement { .. }
            | MutationPhase::UnknownOutcome { .. }
    )
}

/// Whether a pure reconciliation decision and lifecycle trigger jointly permit
/// a mutation dispatch.
pub fn may_dispatch<T>(
    trigger: ReconcileTrigger,
    decision: &ReconcileDecision<T>,
    transaction_phase: Option<&MutationPhase>,
) -> bool {
    if transaction_phase.is_some_and(transaction_blocks_dispatch) {
        return false;
    }
    matches!(dispatch_permission(trigger), DispatchPermission::MayMutate)
        && matches!(decision, ReconcileDecision::Apply { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionRequirement, CapabilityStatus};

    #[test]
    fn startup_and_resume_never_write_just_because_config_exists() {
        assert_eq!(
            dispatch_permission(ReconcileTrigger::Startup),
            DispatchPermission::ObserveOnly
        );
        assert_eq!(
            dispatch_permission(ReconcileTrigger::Resume),
            DispatchPermission::ObserveOnly
        );
    }

    #[test]
    fn direct_desired_change_can_dispatch_after_apply_decision() {
        let decision = ReconcileDecision::Apply { target: 80u8 };
        assert!(may_dispatch(
            ReconcileTrigger::DesiredChanged,
            &decision,
            None
        ));
    }

    #[test]
    fn blocked_or_observe_decisions_never_dispatch() {
        assert!(!may_dispatch(
            ReconcileTrigger::DesiredChanged,
            &ReconcileDecision::<u8>::Observe,
            None
        ));
        assert!(!may_dispatch(
            ReconcileTrigger::DesiredChanged,
            &ReconcileDecision::Blocked {
                target: 80u8,
                status: CapabilityStatus::ReadOnly,
            },
            None
        ));
    }

    #[test]
    fn unknown_or_pending_transaction_suppresses_duplicate_write() {
        assert!(!may_dispatch(
            ReconcileTrigger::DesiredChanged,
            &ReconcileDecision::Apply { target: 80u8 },
            Some(&MutationPhase::UnknownOutcome {
                reason: "timeout".into(),
            })
        ));
        assert!(!may_dispatch(
            ReconcileTrigger::DesiredChanged,
            &ReconcileDecision::Apply { target: 80u8 },
            Some(&MutationPhase::PendingRequirement {
                requirement: ActionRequirement::Reboot,
            })
        ));
    }
}
