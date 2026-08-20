//! Pure capability preflight for planned automation actions.
//!
//! Preflight is deliberately stricter than an interactive control. It accepts
//! no partial execution: every plan block and every action-level evidence gap
//! keeps the whole plan non-executable. This module performs no I/O and never
//! calls providers or workers.

use orbis_core::automation::AutomationAction;
use orbis_core::capability::{
    CapabilityConstraints, CapabilityStatus, DeviceCapabilities, FeatureId,
};

use crate::automation_policy::{AutomationPlan, AutomationPlanBlock};

/// Why a pure automation plan is not safe to hand to an executor yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPreflightBlock {
    /// A blocker already produced while converting desired policy into actions.
    Plan(AutomationPlanBlock),
    /// The Automation runtime itself has no proven write/execution capability.
    AutomationRuntimeWriteUnavailable(CapabilityStatus),
    /// An action's feature has no proven write capability.
    ActionWriteUnavailable {
        /// Feature required by the action.
        feature: FeatureId,
        /// Fresh operation-level write status observed by the caller.
        status: CapabilityStatus,
    },
    /// The feature can write only with an explicit requirement such as reboot.
    /// Unattended automation must not satisfy that requirement implicitly.
    ActionRequiresExplicitRequirement {
        /// Feature requiring explicit handling.
        feature: FeatureId,
    },
    /// The capability model does not carry typed target evidence required by
    /// this unattended action.
    TargetEvidenceMissing {
        /// Feature whose typed target evidence is missing.
        feature: FeatureId,
    },
    /// The requested target is absent from typed advertised constraints.
    TargetNotAdvertised {
        /// Feature whose constraints reject the requested target.
        feature: FeatureId,
    },
    /// `SetLighting(bool)` does not identify KeyboardBacklight vs Aura and is
    /// therefore too ambiguous for unattended execution.
    AmbiguousLightingTarget,
    /// Arbitrary custom commands are never accepted by this product-policy
    /// preflight, even though the core rule model can represent them.
    CustomCommandNotAllowed,
}

/// Result of capability preflight for one immutable plan/snapshot pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationPreflight {
    /// Original pure plan.
    pub plan: AutomationPlan,
    /// Complete set of blockers found from plan semantics and capability evidence.
    pub blocks: Vec<AutomationPreflightBlock>,
}

impl AutomationPreflight {
    /// Return actions only when the entire plan passed every preflight gate.
    ///
    /// Callers cannot use this helper to obtain a partial subset.
    pub fn actions_if_ready(&self) -> Option<&[AutomationAction]> {
        if self.blocks.is_empty() {
            Some(&self.plan.actions)
        } else {
            None
        }
    }

    /// Whether the plan is eligible to proceed to a future executor.
    pub fn is_ready(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// Run a pure fail-closed preflight against one immutable capability snapshot.
pub fn preflight_automation_plan(
    plan: AutomationPlan,
    capabilities: &DeviceCapabilities,
) -> AutomationPreflight {
    let mut blocks = plan
        .blocks
        .iter()
        .cloned()
        .map(AutomationPreflightBlock::Plan)
        .collect::<Vec<_>>();

    let runtime_status = write_status(capabilities, FeatureId::Automation);
    match runtime_status {
        CapabilityStatus::Supported => {}
        CapabilityStatus::SupportedWithRequirement => {
            blocks.push(AutomationPreflightBlock::ActionRequiresExplicitRequirement {
                feature: FeatureId::Automation,
            });
        }
        status => blocks.push(AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(
            status,
        )),
    }

    for action in &plan.actions {
        preflight_action(action, capabilities, &mut blocks);
    }

    AutomationPreflight { plan, blocks }
}

fn preflight_action(
    action: &AutomationAction,
    capabilities: &DeviceCapabilities,
    blocks: &mut Vec<AutomationPreflightBlock>,
) {
    match action {
        AutomationAction::SetProfile(profile) => {
            let feature = FeatureId::Performance;
            if !write_is_directly_supported(capabilities, feature, blocks) {
                return;
            }
            match capabilities.features.get(&feature).map(|cap| &cap.constraints) {
                Some(CapabilityConstraints::PerformanceProfiles(profiles)) => {
                    if !profiles.contains(profile) {
                        blocks.push(AutomationPreflightBlock::TargetNotAdvertised { feature });
                    }
                }
                _ => blocks.push(AutomationPreflightBlock::TargetEvidenceMissing { feature }),
            }
        }
        AutomationAction::SetGpuPolicy(mode) => {
            let feature = FeatureId::GpuProductPolicy;
            if !write_is_directly_supported(capabilities, feature, blocks) {
                return;
            }
            match capabilities.features.get(&feature).map(|cap| &cap.constraints) {
                Some(CapabilityConstraints::GpuModes(modes)) => {
                    if !modes.contains(mode) {
                        blocks.push(AutomationPreflightBlock::TargetNotAdvertised { feature });
                    }
                }
                _ => blocks.push(AutomationPreflightBlock::TargetEvidenceMissing { feature }),
            }
        }
        AutomationAction::SetRefreshPolicy(_) => {
            let feature = FeatureId::DisplayRefresh;
            if !write_is_directly_supported(capabilities, feature, blocks) {
                return;
            }
            // CapabilityConstraints currently has no typed display-refresh
            // choice model that can prove Auto/60/120. Do not infer support from
            // a generic write bit alone.
            blocks.push(AutomationPreflightBlock::TargetEvidenceMissing { feature });
        }
        AutomationAction::SetLighting(_) => {
            // The legacy core action does not identify which lighting owner it
            // means. Do not guess between KeyboardBacklight and Aura.
            blocks.push(AutomationPreflightBlock::AmbiguousLightingTarget);
        }
        AutomationAction::CustomCommand(_) => {
            blocks.push(AutomationPreflightBlock::CustomCommandNotAllowed);
        }
    }
}

fn write_is_directly_supported(
    capabilities: &DeviceCapabilities,
    feature: FeatureId,
    blocks: &mut Vec<AutomationPreflightBlock>,
) -> bool {
    match write_status(capabilities, feature) {
        CapabilityStatus::Supported => true,
        CapabilityStatus::SupportedWithRequirement => {
            blocks.push(AutomationPreflightBlock::ActionRequiresExplicitRequirement { feature });
            false
        }
        status => {
            blocks.push(AutomationPreflightBlock::ActionWriteUnavailable { feature, status });
            false
        }
    }
}

fn write_status(capabilities: &DeviceCapabilities, feature: FeatureId) -> CapabilityStatus {
    capabilities
        .features
        .get(&feature)
        .map(|capability| capability.operations.write.status)
        .unwrap_or(CapabilityStatus::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_policy::{
        AutomationPolicy, AutomationPowerSource, DesiredDisplayPolicy, DesiredGpuPolicy,
        DesiredPerformancePolicy,
    };
    use orbis_core::automation::AutomationTrigger;
    use orbis_core::capability::{
        Capability, CapabilityOperations, OperationCapability,
    };
    use orbis_core::gpu::GpuMode;
    use orbis_core::profile::PerformanceProfile;

    fn capability(
        write: CapabilityStatus,
        constraints: CapabilityConstraints,
    ) -> Capability {
        Capability::new(write)
            .with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(write),
            })
            .with_constraints(constraints)
    }

    fn enabled_ac_policy() -> AutomationPolicy {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.ac.performance =
            DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced);
        policy
    }

    #[test]
    fn unknown_automation_runtime_blocks_even_supported_action() {
        let policy = enabled_ac_policy();
        let plan = policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);
        let mut caps = DeviceCapabilities::default();
        caps.features.insert(
            FeatureId::Performance,
            capability(
                CapabilityStatus::Supported,
                CapabilityConstraints::PerformanceProfiles(vec![PerformanceProfile::Balanced]),
            ),
        );

        let result = preflight_automation_plan(plan, &caps);
        assert!(!result.is_ready());
        assert!(result.actions_if_ready().is_none());
        assert!(result.blocks.contains(
            &AutomationPreflightBlock::AutomationRuntimeWriteUnavailable(
                CapabilityStatus::Unknown
            )
        ));
    }

    #[test]
    fn supported_runtime_and_exact_performance_constraints_pass() {
        let policy = enabled_ac_policy();
        let plan = policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);
        let mut caps = DeviceCapabilities::default();
        caps.features.insert(
            FeatureId::Automation,
            capability(CapabilityStatus::Supported, CapabilityConstraints::None),
        );
        caps.features.insert(
            FeatureId::Performance,
            capability(
                CapabilityStatus::Supported,
                CapabilityConstraints::PerformanceProfiles(vec![PerformanceProfile::Balanced]),
            ),
        );

        let result = preflight_automation_plan(plan, &caps);
        assert!(result.is_ready());
        assert_eq!(
            result.actions_if_ready(),
            Some(&[AutomationAction::SetProfile(PerformanceProfile::Balanced)][..])
        );
    }

    #[test]
    fn supported_write_without_typed_target_evidence_stays_blocked() {
        let policy = enabled_ac_policy();
        let plan = policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);
        let mut caps = DeviceCapabilities::default();
        caps.features.insert(
            FeatureId::Automation,
            capability(CapabilityStatus::Supported, CapabilityConstraints::None),
        );
        caps.features.insert(
            FeatureId::Performance,
            capability(CapabilityStatus::Supported, CapabilityConstraints::Unknown),
        );

        let result = preflight_automation_plan(plan, &caps);
        assert!(result.blocks.contains(&AutomationPreflightBlock::TargetEvidenceMissing {
            feature: FeatureId::Performance,
        }));
        assert!(result.actions_if_ready().is_none());
    }

    #[test]
    fn gpu_requirement_or_missing_mode_blocks_whole_plan() {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.ac.gpu = DesiredGpuPolicy::Mode(GpuMode::Eco);
        let plan = policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);

        let mut caps = DeviceCapabilities::default();
        caps.features.insert(
            FeatureId::Automation,
            capability(CapabilityStatus::Supported, CapabilityConstraints::None),
        );
        caps.features.insert(
            FeatureId::GpuProductPolicy,
            capability(
                CapabilityStatus::SupportedWithRequirement,
                CapabilityConstraints::GpuModes(vec![GpuMode::Eco]),
            ),
        );

        let result = preflight_automation_plan(plan, &caps);
        assert!(result.blocks.contains(
            &AutomationPreflightBlock::ActionRequiresExplicitRequirement {
                feature: FeatureId::GpuProductPolicy,
            }
        ));
        assert!(result.actions_if_ready().is_none());
    }

    #[test]
    fn display_write_bit_alone_is_not_enough_for_unattended_target() {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.ac.display = DesiredDisplayPolicy::Hz120;
        let plan = policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);

        let mut caps = DeviceCapabilities::default();
        caps.features.insert(
            FeatureId::Automation,
            capability(CapabilityStatus::Supported, CapabilityConstraints::None),
        );
        caps.features.insert(
            FeatureId::DisplayRefresh,
            capability(CapabilityStatus::Supported, CapabilityConstraints::None),
        );

        let result = preflight_automation_plan(plan, &caps);
        assert!(result.blocks.contains(&AutomationPreflightBlock::TargetEvidenceMissing {
            feature: FeatureId::DisplayRefresh,
        }));
        assert!(result.actions_if_ready().is_none());
    }
}
