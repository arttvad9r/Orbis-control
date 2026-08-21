//! Authoritative applied-state model for compositor-owned DisplayRefresh.
//!
//! Supported-target evidence and applied state are deliberately separate.
//! `DisplayRefreshEvidence` can prove that Auto/60/120 targets exist, but the
//! current mode alone cannot prove whether an automatic compositor policy is
//! actually active. A mutation owner must therefore publish this additional
//! applied-state snapshot for post-write read-back.

use serde::{Deserialize, Serialize};

use crate::display_output::DisplayMode;
use crate::display_refresh::{DisplayRefreshTargetId, DisplayRefreshTargetRole};
use crate::newtypes::RefreshMilliHz;

/// Authoritative policy currently active for one compositor-owned target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DisplayRefreshActivePolicy {
    /// Compositor/product automatic refresh policy is positively reported active.
    Auto,
    /// A fixed refresh operation is positively reported active at this exact
    /// lossless mHz value.
    Fixed {
        /// Exact active fixed refresh value.
        refresh: RefreshMilliHz,
    },
    /// The owner can read current mode but cannot prove Auto-vs-fixed policy.
    /// This state is useful for observation but is insufficient for mutation
    /// success confirmation.
    Unknown,
}

/// One fresh authoritative post-operation DisplayRefresh state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayRefreshAppliedState {
    /// Opaque target identity owned by the same compositor adapter that owns
    /// mutation.
    pub target: DisplayRefreshTargetId,
    /// Fresh owner-proven target role.
    pub role: DisplayRefreshTargetRole,
    /// Current compositor mode after the operation/read.
    pub current: DisplayMode,
    /// Authoritative active refresh policy. `Unknown` never confirms mutation
    /// success.
    pub policy: DisplayRefreshActivePolicy,
}

impl DisplayRefreshAppliedState {
    /// Whether the target is still positively identified as the internal panel.
    pub fn internal_panel_is_proven(&self) -> bool {
        self.role == DisplayRefreshTargetRole::InternalPanelProven
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(refresh: u32) -> DisplayMode {
        DisplayMode::new(2560, 1600, RefreshMilliHz::new(refresh).expect("refresh"))
    }

    #[test]
    fn unknown_policy_is_distinct_from_auto_and_fixed() {
        assert_ne!(
            DisplayRefreshActivePolicy::Unknown,
            DisplayRefreshActivePolicy::Auto
        );
        assert_ne!(
            DisplayRefreshActivePolicy::Unknown,
            DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(60_000).unwrap(),
            }
        );
    }

    #[test]
    fn applied_state_keeps_target_role_mode_and_policy_separate() {
        let state = DisplayRefreshAppliedState {
            target: DisplayRefreshTargetId::new("owner:panel-0"),
            role: DisplayRefreshTargetRole::InternalPanelProven,
            current: mode(119_880),
            policy: DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(119_880).unwrap(),
            },
        };
        assert!(state.internal_panel_is_proven());
        assert_eq!(state.target.as_str(), "owner:panel-0");
        assert_eq!(state.current.refresh.get(), 119_880);
    }

    #[test]
    fn external_target_never_reports_internal_proof() {
        let state = DisplayRefreshAppliedState {
            target: DisplayRefreshTargetId::new("owner:external-0"),
            role: DisplayRefreshTargetRole::External,
            current: mode(60_000),
            policy: DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(60_000).unwrap(),
            },
        };
        assert!(!state.internal_panel_is_proven());
    }
}
