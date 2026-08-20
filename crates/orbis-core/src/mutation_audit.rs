//! Mutation audit records.
//!
//! Audit state is descriptive only. It records what Orbis requested and what
//! was proven afterwards; it never authorizes or replays a mutation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ActionRequirement, ApplyResult, FeatureId, MutationPhase};

/// User-visible/diagnostic classification of a mutation outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationAuditOutcome {
    /// Authoritative observation proved the requested target.
    Applied,
    /// Backend/config accepted the request but hardware state was not proven.
    Accepted,
    /// External requirement remains.
    Pending {
        /// Requirement such as reboot/logout/confirmation.
        requirement: ActionRequirement,
    },
    /// Previous state was restored.
    RolledBack {
        /// Technical reason.
        reason: String,
    },
    /// Failure was proven.
    Failed {
        /// Technical reason.
        reason: String,
        /// Reporting backend.
        backend: String,
    },
    /// Dispatch may have reached hardware but completion was not observed.
    UnknownOutcome {
        /// Technical reason.
        reason: String,
    },
}

/// One audit record for a typed feature mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationAuditEntry {
    /// Caller-generated operation id.
    pub operation_id: String,
    /// Feature affected.
    pub feature: FeatureId,
    /// Stable technical target summary. Domain-specific structured targets may
    /// be stored alongside this record by their owning subsystem.
    pub target: String,
    /// Proven outcome class.
    pub outcome: MutationAuditOutcome,
    /// Unix timestamp milliseconds supplied by the application clock.
    pub recorded_at_ms: u64,
}

/// Latest mutation audit record per feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LastMutationAudit {
    entries: BTreeMap<FeatureId, MutationAuditEntry>,
}

impl LastMutationAudit {
    /// Record or replace the latest entry for its feature.
    pub fn record(&mut self, entry: MutationAuditEntry) {
        self.entries.insert(entry.feature, entry);
    }

    /// Latest entry for a feature.
    pub fn latest(&self, feature: FeatureId) -> Option<&MutationAuditEntry> {
        self.entries.get(&feature)
    }

    /// Iterate in deterministic `FeatureId` order.
    pub fn iter(&self) -> impl Iterator<Item = (&FeatureId, &MutationAuditEntry)> {
        self.entries.iter()
    }

    /// Remove all audit entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Convert an immediate provider `ApplyResult` into audit semantics.
///
/// `Applied` is accepted here only as the provider's stated result; callers
/// that require application-level observation should later replace it from the
/// transaction phase with [`outcome_from_phase`].
pub fn outcome_from_apply_result(result: &ApplyResult) -> MutationAuditOutcome {
    match result {
        ApplyResult::Applied => MutationAuditOutcome::Applied,
        ApplyResult::Accepted => MutationAuditOutcome::Accepted,
        ApplyResult::Pending { requirement } => MutationAuditOutcome::Pending {
            requirement: *requirement,
        },
        ApplyResult::Failed { reason, backend } => MutationAuditOutcome::Failed {
            reason: reason.clone(),
            backend: backend.clone(),
        },
        ApplyResult::RolledBack { reason } => MutationAuditOutcome::RolledBack {
            reason: reason.clone(),
        },
    }
}

/// Convert the transaction state machine into conservative audit semantics.
///
/// Pre-terminal states map to `Pending`; `AwaitingObservation` is deliberately
/// not `Applied`.
pub fn outcome_from_phase(phase: &MutationPhase) -> MutationAuditOutcome {
    match phase {
        MutationPhase::Prepared | MutationPhase::AwaitingObservation => MutationAuditOutcome::Pending {
            requirement: ActionRequirement::None,
        },
        MutationPhase::AwaitingConfirmation => MutationAuditOutcome::Pending {
            requirement: ActionRequirement::Confirmation,
        },
        MutationPhase::PendingRequirement { requirement } => MutationAuditOutcome::Pending {
            requirement: *requirement,
        },
        MutationPhase::Applied => MutationAuditOutcome::Applied,
        MutationPhase::RolledBack { reason } => MutationAuditOutcome::RolledBack {
            reason: reason.clone(),
        },
        MutationPhase::Failed { reason, backend } => MutationAuditOutcome::Failed {
            reason: reason.clone(),
            backend: backend.clone(),
        },
        MutationPhase::UnknownOutcome { reason } => MutationAuditOutcome::UnknownOutcome {
            reason: reason.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_and_unknown_outcome_remain_distinct() {
        assert_eq!(
            outcome_from_apply_result(&ApplyResult::Accepted),
            MutationAuditOutcome::Accepted
        );
        assert_eq!(
            outcome_from_phase(&MutationPhase::UnknownOutcome {
                reason: "timeout".into()
            }),
            MutationAuditOutcome::UnknownOutcome {
                reason: "timeout".into()
            }
        );
    }

    #[test]
    fn awaiting_observation_is_not_audited_as_applied() {
        assert_eq!(
            outcome_from_phase(&MutationPhase::AwaitingObservation),
            MutationAuditOutcome::Pending {
                requirement: ActionRequirement::None
            }
        );
    }

    #[test]
    fn latest_record_replaces_same_feature_only() {
        let mut audit = LastMutationAudit::default();
        audit.record(MutationAuditEntry {
            operation_id: "1".into(),
            feature: FeatureId::Performance,
            target: "balanced".into(),
            outcome: MutationAuditOutcome::Applied,
            recorded_at_ms: 1,
        });
        audit.record(MutationAuditEntry {
            operation_id: "2".into(),
            feature: FeatureId::Performance,
            target: "turbo".into(),
            outcome: MutationAuditOutcome::Applied,
            recorded_at_ms: 2,
        });
        audit.record(MutationAuditEntry {
            operation_id: "3".into(),
            feature: FeatureId::ChargeLimit,
            target: "80".into(),
            outcome: MutationAuditOutcome::Accepted,
            recorded_at_ms: 3,
        });

        assert_eq!(
            audit.latest(FeatureId::Performance).unwrap().operation_id,
            "2"
        );
        assert_eq!(audit.iter().count(), 2);
    }
}
