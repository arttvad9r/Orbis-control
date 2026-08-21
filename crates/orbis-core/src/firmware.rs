//! Typed ASUS firmware-attribute states that are not display-specific.

use serde::{Deserialize, Serialize};

/// BIOS/POST boot sound state.
///
/// The modern kernel `asus-armoury` ABI exposes `boot_sound/current_value` as
/// the boolean enumeration `0;1`: zero disables the POST sound and one enables
/// it. Read failure/absence must not be represented as `Disabled`; callers use
/// their provider error/readiness channel for unavailable evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootSoundState {
    /// POST sound is disabled (`current_value == 0`).
    Disabled,
    /// POST sound is enabled (`current_value == 1`).
    Enabled,
}

impl BootSoundState {
    /// Strictly decode the kernel boolean wire. Unknown values are rejected so
    /// malformed firmware evidence can never silently become a default state.
    pub fn from_kernel_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Disabled),
            1 => Some(Self::Enabled),
            _ => None,
        }
    }

    /// Exact kernel wire value.
    pub const fn kernel_value(self) -> u32 {
        match self {
            Self::Disabled => 0,
            Self::Enabled => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_sound_wire_is_total_only_for_boolean_values() {
        assert_eq!(
            BootSoundState::from_kernel_value(0),
            Some(BootSoundState::Disabled)
        );
        assert_eq!(
            BootSoundState::from_kernel_value(1),
            Some(BootSoundState::Enabled)
        );
        assert_eq!(BootSoundState::from_kernel_value(2), None);
        assert_eq!(BootSoundState::Enabled.kernel_value(), 1);
    }
}
