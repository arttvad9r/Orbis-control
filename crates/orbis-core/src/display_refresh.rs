//! Product-level Display refresh intent/evidence.
//!
//! This is deliberately separate from the read-only core `wl_output` model.
//! `DisplayOutputId` must not be promoted into a mutation target because the
//! Wayland protocol explicitly does not guarantee that `wl_output.name` maps to
//! a DRM connector. A future compositor-specific mutation owner supplies its
//! own opaque runtime target ID and proves whether that target is the internal
//! panel.

use serde::{Deserialize, Serialize};

use crate::display_output::DisplayMode;
use crate::newtypes::RefreshMilliHz;

/// User/product refresh preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayRefreshPreset {
    /// Let the compositor/product backend choose according to its own policy.
    Auto,
    /// 60-Hz-class target. The exact mode can be 59.94/60.00/etc and is carried
    /// separately as lossless mHz evidence.
    Hz60,
    /// 120-Hz-class target. Exact refresh is carried separately in mHz.
    Hz120,
}

/// Opaque runtime mutation-target identity owned by a compositor backend.
///
/// This value is intentionally not constructively related to `DisplayOutputId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DisplayRefreshTargetId(String);

impl DisplayRefreshTargetId {
    /// Construct an opaque target ID supplied by the mutation owner.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the owner-supplied runtime identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Evidence about what kind of display target a compositor owner resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayRefreshTargetRole {
    /// The owner has positively identified this target as the built-in panel.
    InternalPanelProven,
    /// The owner has positively identified an external display.
    External,
    /// Target role is not proven.
    Unknown,
}

/// Exact target behind one product preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DisplayRefreshPresetTarget {
    /// Compositor-specific automatic policy; there is no exact mHz target.
    Auto,
    /// Exact compositor mode selected for a 60-Hz-class preset.
    Hz60 {
        /// Lossless refresh target in mHz.
        refresh: RefreshMilliHz,
    },
    /// Exact compositor mode selected for a 120-Hz-class preset.
    Hz120 {
        /// Lossless refresh target in mHz.
        refresh: RefreshMilliHz,
    },
}

impl DisplayRefreshPresetTarget {
    /// Product preset represented by this exact target.
    pub fn preset(self) -> DisplayRefreshPreset {
        match self {
            Self::Auto => DisplayRefreshPreset::Auto,
            Self::Hz60 { .. } => DisplayRefreshPreset::Hz60,
            Self::Hz120 { .. } => DisplayRefreshPreset::Hz120,
        }
    }

    /// Exact refresh target when the preset maps to a fixed compositor mode.
    pub fn exact_refresh(self) -> Option<RefreshMilliHz> {
        match self {
            Self::Auto => None,
            Self::Hz60 { refresh } | Self::Hz120 { refresh } => Some(refresh),
        }
    }
}

/// One frozen target/evidence snapshot supplied to future Display mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayRefreshEvidence {
    /// Opaque mutation target owned by the compositor backend.
    pub target: DisplayRefreshTargetId,
    /// Whether the target is positively identified as the internal panel.
    pub role: DisplayRefreshTargetRole,
    /// Current compositor mode at evidence collection time.
    pub current: DisplayMode,
    /// Exact product preset targets proven by backend/observed mode evidence.
    pub targets: Vec<DisplayRefreshPresetTarget>,
}

impl DisplayRefreshEvidence {
    /// Build evidence from one owner-provided target plus observed modes.
    ///
    /// `available_modes` is evidence only when supplied by the compositor. The
    /// function never interprets an absent mode as unsupported. Fixed targets
    /// are derived only from modes with the same hardware width/height as the
    /// current mode, preventing a refresh preset from silently changing
    /// resolution. `auto_supported` must come from the compositor mutation
    /// owner; core `wl_output` observation cannot prove Auto semantics.
    pub fn from_observed_modes(
        target: DisplayRefreshTargetId,
        role: DisplayRefreshTargetRole,
        current: DisplayMode,
        available_modes: &[DisplayMode],
        auto_supported: bool,
    ) -> Self {
        let mut candidates = Vec::with_capacity(available_modes.len() + 1);
        candidates.push(current);
        candidates.extend(
            available_modes
                .iter()
                .copied()
                .filter(|mode| mode.width == current.width && mode.height == current.height),
        );

        let mut targets = Vec::new();
        if auto_supported {
            targets.push(DisplayRefreshPresetTarget::Auto);
        }
        if let Some(refresh) = closest_refresh(&candidates, 60_000, 59_000, 61_000) {
            targets.push(DisplayRefreshPresetTarget::Hz60 { refresh });
        }
        if let Some(refresh) = closest_refresh(&candidates, 120_000, 118_000, 122_000) {
            targets.push(DisplayRefreshPresetTarget::Hz120 { refresh });
        }

        Self {
            target,
            role,
            current,
            targets,
        }
    }

    /// Return exact evidence for a preset, regardless of target role.
    pub fn target_for(&self, preset: DisplayRefreshPreset) -> Option<DisplayRefreshPresetTarget> {
        self.targets
            .iter()
            .copied()
            .find(|target| target.preset() == preset)
    }

    /// Return a target only when both preset evidence and internal-panel
    /// identity are proven. This is the minimum product-level gate for future
    /// Quick Control writes; compositor capability evidence is still required
    /// separately.
    pub fn writable_target_for(
        &self,
        preset: DisplayRefreshPreset,
    ) -> Option<DisplayRefreshPresetTarget> {
        if self.role != DisplayRefreshTargetRole::InternalPanelProven {
            return None;
        }
        self.target_for(preset)
    }
}

fn closest_refresh(
    modes: &[DisplayMode],
    center_mhz: u32,
    min_mhz: u32,
    max_mhz: u32,
) -> Option<RefreshMilliHz> {
    modes
        .iter()
        .map(|mode| mode.refresh)
        .filter(|refresh| {
            let value = refresh.get();
            value != 0 && (min_mhz..=max_mhz).contains(&value)
        })
        .min_by_key(|refresh| refresh.get().abs_diff(center_mhz))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: u32, height: u32, refresh_mhz: u32) -> DisplayMode {
        DisplayMode::new(
            width,
            height,
            RefreshMilliHz::new(refresh_mhz).expect("refresh"),
        )
    }

    #[test]
    fn exact_refresh_is_preserved_while_mapping_product_presets() {
        let evidence = DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new("owner-target-1"),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(2560, 1600, 59_940),
            &[mode(2560, 1600, 119_880)],
            false,
        );

        assert_eq!(
            evidence.target_for(DisplayRefreshPreset::Hz60),
            Some(DisplayRefreshPresetTarget::Hz60 {
                refresh: RefreshMilliHz::new(59_940).unwrap(),
            })
        );
        assert_eq!(
            evidence.target_for(DisplayRefreshPreset::Hz120),
            Some(DisplayRefreshPresetTarget::Hz120 {
                refresh: RefreshMilliHz::new(119_880).unwrap(),
            })
        );
    }

    #[test]
    fn different_resolution_modes_never_become_refresh_only_targets() {
        let evidence = DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new("panel"),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(1920, 1080, 60_000),
            &[mode(2560, 1600, 120_000)],
            false,
        );
        assert!(evidence.target_for(DisplayRefreshPreset::Hz60).is_some());
        assert_eq!(evidence.target_for(DisplayRefreshPreset::Hz120), None);
    }

    #[test]
    fn missing_non_current_modes_never_invents_unseen_preset() {
        let evidence = DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new("panel"),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(2560, 1600, 120_000),
            &[],
            false,
        );
        assert_eq!(evidence.target_for(DisplayRefreshPreset::Hz60), None);
        assert!(evidence.target_for(DisplayRefreshPreset::Hz120).is_some());
    }

    #[test]
    fn unknown_or_external_target_is_never_writable_even_with_mode_evidence() {
        for role in [
            DisplayRefreshTargetRole::Unknown,
            DisplayRefreshTargetRole::External,
        ] {
            let evidence = DisplayRefreshEvidence::from_observed_modes(
                DisplayRefreshTargetId::new("target"),
                role,
                mode(2560, 1600, 60_000),
                &[mode(2560, 1600, 120_000)],
                true,
            );
            assert!(evidence.target_for(DisplayRefreshPreset::Hz60).is_some());
            assert_eq!(evidence.writable_target_for(DisplayRefreshPreset::Hz60), None);
            assert_eq!(evidence.writable_target_for(DisplayRefreshPreset::Auto), None);
        }
    }

    #[test]
    fn auto_is_never_inferred_from_wl_output_mode_observation() {
        let without_owner_evidence = DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new("panel"),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(2560, 1600, 60_000),
            &[mode(2560, 1600, 120_000)],
            false,
        );
        assert_eq!(
            without_owner_evidence.target_for(DisplayRefreshPreset::Auto),
            None
        );

        let with_owner_evidence = DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new("panel"),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(2560, 1600, 60_000),
            &[mode(2560, 1600, 120_000)],
            true,
        );
        assert_eq!(
            with_owner_evidence.writable_target_for(DisplayRefreshPreset::Auto),
            Some(DisplayRefreshPresetTarget::Auto)
        );
    }

    #[test]
    fn closest_observed_mode_wins_without_rounding_the_target() {
        let evidence = DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new("panel"),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(2560, 1600, 59_940),
            &[mode(2560, 1600, 60_010), mode(2560, 1600, 60_500)],
            false,
        );
        assert_eq!(
            evidence.target_for(DisplayRefreshPreset::Hz60),
            Some(DisplayRefreshPresetTarget::Hz60 {
                refresh: RefreshMilliHz::new(60_010).unwrap(),
            })
        );
    }
}
