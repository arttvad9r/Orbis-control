//! Validated product request for a future DisplayRefresh mutation owner.
//!
//! A request can be constructed only from typed [`DisplayRefreshConstraints`]
//! that already prove the internal-panel role and the exact product preset
//! target. The opaque target ID is preserved verbatim; no `wl_output` name or
//! connector guess is accepted here.

use crate::display_refresh::{
    DisplayRefreshConstraints, DisplayRefreshPreset, DisplayRefreshPresetTarget,
    DisplayRefreshTargetId,
};

/// One validated DisplayRefresh mutation request.
///
/// Fields are private so callers cannot pair an arbitrary target ID with an
/// arbitrary refresh value. The future provider must still re-read its own
/// target/evidence immediately before mutation because compositor topology can
/// change after this request was constructed.
#[derive(Debug, PartialEq, Eq)]
pub struct DisplayRefreshRequest {
    target: DisplayRefreshTargetId,
    preset_target: DisplayRefreshPresetTarget,
}

impl DisplayRefreshRequest {
    /// Construct a request only from a writable target present in frozen typed
    /// constraints.
    pub fn from_constraints(
        constraints: &DisplayRefreshConstraints,
        preset: DisplayRefreshPreset,
    ) -> Option<Self> {
        let preset_target = constraints.writable_target_for(preset)?;
        Some(Self {
            target: constraints.target.clone(),
            preset_target,
        })
    }

    /// Opaque compositor-owner target identity.
    pub fn target(&self) -> &DisplayRefreshTargetId {
        &self.target
    }

    /// Exact product preset target proven by the source constraints.
    pub fn preset_target(&self) -> DisplayRefreshPresetTarget {
        self.preset_target
    }

    /// Product preset represented by the exact request target.
    pub fn preset(&self) -> DisplayRefreshPreset {
        self.preset_target.preset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display_refresh::{DisplayRefreshPresetTarget, DisplayRefreshTargetRole};
    use crate::newtypes::RefreshMilliHz;

    fn constraints(role: DisplayRefreshTargetRole) -> DisplayRefreshConstraints {
        DisplayRefreshConstraints {
            target: DisplayRefreshTargetId::new("owner:panel-0"),
            role,
            targets: vec![
                DisplayRefreshPresetTarget::Auto,
                DisplayRefreshPresetTarget::Hz60 {
                    refresh: RefreshMilliHz::new(59_940).unwrap(),
                },
                DisplayRefreshPresetTarget::Hz120 {
                    refresh: RefreshMilliHz::new(119_880).unwrap(),
                },
            ],
        }
    }

    #[test]
    fn request_preserves_opaque_target_and_exact_refresh() {
        let request = DisplayRefreshRequest::from_constraints(
            &constraints(DisplayRefreshTargetRole::InternalPanelProven),
            DisplayRefreshPreset::Hz120,
        )
        .expect("proven request");
        assert_eq!(request.target().as_str(), "owner:panel-0");
        assert_eq!(request.preset(), DisplayRefreshPreset::Hz120);
        assert_eq!(
            request.preset_target(),
            DisplayRefreshPresetTarget::Hz120 {
                refresh: RefreshMilliHz::new(119_880).unwrap(),
            }
        );
    }

    #[test]
    fn external_or_unknown_target_cannot_construct_request() {
        for role in [
            DisplayRefreshTargetRole::External,
            DisplayRefreshTargetRole::Unknown,
        ] {
            assert_eq!(
                DisplayRefreshRequest::from_constraints(
                    &constraints(role),
                    DisplayRefreshPreset::Hz60,
                ),
                None
            );
        }
    }

    #[test]
    fn absent_preset_cannot_be_invented() {
        let constraints = DisplayRefreshConstraints {
            target: DisplayRefreshTargetId::new("owner:panel-0"),
            role: DisplayRefreshTargetRole::InternalPanelProven,
            targets: vec![DisplayRefreshPresetTarget::Hz120 {
                refresh: RefreshMilliHz::new(120_000).unwrap(),
            }],
        };
        assert_eq!(
            DisplayRefreshRequest::from_constraints(&constraints, DisplayRefreshPreset::Hz60),
            None
        );
    }
}
