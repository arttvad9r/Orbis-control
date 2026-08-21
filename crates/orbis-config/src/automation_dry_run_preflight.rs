//! Hardware-inert Automation preflight used by shadow/worker dry-run stages.
//!
//! The strict execution preflight intentionally requires
//! `FeatureId::Automation.write == Supported`. That requirement is correct for
//! a real unattended mutation, but it would make a read-only shadow runtime
//! incapable of proving the rest of the pipeline before product promotion.
//!
//! This adapter therefore starts from the strict preflight and removes only the
//! Automation-runtime write blocker when the same immutable capability snapshot
//! proves `Automation.read == Supported`. Every plan blocker and every
//! action-level write/target/requirement blocker is preserved verbatim. It never
//! mutates a capability snapshot and cannot advertise Automation write support.

use orbis_core::capability::{CapabilityStatus, DeviceCapabilities, FeatureId};

use crate::{
    AutomationPlan, AutomationPreflight, AutomationPreflightBlock, preflight_automation_plan,
};

/// Run the hardware-inert shadow/dry-run preflight.
///
/// A result can be ready while `Automation.write` remains `Unsupported`, but
/// only when `Automation.read` is explicitly `Supported`. Such readiness is not
/// execution authority: a production executor must run the strict
/// [`preflight_automation_plan`] again on the current immutable snapshot before
/// its first mutation.
pub fn preflight_automation_plan_for_dry_run(
    plan: AutomationPlan,
    capabilities: &DeviceCapabilities,
) -> AutomationPreflight {
    let runtime_read = capabilities
        .features
        .get(&FeatureId::Automation)
        .map(|capability| capability.operations.read.status)
        .unwrap_or(CapabilityStatus::Unknown);

    let mut preflight = preflight_automation_plan(plan, capabilities);

    if runtime_read == CapabilityStatus::Supported {
        preflight.blocks.retain(|block| {
            !matches!(
                block,
                AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(_)
                    | AutomationPreflightBlock::ActionRequiresExplicitRequirement {
                        feature: FeatureId::Automation,
                    }
            )
        });
    } else if !preflight.blocks.iter().any(|block| {
        matches!(
            block,
            AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(_)
                | AutomationPreflightBlock::ActionRequiresExplicitRequirement {
                    feature: FeatureId::Automation,
                }
        )
    }) {
        // Compatibility representation: the public preflight block enum has a
        // runtime-write blocker but no separate runtime-read blocker. Keep the
        // result fail-closed without inventing a second parallel preflight DTO.
        preflight
            .blocks
            .push(AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(
                runtime_read,
            ));
    }

    preflight
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AutomationPolicy, AutomationPowerSource, DesiredPerformancePolicy};
    use orbis_core::automation::AutomationTrigger;
    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;

    fn capability(
        status: CapabilityStatus,
        read: CapabilityStatus,
        write: CapabilityStatus,
        constraints: CapabilityConstraints,
    ) -> Capability {
        Capability::new(status)
            .with_operations(CapabilityOperations {
                read: OperationCapability::new(read),
                write: OperationCapability::new(write),
            })
            .with_constraints(constraints)
    }

    fn performance_policy() -> AutomationPlan {
        let mut policy = AutomationPolicy {
            enabled: true,
            on_ac_change: true,
            ..Default::default()
        };
        policy.ac.performance = DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced);
        policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac)
    }

    #[test]
    fn read_only_automation_can_prove_dry_run_but_not_execution() {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.features.insert(
            FeatureId::Automation,
            capability(
                CapabilityStatus::ReadOnly,
                CapabilityStatus::Supported,
                CapabilityStatus::Unsupported,
                CapabilityConstraints::None,
            ),
        );
        capabilities.features.insert(
            FeatureId::Performance,
            capability(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityConstraints::PerformanceProfiles(vec![PerformanceProfile::Balanced]),
            ),
        );

        let plan = performance_policy();
        let strict = preflight_automation_plan(plan.clone(), &capabilities);
        assert!(!strict.is_ready());
        assert!(strict.blocks.iter().any(|block| matches!(
            block,
            AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(
                CapabilityStatus::Unsupported
            )
        )));

        let dry_run = preflight_automation_plan_for_dry_run(plan, &capabilities);
        assert!(dry_run.is_ready());
        assert_eq!(dry_run.actions_if_ready().unwrap().len(), 1);
    }

    #[test]
    fn action_write_blockers_are_never_removed() {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.features.insert(
            FeatureId::Automation,
            capability(
                CapabilityStatus::ReadOnly,
                CapabilityStatus::Supported,
                CapabilityStatus::Unsupported,
                CapabilityConstraints::None,
            ),
        );
        capabilities.features.insert(
            FeatureId::Performance,
            capability(
                CapabilityStatus::ReadOnly,
                CapabilityStatus::Supported,
                CapabilityStatus::Unsupported,
                CapabilityConstraints::PerformanceProfiles(vec![PerformanceProfile::Balanced]),
            ),
        );

        let result = preflight_automation_plan_for_dry_run(performance_policy(), &capabilities);
        assert!(!result.is_ready());
        assert!(result.blocks.iter().any(|block| matches!(
            block,
            AutomationPreflightBlock::ActionWriteUnavailable {
                feature: FeatureId::Performance,
                status: CapabilityStatus::Unsupported,
            }
        )));
    }

    #[test]
    fn missing_automation_read_evidence_remains_blocked() {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.features.insert(
            FeatureId::Performance,
            capability(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityConstraints::PerformanceProfiles(vec![PerformanceProfile::Balanced]),
            ),
        );

        let result = preflight_automation_plan_for_dry_run(performance_policy(), &capabilities);
        assert!(!result.is_ready());
    }
}
