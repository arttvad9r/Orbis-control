//! Pure reconciliation decisions for desired/observed/pending state.
//!
//! No provider calls or persistence happen here. The function only decides
//! what the next safe class of action is from authoritative state and current
//! write-capability evidence.

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityStatus;
use crate::desired_observed::{DesiredObservedState, DesiredValue, ObservedValue};
use crate::ActionRequirement;

/// Pure next-step decision for one typed setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReconcileDecision<T> {
    /// No desired value is currently owned; nothing should be written.
    NoDesired,
    /// An authoritative observation is required before any mutation decision.
    Observe,
    /// Desired and observed already match.
    Converged,
    /// One mutation attempt may be planned for this target.
    Apply {
        /// Desired target.
        target: T,
    },
    /// A previously recorded transition is waiting for an explicit
    /// confirmation or lifecycle requirement.
    Wait {
        /// Pending target.
        target: T,
        /// Requirement that must be satisfied.
        requirement: ActionRequirement,
    },
    /// A mutation is required for convergence but current write evidence does
    /// not permit it.
    Blocked {
        /// Desired target.
        target: T,
        /// Current write-operation capability status.
        status: CapabilityStatus,
    },
}

/// Decide the next safe reconciliation step.
///
/// The function never infers observed state from desired state and never clears
/// pending implicitly. Unknown observation always wins over write capability:
/// Orbis must read before it writes.
pub fn decide_reconciliation<T>(
    state: &DesiredObservedState<T>,
    write_status: CapabilityStatus,
) -> ReconcileDecision<T>
where
    T: Clone + PartialEq,
{
    if let Some(pending) = &state.pending {
        return ReconcileDecision::Wait {
            target: pending.target.clone(),
            requirement: pending.requirement,
        };
    }

    let desired = match &state.desired {
        DesiredValue::Unset => return ReconcileDecision::NoDesired,
        DesiredValue::Set(value) => value,
    };

    let observed = match &state.observed {
        ObservedValue::Unknown => return ReconcileDecision::Observe,
        ObservedValue::Known(value) => value,
    };

    if desired == observed {
        return ReconcileDecision::Converged;
    }

    match write_status {
        CapabilityStatus::Supported | CapabilityStatus::SupportedWithRequirement => {
            ReconcileDecision::Apply {
                target: desired.clone(),
            }
        }
        status => ReconcileDecision::Blocked {
            target: desired.clone(),
            status,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desired_observed::PendingValue;

    #[test]
    fn no_desired_means_no_action() {
        let state = DesiredObservedState::<u8>::default();
        assert_eq!(
            decide_reconciliation(&state, CapabilityStatus::Supported),
            ReconcileDecision::NoDesired
        );
    }

    #[test]
    fn unknown_observation_requires_read_before_write() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        assert_eq!(
            decide_reconciliation(&state, CapabilityStatus::Supported),
            ReconcileDecision::Observe
        );
    }

    #[test]
    fn matching_values_are_converged() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        state.set_observed(80);
        assert_eq!(
            decide_reconciliation(&state, CapabilityStatus::Supported),
            ReconcileDecision::Converged
        );
    }

    #[test]
    fn mismatch_with_supported_write_can_apply_once() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        state.set_observed(60);
        assert_eq!(
            decide_reconciliation(&state, CapabilityStatus::Supported),
            ReconcileDecision::Apply { target: 80 }
        );
    }

    #[test]
    fn mismatch_with_readonly_is_blocked_not_faked() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        state.set_observed(60);
        assert_eq!(
            decide_reconciliation(&state, CapabilityStatus::ReadOnly),
            ReconcileDecision::Blocked {
                target: 80,
                status: CapabilityStatus::ReadOnly
            }
        );
    }

    #[test]
    fn pending_requirement_has_priority_over_incidental_match() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        state.set_observed(80);
        state.set_pending(PendingValue::new(80, ActionRequirement::Reboot));
        assert_eq!(
            decide_reconciliation(&state, CapabilityStatus::Supported),
            ReconcileDecision::Wait {
                target: 80,
                requirement: ActionRequirement::Reboot
            }
        );
    }
}
