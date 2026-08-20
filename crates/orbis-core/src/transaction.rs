//! Pure mutation transaction state.
//!
//! This module models the lifecycle of a risky mutation without performing any
//! I/O. It deliberately distinguishes an accepted request, authoritative
//! observation, pending requirement, rollback and an unknown timeout outcome.

use serde::{Deserialize, Serialize};

use crate::{ActionRequirement, ApplyResult};

/// Lifecycle phase of one mutation transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationPhase {
    /// Previous state and target are known; no mutation result recorded yet.
    Prepared,
    /// Backend accepted/applied something but authoritative hardware
    /// observation is still required before success can be claimed.
    AwaitingObservation,
    /// Target was accepted but requires an explicit user confirmation.
    AwaitingConfirmation,
    /// Target is pending an external lifecycle requirement.
    PendingRequirement {
        /// Requirement that must occur before the target can become observed.
        requirement: ActionRequirement,
    },
    /// Authoritative observation proved the requested target.
    Applied,
    /// Previous state was restored.
    RolledBack {
        /// Human-readable technical reason for the rollback.
        reason: String,
    },
    /// Mutation failed with a proven failure result.
    Failed {
        /// Technical failure reason.
        reason: String,
        /// Backend that reported the failure.
        backend: String,
    },
    /// Execution timed out or transport was lost after dispatch, so the
    /// hardware outcome is not known. Blind retry is unsafe.
    UnknownOutcome {
        /// Technical reason why the result is unknown.
        reason: String,
    },
}

/// Inert transaction record for one typed setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationTransaction<T> {
    /// Authoritative value observed before the mutation.
    pub previous: T,
    /// Requested target.
    pub target: T,
    /// Current lifecycle phase.
    pub phase: MutationPhase,
}

impl<T> MutationTransaction<T> {
    /// Start a transaction after a successful read-before-write observation.
    pub fn new(previous: T, target: T) -> Self {
        Self {
            previous,
            target,
            phase: MutationPhase::Prepared,
        }
    }

    /// Record the immediate provider/backend result.
    ///
    /// `Applied` and `Accepted` both still require an application-level
    /// authoritative observation in this state machine. `Pending` preserves
    /// the external requirement. No result path performs retry automatically.
    pub fn record_apply_result(&mut self, result: &ApplyResult) {
        self.phase = match result {
            ApplyResult::Applied | ApplyResult::Accepted => MutationPhase::AwaitingObservation,
            ApplyResult::Pending { requirement } => match requirement {
                ActionRequirement::Confirmation => MutationPhase::AwaitingConfirmation,
                requirement => MutationPhase::PendingRequirement {
                    requirement: *requirement,
                },
            },
            ApplyResult::Failed { reason, backend } => MutationPhase::Failed {
                reason: reason.clone(),
                backend: backend.clone(),
            },
            ApplyResult::RolledBack { reason } => MutationPhase::RolledBack {
                reason: reason.clone(),
            },
        };
    }

    /// Mark dispatch as having an unknown outcome, for example after timeout.
    pub fn mark_unknown_outcome(&mut self, reason: impl Into<String>) {
        self.phase = MutationPhase::UnknownOutcome {
            reason: reason.into(),
        };
    }

    /// Record a successful rollback to the previously observed value.
    pub fn mark_rolled_back(&mut self, reason: impl Into<String>) {
        self.phase = MutationPhase::RolledBack {
            reason: reason.into(),
        };
    }

    /// Record a proven failure without claiming anything about hardware state.
    pub fn mark_failed(&mut self, reason: impl Into<String>, backend: impl Into<String>) {
        self.phase = MutationPhase::Failed {
            reason: reason.into(),
            backend: backend.into(),
        };
    }

    /// Whether a blind automatic retry is safe from the state-machine point of
    /// view.
    ///
    /// Only `Prepared` is retryable because no mutation attempt has been
    /// recorded yet. Once any attempt was dispatched, even a reported failure
    /// may conceal backend-specific partial effects; retry requires explicit
    /// higher-level evidence and policy.
    pub fn allows_automatic_retry(&self) -> bool {
        matches!(self.phase, MutationPhase::Prepared)
    }
}

impl<T: PartialEq> MutationTransaction<T> {
    /// Feed one authoritative read-back into the transaction.
    ///
    /// A matching value proves `Applied` only when the transaction is waiting
    /// for observation. Pending reboot/logout/confirmation is not silently
    /// cleared by an incidental observation.
    pub fn observe(&mut self, observed: &T) -> bool {
        if matches!(self.phase, MutationPhase::AwaitingObservation) && observed == &self.target {
            self.phase = MutationPhase::Applied;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_result_still_requires_authoritative_observation() {
        let mut tx = MutationTransaction::new(1u8, 2u8);
        tx.record_apply_result(&ApplyResult::Applied);
        assert_eq!(tx.phase, MutationPhase::AwaitingObservation);
        assert!(!tx.observe(&1));
        assert!(tx.observe(&2));
        assert_eq!(tx.phase, MutationPhase::Applied);
    }

    #[test]
    fn accepted_is_never_treated_as_applied_without_observation() {
        let mut tx = MutationTransaction::new(1u8, 2u8);
        tx.record_apply_result(&ApplyResult::Accepted);
        assert_eq!(tx.phase, MutationPhase::AwaitingObservation);
    }

    #[test]
    fn lifecycle_requirement_is_preserved() {
        let mut tx = MutationTransaction::new(1u8, 2u8);
        tx.record_apply_result(&ApplyResult::Pending {
            requirement: ActionRequirement::Reboot,
        });
        assert_eq!(
            tx.phase,
            MutationPhase::PendingRequirement {
                requirement: ActionRequirement::Reboot
            }
        );
        assert!(!tx.observe(&2));
    }

    #[test]
    fn timeout_unknown_outcome_forbids_blind_retry() {
        let mut tx = MutationTransaction::new(1u8, 2u8);
        tx.mark_unknown_outcome("provider timed out after dispatch");
        assert!(matches!(tx.phase, MutationPhase::UnknownOutcome { .. }));
        assert!(!tx.allows_automatic_retry());
    }

    #[test]
    fn reported_failure_still_forbids_automatic_retry() {
        let mut tx = MutationTransaction::new(1u8, 2u8);
        tx.mark_failed("request rejected", "test");
        assert!(!tx.allows_automatic_retry());
    }

    #[test]
    fn only_prepared_transaction_is_automatically_retryable() {
        let tx = MutationTransaction::new(1u8, 2u8);
        assert!(tx.allows_automatic_retry());
    }

    #[test]
    fn serde_roundtrip_preserves_unknown_outcome() {
        let mut tx = MutationTransaction::new(40u8, 80u8);
        tx.mark_unknown_outcome("timeout");
        let json = serde_json::to_string(&tx).unwrap();
        let back: MutationTransaction<u8> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tx);
    }
}
