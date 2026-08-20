//! Pure execution-scope validation for the first Automation executor version.
//!
//! The current production project has one already-proven application mutation
//! owner suitable for unattended execution work: Performance profile changes.
//! GPU product mutation is policy-disabled, Display refresh has no mutation
//! owner, and Lighting is still ambiguous. This module therefore converts a
//! serialized dry-run lease into either a no-op/Performance-only prepared batch
//! or one fail-closed blocker. It performs no I/O and no mutation.

use orbis_core::automation::{AutomationAction, AutomationTrigger};
use orbis_core::profile::PerformanceProfile;

use crate::automation_serialization::AutomationDryRunLease;

/// First-version executor scope after all earlier lifecycle/capability guards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPreparedKind {
    /// Policy contained only KeepCurrent choices; no mutation is required.
    NoOp,
    /// Exactly one proven Performance profile intent.
    Performance(PerformanceProfile),
}

/// Prepared hardware-inert batch metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationPreparedBatch {
    lease_id: u64,
    required_generation: u64,
    trigger: AutomationTrigger,
    kind: AutomationPreparedKind,
}

impl AutomationPreparedBatch {
    /// Serialization lease identifier retained for audit/result correlation.
    pub fn lease_id(&self) -> u64 {
        self.lease_id
    }

    /// Capability generation that must remain current through future execution.
    pub fn required_generation(&self) -> u64 {
        self.required_generation
    }

    /// Lifecycle trigger that produced the batch.
    pub fn trigger(&self) -> &AutomationTrigger {
        &self.trigger
    }

    /// Exact first-version execution kind.
    pub fn kind(&self) -> &AutomationPreparedKind {
        &self.kind
    }
}

/// Why a serialized batch is outside the first executor's proven owner scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationExecutionScopeBlock {
    /// More than one Performance mutation appeared in a single batch. The
    /// current policy model should not generate this; treat it as a contract
    /// violation rather than choosing an order.
    DuplicatePerformanceAction,
    /// A planned action has no proven first-version executor owner.
    UnsupportedAction(AutomationAction),
}

/// Validate that one dry-run lease is fully representable by the first
/// Performance-only executor scope.
///
/// The entire batch is rejected when any unsupported action is present. There
/// is no partial subset API: a future executor must never run Performance while
/// silently dropping GPU/Display/Lighting/Custom intent from the same policy.
pub fn prepare_automation_execution_scope(
    lease: &AutomationDryRunLease,
) -> Result<AutomationPreparedBatch, AutomationExecutionScopeBlock> {
    let kind = classify_actions(lease.actions())?;
    Ok(AutomationPreparedBatch {
        lease_id: lease.id(),
        required_generation: lease.required_generation(),
        trigger: lease.trigger().clone(),
        kind,
    })
}

fn classify_actions(
    actions: &[AutomationAction],
) -> Result<AutomationPreparedKind, AutomationExecutionScopeBlock> {
    let mut performance = None;

    for action in actions {
        match action {
            AutomationAction::SetProfile(profile) => {
                if performance.replace(*profile).is_some() {
                    return Err(AutomationExecutionScopeBlock::DuplicatePerformanceAction);
                }
            }
            unsupported => {
                return Err(AutomationExecutionScopeBlock::UnsupportedAction(
                    unsupported.clone(),
                ));
            }
        }
    }

    Ok(match performance {
        Some(profile) => AutomationPreparedKind::Performance(profile),
        None => AutomationPreparedKind::NoOp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::automation::RefreshPolicy;
    use orbis_core::gpu::GpuMode;
    use orbis_core::newtypes::RefreshHz;

    #[test]
    fn empty_batch_is_explicit_noop() {
        assert_eq!(classify_actions(&[]), Ok(AutomationPreparedKind::NoOp));
    }

    #[test]
    fn exactly_one_performance_action_is_the_only_mutating_v1_scope() {
        assert_eq!(
            classify_actions(&[AutomationAction::SetProfile(
                PerformanceProfile::Balanced
            )]),
            Ok(AutomationPreparedKind::Performance(
                PerformanceProfile::Balanced
            ))
        );
    }

    #[test]
    fn duplicate_performance_actions_fail_closed() {
        assert_eq!(
            classify_actions(&[
                AutomationAction::SetProfile(PerformanceProfile::Silent),
                AutomationAction::SetProfile(PerformanceProfile::Turbo),
            ]),
            Err(AutomationExecutionScopeBlock::DuplicatePerformanceAction)
        );
    }

    #[test]
    fn every_unproven_owner_blocks_the_whole_batch() {
        let unsupported = [
            AutomationAction::SetGpuPolicy(GpuMode::Standard),
            AutomationAction::SetRefreshPolicy(RefreshPolicy::Fixed(
                RefreshHz::new(60).unwrap(),
            )),
            AutomationAction::SetLighting(false),
            AutomationAction::CustomCommand(vec!["true".into()]),
        ];

        for action in unsupported {
            assert_eq!(
                classify_actions(&[
                    AutomationAction::SetProfile(PerformanceProfile::Balanced),
                    action.clone(),
                ]),
                Err(AutomationExecutionScopeBlock::UnsupportedAction(action))
            );
        }
    }

    #[test]
    fn source_contains_no_mutation_or_process_surface() {
        let source = include_str!("automation_execution_scope.rs");
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
