//! Typed application-owned automation desired state.
//!
//! This module defines persistence semantics only. It does not subscribe to
//! lifecycle events, call providers, reconcile hardware, or imply that a saved
//! policy is active. Execution remains a separate application-layer concern.

use serde::{Deserialize, Serialize};

use orbis_core::gpu::GpuMode;
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
}
