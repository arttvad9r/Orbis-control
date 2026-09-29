//! NVIDIA discrete GPU tuning: core/memory clock offsets and power limit.
//!
//! NVML keeps these settings only until reboot or driver reload, so the values
//! are re-applied by the application from explicitly stored per-profile intent.

use serde::{Deserialize, Serialize};

/// One tunable NVIDIA setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NvidiaField {
    /// Graphics clock offset in MHz.
    CoreOffset,
    /// Memory clock offset in MHz.
    MemoryOffset,
    /// Board power limit in whole watts.
    PowerLimit,
}

impl NvidiaField {
    /// All fields in display order.
    pub const ALL: [NvidiaField; 3] = [Self::CoreOffset, Self::MemoryOffset, Self::PowerLimit];

    /// Stable D-Bus wire value.
    pub fn wire(self) -> u8 {
        match self {
            Self::CoreOffset => 0,
            Self::MemoryOffset => 1,
            Self::PowerLimit => 2,
        }
    }

    /// Inverse of [`NvidiaField::wire`].
    pub fn from_wire(raw: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.wire() == raw)
    }

    /// Short human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::CoreOffset => "смещение частоты ядра",
            Self::MemoryOffset => "смещение частоты памяти",
            Self::PowerLimit => "лимит мощности",
        }
    }
}

/// Observed value of one setting with the driver-reported bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NvidiaSetting {
    /// Current value (MHz or W).
    pub current: i32,
    /// Lowest accepted value.
    pub min: i32,
    /// Highest accepted value.
    pub max: i32,
    /// Driver default when known (power limit only).
    pub default: Option<i32>,
}

impl NvidiaSetting {
    /// True when `value` is inside the driver-reported bounds.
    pub fn accepts(&self, value: i32) -> bool {
        (self.min..=self.max).contains(&value)
    }
}

/// Why no setting could be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NvidiaAvailability {
    /// No NVIDIA GPU is bound to the proprietary driver (Eco mode, no dGPU).
    #[default]
    Absent,
    /// The GPU is runtime-suspended; it is not woken just to read a value.
    Asleep,
    /// The driver library or device could not be queried.
    Unreadable,
    /// Settings below were read from the driver.
    Ready,
}

/// Observed NVIDIA tuning state. A setting the driver does not expose is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NvidiaTuningState {
    /// Read availability.
    pub availability: NvidiaAvailability,
    /// Graphics clock offset.
    pub core: Option<NvidiaSetting>,
    /// Memory clock offset.
    pub memory: Option<NvidiaSetting>,
    /// Power limit; absent when the driver cannot report the current limit.
    pub power: Option<NvidiaSetting>,
    /// True when the privileged writer is reachable for this GPU.
    pub writable: bool,
}

impl NvidiaTuningState {
    /// Setting for a field.
    pub fn setting(&self, field: NvidiaField) -> Option<NvidiaSetting> {
        match field {
            NvidiaField::CoreOffset => self.core,
            NvidiaField::MemoryOffset => self.memory,
            NvidiaField::PowerLimit => self.power,
        }
    }

    /// True when at least one setting can be shown.
    pub fn has_settings(&self) -> bool {
        self.core.is_some() || self.memory.is_some() || self.power.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_values_round_trip() {
        for field in NvidiaField::ALL {
            assert_eq!(NvidiaField::from_wire(field.wire()), Some(field));
        }
        assert_eq!(NvidiaField::from_wire(3), None);
    }

    #[test]
    fn bounds_are_inclusive() {
        let setting = NvidiaSetting {
            current: 0,
            min: -1000,
            max: 1000,
            default: None,
        };
        assert!(setting.accepts(-1000) && setting.accepts(1000));
        assert!(!setting.accepts(1001));
    }
}
