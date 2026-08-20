//! Typed Desired-State payload for preset/policy-controlled hardware intent.
//!
//! This type is intended to be stored through the existing hardened
//! `DesiredStateDocument<T>` persistence. Applying a preset here only updates
//! Desired values; it never invokes a provider or hardware mutation.

use serde::{Deserialize, Serialize};

use orbis_core::{
    AsusdFanProfile, DesiredValue, GpuMode, Percent, PerformanceProfile, Preset, PresetIntent,
    RefreshHz,
};

/// How fields omitted by a preset affect existing Desired ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnspecifiedPresetFieldPolicy {
    /// Preserve the currently owned Desired value.
    Preserve,
    /// Explicitly release Orbis ownership for an omitted field.
    Unset,
}

/// Application-owned desired policy values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PolicyDesiredState {
    /// Selected preset id for UI/audit correlation. This is intent metadata,
    /// not proof that all preset fields are applied.
    pub selected_preset: DesiredValue<String>,
    /// Desired performance profile.
    pub performance: DesiredValue<PerformanceProfile>,
    /// Desired product GPU policy.
    pub gpu_mode: DesiredValue<GpuMode>,
    /// Desired battery charge limit.
    pub charge_limit: DesiredValue<Percent>,
    /// Desired display refresh rate.
    pub display_refresh: DesiredValue<RefreshHz>,
    /// Desired ASUS fan profile identity.
    pub fan_profile: DesiredValue<AsusdFanProfile>,
}

impl PolicyDesiredState {
    /// Apply one inert preset to Desired state.
    ///
    /// This is configuration transformation only. `selected_preset` records
    /// which preset generated the intent but does not imply convergence.
    pub fn apply_preset(
        &mut self,
        preset: &Preset,
        unspecified: UnspecifiedPresetFieldPolicy,
    ) {
        self.selected_preset = DesiredValue::Set(preset.id.clone());
        apply_optional(&mut self.performance, preset.intent.performance, unspecified);
        apply_optional(&mut self.gpu_mode, preset.intent.gpu_mode, unspecified);
        apply_optional(
            &mut self.charge_limit,
            preset.intent.charge_limit,
            unspecified,
        );
        apply_optional(
            &mut self.display_refresh,
            preset.intent.display_refresh,
            unspecified,
        );
        apply_optional(
            &mut self.fan_profile,
            preset.intent.fan_profile,
            unspecified,
        );
    }

    /// Clear selected-preset metadata without changing the feature Desired
    /// values it previously produced.
    pub fn clear_selected_preset(&mut self) {
        self.selected_preset = DesiredValue::Unset;
    }
}

fn apply_optional<T: Clone>(
    destination: &mut DesiredValue<T>,
    source: Option<T>,
    unspecified: UnspecifiedPresetFieldPolicy,
) {
    match source {
        Some(value) => *destination = DesiredValue::Set(value),
        None if unspecified == UnspecifiedPresetFieldPolicy::Unset => {
            *destination = DesiredValue::Unset;
        }
        None => {}
    }
}

/// Build a one-off preset from a Desired-state subset.
///
/// Useful for export/UI previews. Unset values become omitted preset fields;
/// no hardware state is consulted.
pub fn preset_intent_from_desired(state: &PolicyDesiredState) -> PresetIntent {
    PresetIntent {
        performance: desired_option(&state.performance),
        gpu_mode: desired_option(&state.gpu_mode),
        charge_limit: desired_option(&state.charge_limit),
        display_refresh: desired_option(&state.display_refresh),
        fan_profile: desired_option(&state.fan_profile),
    }
}

fn desired_option<T: Clone>(value: &DesiredValue<T>) -> Option<T> {
    match value {
        DesiredValue::Unset => None,
        DesiredValue::Set(value) => Some(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partial_preset() -> Preset {
        Preset {
            id: "battery".into(),
            name: "Battery".into(),
            intent: PresetIntent {
                performance: Some(PerformanceProfile::Silent),
                charge_limit: Some(Percent::new(80).unwrap()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn preserve_policy_changes_only_fields_present_in_preset() {
        let mut state = PolicyDesiredState {
            gpu_mode: DesiredValue::Set(GpuMode::Standard),
            display_refresh: DesiredValue::Set(RefreshHz::new(165).unwrap()),
            ..Default::default()
        };
        state.apply_preset(&partial_preset(), UnspecifiedPresetFieldPolicy::Preserve);

        assert_eq!(
            state.performance,
            DesiredValue::Set(PerformanceProfile::Silent)
        );
        assert_eq!(state.gpu_mode, DesiredValue::Set(GpuMode::Standard));
        assert_eq!(
            state.display_refresh,
            DesiredValue::Set(RefreshHz::new(165).unwrap())
        );
        assert_eq!(state.charge_limit, DesiredValue::Set(Percent::new(80).unwrap()));
        assert_eq!(state.selected_preset, DesiredValue::Set("battery".into()));
    }

    #[test]
    fn unset_policy_releases_omitted_fields_explicitly() {
        let mut state = PolicyDesiredState {
            gpu_mode: DesiredValue::Set(GpuMode::Standard),
            display_refresh: DesiredValue::Set(RefreshHz::new(165).unwrap()),
            ..Default::default()
        };
        state.apply_preset(&partial_preset(), UnspecifiedPresetFieldPolicy::Unset);
        assert_eq!(state.gpu_mode, DesiredValue::Unset);
        assert_eq!(state.display_refresh, DesiredValue::Unset);
    }

    #[test]
    fn desired_state_can_be_stored_by_existing_generic_document() {
        let mut state = PolicyDesiredState::default();
        state.apply_preset(&partial_preset(), UnspecifiedPresetFieldPolicy::Preserve);
        let document = crate::DesiredStateDocument::new(state.clone());
        let toml = toml::to_string(&document).unwrap();
        let back: crate::DesiredStateDocument<PolicyDesiredState> = toml::from_str(&toml).unwrap();
        assert_eq!(back.desired(), &state);
    }

    #[test]
    fn exporting_intent_does_not_infer_observed_state() {
        let state = PolicyDesiredState {
            performance: DesiredValue::Set(PerformanceProfile::Turbo),
            ..Default::default()
        };
        let intent = preset_intent_from_desired(&state);
        assert_eq!(intent.performance, Some(PerformanceProfile::Turbo));
        assert_eq!(intent.gpu_mode, None);
    }
}
