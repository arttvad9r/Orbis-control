//! Backend-neutral read/staged contract for supergfxd 5.2.7.

use orbis_core::gpu::GpuPowerState;

/// Exact supergfxd backend mode, separate from product `GpuMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupergfxdMode {
    /// Hybrid/Optimus.
    Hybrid,
    /// Integrated GPU only.
    Integrated,
    /// NVIDIA without modeset.
    NvidiaNoModeset,
    /// VFIO.
    Vfio,
    /// ASUS eGPU.
    AsusEgpu,
    /// ASUS MUX discrete GPU.
    AsusMuxDgpu,
    /// No mode.
    None,
    /// Future wire value.
    Unknown(u32),
}

impl SupergfxdMode {
    /// Decode a supergfxd 5.2.7 mode wire discriminant.
    pub fn from_wire(raw: u32) -> Self {
        match raw {
            0 => Self::Hybrid,
            1 => Self::Integrated,
            2 => Self::NvidiaNoModeset,
            3 => Self::Vfio,
            4 => Self::AsusEgpu,
            5 => Self::AsusMuxDgpu,
            6 => Self::None,
            other => Self::Unknown(other),
        }
    }
}

/// Decode the supergfxd `Power()` wire value without collapsing unknown backend
/// states into a known physical power state.
///
/// Live/read-path evidence establishes only 0=Active, 1=Suspended and 2=Off.
/// Backend-specific/future values (including AsusDisabled/Unknown variants)
/// remain `GpuPowerState::Unknown` until independently proven.
pub fn power_from_wire(raw: u32) -> GpuPowerState {
    match raw {
        0 => GpuPowerState::Active,
        1 => GpuPowerState::Suspended,
        2 => GpuPowerState::Off,
        _ => GpuPowerState::Unknown,
    }
}

/// Exact supergfxd user action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupergfxdUserAction {
    /// Logout required.
    Logout,
    /// Reboot required.
    Reboot,
    /// Switch to Integrated first.
    SwitchToIntegrated,
    /// Disable ASUS eGPU.
    AsusEgpuDisable,
    /// No action required.
    Nothing,
    /// Future wire value.
    Unknown(u32),
}

impl SupergfxdUserAction {
    /// Decode a supergfxd 5.2.7 action wire discriminant.
    pub fn from_wire(raw: u32) -> Self {
        match raw {
            0 => Self::Logout,
            1 => Self::Reboot,
            2 => Self::SwitchToIntegrated,
            3 => Self::AsusEgpuDisable,
            4 => Self::Nothing,
            other => Self::Unknown(other),
        }
    }
}

/// Read-only supergfxd staged snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupergfxdSnapshot {
    /// Current backend mode.
    pub current_mode: SupergfxdMode,
    /// Pending backend mode.
    pub pending_mode: SupergfxdMode,
    /// Pending user action.
    pub pending_user_action: SupergfxdUserAction,
    /// Current power state.
    pub power: GpuPowerState,
    /// Supported backend modes.
    pub supported_modes: Vec<SupergfxdMode>,
}

/// Pure staged-state classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupergfxdStagedState {
    /// Applied with no pending state.
    Applied,
    /// Pending without a user action.
    Pending,
    /// Pending and requiring a user action.
    RequiresUserAction(SupergfxdUserAction),
    /// Contradictory or unknown state.
    Inconsistent,
}

/// Classify a requested backend mode against a fresh snapshot.
pub fn classify_supergfxd_state(
    requested: SupergfxdMode,
    snapshot: &SupergfxdSnapshot,
) -> SupergfxdStagedState {
    let known = !matches!(requested, SupergfxdMode::Unknown(_))
        && !matches!(snapshot.current_mode, SupergfxdMode::Unknown(_))
        && !matches!(snapshot.pending_mode, SupergfxdMode::Unknown(_))
        && !matches!(
            snapshot.pending_user_action,
            SupergfxdUserAction::Unknown(_)
        );
    let pending_none = snapshot.pending_mode == SupergfxdMode::None;
    let action_nothing = snapshot.pending_user_action == SupergfxdUserAction::Nothing;
    let pending_matches = snapshot.pending_mode == requested;
    if !known
        || (!action_nothing && pending_none)
        || (!pending_none && !pending_matches)
        || (snapshot.current_mode == requested && !pending_none)
    {
        return SupergfxdStagedState::Inconsistent;
    }
    if snapshot.current_mode == requested && pending_none && action_nothing {
        return SupergfxdStagedState::Applied;
    }
    if pending_matches {
        return if action_nothing {
            SupergfxdStagedState::Pending
        } else {
            SupergfxdStagedState::RequiresUserAction(snapshot.pending_user_action)
        };
    }
    SupergfxdStagedState::Inconsistent
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(
        current: SupergfxdMode,
        pending: SupergfxdMode,
        action: SupergfxdUserAction,
    ) -> SupergfxdSnapshot {
        SupergfxdSnapshot {
            current_mode: current,
            pending_mode: pending,
            pending_user_action: action,
            power: GpuPowerState::Suspended,
            supported_modes: vec![
                SupergfxdMode::Hybrid,
                SupergfxdMode::Integrated,
                SupergfxdMode::AsusMuxDgpu,
            ],
        }
    }

    #[test]
    fn wire_mappings_are_exact_and_future_safe() {
        assert_eq!(SupergfxdMode::from_wire(0), SupergfxdMode::Hybrid);
        assert_eq!(SupergfxdMode::from_wire(1), SupergfxdMode::Integrated);
        assert_eq!(SupergfxdMode::from_wire(2), SupergfxdMode::NvidiaNoModeset);
        assert_eq!(SupergfxdMode::from_wire(3), SupergfxdMode::Vfio);
        assert_eq!(SupergfxdMode::from_wire(4), SupergfxdMode::AsusEgpu);
        assert_eq!(SupergfxdMode::from_wire(5), SupergfxdMode::AsusMuxDgpu);
        assert_eq!(SupergfxdMode::from_wire(6), SupergfxdMode::None);
        assert_eq!(SupergfxdMode::from_wire(99), SupergfxdMode::Unknown(99));
        assert_eq!(
            SupergfxdUserAction::from_wire(0),
            SupergfxdUserAction::Logout
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(1),
            SupergfxdUserAction::Reboot
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(2),
            SupergfxdUserAction::SwitchToIntegrated
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(3),
            SupergfxdUserAction::AsusEgpuDisable
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(4),
            SupergfxdUserAction::Nothing
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(99),
            SupergfxdUserAction::Unknown(99)
        );
    }

    #[test]
    fn power_wire_mapping_preserves_unknown_states() {
        assert_eq!(power_from_wire(0), GpuPowerState::Active);
        assert_eq!(power_from_wire(1), GpuPowerState::Suspended);
        assert_eq!(power_from_wire(2), GpuPowerState::Off);
        for raw in [3, 4, 99] {
            assert_eq!(power_from_wire(raw), GpuPowerState::Unknown);
        }
    }

    #[test]
    fn staged_invariants_are_preserved() {
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::Integrated,
                &s(
                    SupergfxdMode::Integrated,
                    SupergfxdMode::None,
                    SupergfxdUserAction::Nothing
                )
            ),
            SupergfxdStagedState::Applied
        );
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::Integrated,
                &s(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::Integrated,
                    SupergfxdUserAction::Logout
                )
            ),
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Logout)
        );
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::AsusMuxDgpu,
                &s(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::AsusMuxDgpu,
                    SupergfxdUserAction::Reboot
                )
            ),
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Reboot)
        );
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::Hybrid,
                &s(
                    SupergfxdMode::Integrated,
                    SupergfxdMode::Hybrid,
                    SupergfxdUserAction::Nothing
                )
            ),
            SupergfxdStagedState::Pending
        );
        for state in [
            s(
                SupergfxdMode::Hybrid,
                SupergfxdMode::AsusMuxDgpu,
                SupergfxdUserAction::Nothing,
            ),
            s(
                SupergfxdMode::Hybrid,
                SupergfxdMode::None,
                SupergfxdUserAction::Logout,
            ),
            s(
                SupergfxdMode::Integrated,
                SupergfxdMode::Hybrid,
                SupergfxdUserAction::Nothing,
            ),
            s(
                SupergfxdMode::Hybrid,
                SupergfxdMode::Integrated,
                SupergfxdUserAction::Unknown(9),
            ),
            s(
                SupergfxdMode::Unknown(9),
                SupergfxdMode::None,
                SupergfxdUserAction::Nothing,
            ),
        ] {
            assert_eq!(
                classify_supergfxd_state(SupergfxdMode::Integrated, &state),
                SupergfxdStagedState::Inconsistent
            );
        }
    }
}
