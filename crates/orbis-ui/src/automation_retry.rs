//! Pure retry classification for blocked Automation lifecycle evaluations.
//!
//! Confirmed lifecycle events must not be retried indiscriminately: policy,
//! authorization, unsupported-target and malformed-evidence failures are not
//! reasons for background mutation attempts. This module identifies only
//! blockers that may become valid after a fresh capability generation.
//! It performs no I/O, scheduling or execution.

use orbis_core::capability::{CapabilityStatus, FeatureId};

use crate::automation_shadow_runtime::{
    AutomationShadowBlock, AutomationShadowOutcome,
};
use orbis_config::AutomationPreflightBlock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationRetryReason {
    CapabilitySnapshotStale,
    RuntimeEvidenceTransient(CapabilityStatus),
    ActionEvidenceTransient {
        feature: FeatureId,
        status: CapabilityStatus,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationRetryDisposition {
    /// Ordinary observation or already-ready outcome; retry classification does
    /// not apply.
    NotApplicable,
    /// The same lifecycle event may be reconsidered only after a newer
    /// capability generation exists. This is not permission to execute.
    AfterCapabilityRefresh(Vec<AutomationRetryReason>),
    /// The current event must not be retried automatically. A new lifecycle,
    /// policy or explicit user action is required.
    Terminal,
}

pub fn classify_automation_retry(
    outcome: &AutomationShadowOutcome,
) -> AutomationRetryDisposition {
    match outcome {
        AutomationShadowOutcome::Observation(_)
        | AutomationShadowOutcome::ReadyButExecutionDisabled { .. } => {
            AutomationRetryDisposition::NotApplicable
        }
        AutomationShadowOutcome::Blocked { block, .. } => match block {
            AutomationShadowBlock::CapabilitySnapshotStale => {
                AutomationRetryDisposition::AfterCapabilityRefresh(vec![
                    AutomationRetryReason::CapabilitySnapshotStale,
                ])
            }
            AutomationShadowBlock::CapabilitySnapshotFromFuture
            | AutomationShadowBlock::ResumePowerSourceUnknown
            | AutomationShadowBlock::ResumeTelemetryStale
            | AutomationShadowBlock::ResumeTelemetryFromFuture => {
                AutomationRetryDisposition::Terminal
            }
        },
        AutomationShadowOutcome::PreflightBlocked { preflight, .. } => {
            let mut reasons = Vec::new();
            for block in &preflight.blocks {
                match block {
                    AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(status)
                        if retryable_status(*status) =>
                    {
                        reasons.push(AutomationRetryReason::RuntimeEvidenceTransient(*status));
                    }
                    AutomationPreflightBlock::ActionWriteUnavailable { feature, status }
                        if retryable_status(*status) =>
                    {
                        reasons.push(AutomationRetryReason::ActionEvidenceTransient {
                            feature: *feature,
                            status: *status,
                        });
                    }
                    // Any non-transient blocker makes the complete batch
                    // terminal for this lifecycle event. We never retry only a
                    // permitted subset of a blocked plan.
                    _ => return AutomationRetryDisposition::Terminal,
                }
            }

            if reasons.is_empty() {
                AutomationRetryDisposition::Terminal
            } else {
                AutomationRetryDisposition::AfterCapabilityRefresh(reasons)
            }
        }
    }
}

fn retryable_status(status: CapabilityStatus) -> bool {
    matches!(
        status,
        CapabilityStatus::TemporarilyUnavailable
            | CapabilityStatus::BackendMissing
            | CapabilityStatus::Unknown
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_config::{AutomationPlan, AutomationPowerSource, AutomationPreflight};
    use orbis_core::automation::AutomationTrigger;

    fn preflight(blocks: Vec<AutomationPreflightBlock>) -> AutomationShadowOutcome {
        AutomationShadowOutcome::PreflightBlocked {
            trigger: AutomationTrigger::OnBattery,
            generation: 7,
            preflight: AutomationPreflight {
                plan: AutomationPlan {
                    trigger: AutomationTrigger::OnBattery,
                    power_source: AutomationPowerSource::Battery,
                    actions: Vec::new(),
                    blocks: Vec::new(),
                },
                blocks,
            },
        }
    }

    #[test]
    fn stale_capability_snapshot_is_retryable_only_after_refresh() {
        let outcome = AutomationShadowOutcome::Blocked {
            trigger: AutomationTrigger::OnResume,
            generation: 4,
            block: AutomationShadowBlock::CapabilitySnapshotStale,
        };
        assert_eq!(
            classify_automation_retry(&outcome),
            AutomationRetryDisposition::AfterCapabilityRefresh(vec![
                AutomationRetryReason::CapabilitySnapshotStale,
            ])
        );
    }

    #[test]
    fn transient_runtime_and_action_evidence_can_retry_as_whole_batch() {
        let outcome = preflight(vec![
            AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(
                CapabilityStatus::TemporarilyUnavailable,
            ),
            AutomationPreflightBlock::ActionWriteUnavailable {
                feature: FeatureId::Performance,
                status: CapabilityStatus::BackendMissing,
            },
        ]);
        assert_eq!(
            classify_automation_retry(&outcome),
            AutomationRetryDisposition::AfterCapabilityRefresh(vec![
                AutomationRetryReason::RuntimeEvidenceTransient(
                    CapabilityStatus::TemporarilyUnavailable,
                ),
                AutomationRetryReason::ActionEvidenceTransient {
                    feature: FeatureId::Performance,
                    status: CapabilityStatus::BackendMissing,
                },
            ])
        );
    }

    #[test]
    fn one_terminal_block_makes_entire_batch_terminal() {
        let outcome = preflight(vec![
            AutomationPreflightBlock::ActionWriteUnavailable {
                feature: FeatureId::Performance,
                status: CapabilityStatus::TemporarilyUnavailable,
            },
            AutomationPreflightBlock::ActionWriteUnavailable {
                feature: FeatureId::GpuProductPolicy,
                status: CapabilityStatus::Unsupported,
            },
        ]);
        assert_eq!(
            classify_automation_retry(&outcome),
            AutomationRetryDisposition::Terminal
        );
    }

    #[test]
    fn permission_and_requirement_are_never_background_retry_reasons() {
        for block in [
            AutomationPreflightBlock::ActionWriteUnavailable {
                feature: FeatureId::Performance,
                status: CapabilityStatus::PermissionDenied,
            },
            AutomationPreflightBlock::ActionRequiresExplicitRequirement {
                feature: FeatureId::GpuProductPolicy,
            },
        ] {
            assert_eq!(
                classify_automation_retry(&preflight(vec![block])),
                AutomationRetryDisposition::Terminal
            );
        }
    }

    #[test]
    fn source_is_hardware_inert() {
        let source = include_str!("automation_retry.rs");
        for token in [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "performance("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["Command", "::new("].concat(),
        ] {
            assert!(!source.contains(&token), "unexpected execution surface: {token}");
        }
        assert!(!source.contains("unsafe"));
    }
}
