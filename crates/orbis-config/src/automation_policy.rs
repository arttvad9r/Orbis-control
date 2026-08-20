//! Typed application-owned automation desired state and pure planning.
//!
//! Persistence and planning are hardware-inert. This module does not subscribe
//! to lifecycle events, call providers, reconcile hardware, or imply that a
//! saved policy is active. The planner only converts explicit desired intent
//! into typed core actions and reports anything it cannot represent exactly.

use serde::{Deserialize, Serialize};

use orbis_core::automation::{AutomationAction, AutomationTrigger, RefreshPolicy};
use orbis_core::gpu::GpuMode;
use orbis_core::newtypes::RefreshHz;
use orbis_core::profile::PerformanceProfile;

/// Desired Performance action for one power source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesiredPerformancePolicy {
    /// Do not change the current Performance profile.
    #[default]
    KeepCurrent,
    /// Apply one of the canonical three UI profiles when reconciliation exists.
    Profile(PerformanceProfile),
}

/// Desired GPU action for one power source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesiredGpuPolicy {
    /// Do not change the current GPU policy.
    #[default]
    KeepCurrent,
    /// Apply an explicitly selected product GPU mode when reconciliation exists.
    Mode(GpuMode),
}

/// Desired display-refresh policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesiredDisplayPolicy {
    /// Do not change the current display refresh state.
    #[default]
    KeepCurrent,
    /// Request the 60 Hz product preset.
    Hz60,
    /// Request the 120 Hz product preset.
    Hz120,
    /// Request automatic refresh policy.
    Auto,
}

/// Desired keyboard/Aura brightness policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesiredLightingPolicy {
    /// Do not change lighting.
    #[default]
    KeepCurrent,
    /// Lighting off.
    Off,
    /// Low/dim lighting.
    Dim,
    /// Normal product lighting level.
    Normal,
}

/// Desired settings associated with one power source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PowerAutomationPolicy {
    /// Performance policy.
    pub performance: DesiredPerformancePolicy,
    /// GPU product-mode policy.
    pub gpu: DesiredGpuPolicy,
    /// Display refresh policy.
    pub display: DesiredDisplayPolicy,
    /// Lighting policy.
    pub lighting: DesiredLightingPolicy,
}

/// Persisted automation policy draft.
///
/// `enabled` is desired intent only. It must never be interpreted as evidence
/// that lifecycle subscriptions or hardware reconciliation are running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationPolicy {
    /// Desired automation enable state. Runtime may still be unavailable.
    pub enabled: bool,
    /// AC policy draft.
    pub ac: PowerAutomationPolicy,
    /// Battery policy draft.
    pub battery: PowerAutomationPolicy,
    /// Reconcile after resume once lifecycle execution exists.
    pub on_resume: bool,
    /// Reconcile when the AC/battery source changes once execution exists.
    pub on_ac_change: bool,
    /// Avoid writes when authoritative state already equals desired state.
    pub reconcile_only: bool,
    /// Request user-facing transition notifications once execution exists.
    pub notify_transitions: bool,
}

impl Default for AutomationPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            ac: PowerAutomationPolicy::default(),
            battery: PowerAutomationPolicy::default(),
            on_resume: false,
            on_ac_change: false,
            reconcile_only: true,
            notify_transitions: false,
        }
    }
}

/// Current observed power source supplied to the pure planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationPowerSource {
    /// External AC power is active.
    Ac,
    /// The machine is currently running from battery.
    Battery,
}

/// A reason the desired policy cannot yet become an executable plan.
///
/// These are planning facts only. Capability/write evidence is intentionally
/// checked later by the application executor against a fresh registry snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPlanBlock {
    /// Persisted policy does not request automation execution.
    PolicyDisabled,
    /// The matching lifecycle trigger is disabled in desired state.
    TriggerDisabled,
    /// This typed policy does not define behavior for the supplied trigger.
    TriggerNotConfigured,
    /// The supplied source contradicts an AC/Battery transition trigger.
    PowerSourceMismatch,
    /// The current core action can represent only lighting off/on, not an exact
    /// Dim/Normal brightness target, so the planner refuses to coerce it.
    LightingLevelNotRepresentable(DesiredLightingPolicy),
}

/// Pure, non-executing automation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationPlan {
    /// Lifecycle event that caused planning.
    pub trigger: AutomationTrigger,
    /// Authoritative power-source observation used to select AC/Battery policy.
    pub power_source: AutomationPowerSource,
    /// Exact core actions derived from desired state.
    pub actions: Vec<AutomationAction>,
    /// Reasons the plan must not be executed as a complete policy.
    pub blocks: Vec<AutomationPlanBlock>,
    /// Desired optimization hint retained for the future executor.
    pub reconcile_only: bool,
    /// Desired notification hint retained for the future executor.
    pub notify_transitions: bool,
}

impl AutomationPlan {
    /// Whether the desired policy was represented without any blocking gap.
    ///
    /// This does not prove write capability. The future executor must still
    /// require fresh operation-level evidence and authoritative read-back.
    pub fn is_complete(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl AutomationPolicy {
    /// Build an execution-independent plan for one observed lifecycle event.
    ///
    /// The function performs no I/O and never invents a target. Unsupported
    /// triggers and non-representable desired values are returned as blockers.
    pub fn plan_for(
        &self,
        trigger: AutomationTrigger,
        power_source: AutomationPowerSource,
    ) -> AutomationPlan {
        let mut plan = AutomationPlan {
            trigger: trigger.clone(),
            power_source,
            actions: Vec::new(),
            blocks: Vec::new(),
            reconcile_only: self.reconcile_only,
            notify_transitions: self.notify_transitions,
        };

        if !self.enabled {
            plan.blocks.push(AutomationPlanBlock::PolicyDisabled);
            return plan;
        }

        let selected = match trigger {
            AutomationTrigger::OnAc => {
                if !self.on_ac_change {
                    plan.blocks.push(AutomationPlanBlock::TriggerDisabled);
                    return plan;
                }
                if power_source != AutomationPowerSource::Ac {
                    plan.blocks.push(AutomationPlanBlock::PowerSourceMismatch);
                    return plan;
                }
                &self.ac
            }
            AutomationTrigger::OnBattery => {
                if !self.on_ac_change {
                    plan.blocks.push(AutomationPlanBlock::TriggerDisabled);
                    return plan;
                }
                if power_source != AutomationPowerSource::Battery {
                    plan.blocks.push(AutomationPlanBlock::PowerSourceMismatch);
                    return plan;
                }
                &self.battery
            }
            AutomationTrigger::OnResume => {
                if !self.on_resume {
                    plan.blocks.push(AutomationPlanBlock::TriggerDisabled);
                    return plan;
                }
                match power_source {
                    AutomationPowerSource::Ac => &self.ac,
                    AutomationPowerSource::Battery => &self.battery,
                }
            }
            _ => {
                plan.blocks.push(AutomationPlanBlock::TriggerNotConfigured);
                return plan;
            }
        };

        match selected.performance {
            DesiredPerformancePolicy::KeepCurrent => {}
            DesiredPerformancePolicy::Profile(profile) => {
                plan.actions.push(AutomationAction::SetProfile(profile));
            }
        }

        match selected.gpu {
            DesiredGpuPolicy::KeepCurrent => {}
            DesiredGpuPolicy::Mode(mode) => {
                plan.actions.push(AutomationAction::SetGpuPolicy(mode));
            }
        }

        match selected.display {
            DesiredDisplayPolicy::KeepCurrent => {}
            DesiredDisplayPolicy::Auto => {
                plan.actions
                    .push(AutomationAction::SetRefreshPolicy(RefreshPolicy::Auto));
            }
            DesiredDisplayPolicy::Hz60 => {
                let hz = RefreshHz::new(60).expect("60 Hz is inside the validated RefreshHz range");
                plan.actions
                    .push(AutomationAction::SetRefreshPolicy(RefreshPolicy::Fixed(hz)));
            }
            DesiredDisplayPolicy::Hz120 => {
                let hz = RefreshHz::new(120).expect("120 Hz is inside the validated RefreshHz range");
                plan.actions
                    .push(AutomationAction::SetRefreshPolicy(RefreshPolicy::Fixed(hz)));
            }
        }

        match selected.lighting {
            DesiredLightingPolicy::KeepCurrent => {}
            DesiredLightingPolicy::Off => {
                plan.actions.push(AutomationAction::SetLighting(false));
            }
            value @ (DesiredLightingPolicy::Dim | DesiredLightingPolicy::Normal) => {
                plan.blocks
                    .push(AutomationPlanBlock::LightingLevelNotRepresentable(value));
            }
        }

        plan
    }
}

/// Current application-owned desired-state payload.
///
/// Keeping a root payload rather than persisting `AutomationPolicy` directly
/// leaves one owned document for future typed desired-state sections without
/// reviving the legacy `AppConfig` reconciliation surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OrbisDesiredState {
    /// Automation desired state.
    pub automation: AutomationPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DesiredStateDocument, load_desired_state_from_dir, save_desired_state_to_dir,
    };

    #[test]
    fn default_policy_is_hardware_inert() {
        let desired = OrbisDesiredState::default();
        assert!(!desired.automation.enabled);
        assert_eq!(
            desired.automation.ac.performance,
            DesiredPerformancePolicy::KeepCurrent
        );
        assert_eq!(desired.automation.ac.gpu, DesiredGpuPolicy::KeepCurrent);
        assert_eq!(
            desired.automation.ac.display,
            DesiredDisplayPolicy::KeepCurrent
        );
        assert_eq!(
            desired.automation.ac.lighting,
            DesiredLightingPolicy::KeepCurrent
        );
        assert!(!desired.automation.on_resume);
        assert!(!desired.automation.on_ac_change);
        assert!(desired.automation.reconcile_only);

        let plan = desired
            .automation
            .plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);
        assert_eq!(plan.blocks, vec![AutomationPlanBlock::PolicyDisabled]);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn typed_policy_roundtrip_uses_hardened_desired_state_store() {
        let td = tempfile::tempdir().expect("tempdir");
        let mut desired = OrbisDesiredState::default();
        desired.automation.ac.performance =
            DesiredPerformancePolicy::Profile(PerformanceProfile::Turbo);
        desired.automation.battery.gpu = DesiredGpuPolicy::Mode(GpuMode::Eco);
        desired.automation.battery.display = DesiredDisplayPolicy::Hz60;
        desired.automation.battery.lighting = DesiredLightingPolicy::Dim;
        desired.automation.on_resume = true;

        let document = DesiredStateDocument::new(desired.clone());
        save_desired_state_to_dir(&document, td.path()).expect("save desired state");
        let loaded = load_desired_state_from_dir::<OrbisDesiredState>(td.path())
            .expect("load desired state");

        assert!(loaded.warning.is_none());
        assert_eq!(loaded.state.desired(), &desired);
    }

    #[test]
    fn ac_plan_is_typed_and_hardware_inert() {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.ac.performance = DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced);
        policy.ac.gpu = DesiredGpuPolicy::Mode(GpuMode::Eco);
        policy.ac.display = DesiredDisplayPolicy::Hz60;
        policy.ac.lighting = DesiredLightingPolicy::Off;

        let plan = policy.plan_for(AutomationTrigger::OnAc, AutomationPowerSource::Ac);
        assert!(plan.is_complete());
        assert_eq!(
            plan.actions,
            vec![
                AutomationAction::SetProfile(PerformanceProfile::Balanced),
                AutomationAction::SetGpuPolicy(GpuMode::Eco),
                AutomationAction::SetRefreshPolicy(RefreshPolicy::Fixed(
                    RefreshHz::new(60).unwrap()
                )),
                AutomationAction::SetLighting(false),
            ]
        );
    }

    #[test]
    fn planner_refuses_to_coerce_dim_or_normal_to_boolean_lighting() {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_resume = true;
        policy.ac.lighting = DesiredLightingPolicy::Dim;

        let plan = policy.plan_for(AutomationTrigger::OnResume, AutomationPowerSource::Ac);
        assert!(!plan.is_complete());
        assert!(plan.actions.is_empty());
        assert_eq!(
            plan.blocks,
            vec![AutomationPlanBlock::LightingLevelNotRepresentable(
                DesiredLightingPolicy::Dim
            )]
        );
    }

    #[test]
    fn transition_trigger_must_match_observed_power_source() {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        policy.on_ac_change = true;
        policy.ac.performance = DesiredPerformancePolicy::Profile(PerformanceProfile::Turbo);

        let plan = policy.plan_for(
            AutomationTrigger::OnAc,
            AutomationPowerSource::Battery,
        );
        assert_eq!(plan.blocks, vec![AutomationPlanBlock::PowerSourceMismatch]);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn unsupported_lifecycle_trigger_never_reuses_another_policy() {
        let mut policy = AutomationPolicy::default();
        policy.enabled = true;
        let plan = policy.plan_for(
            AutomationTrigger::ExternalDisplayConnected,
            AutomationPowerSource::Ac,
        );
        assert_eq!(plan.blocks, vec![AutomationPlanBlock::TriggerNotConfigured]);
        assert!(plan.actions.is_empty());
    }
}
