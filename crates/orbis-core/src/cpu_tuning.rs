//! CPU energy/performance preference (amd-pstate / intel_pstate EPP).

use serde::{Deserialize, Serialize};

/// Kernel EPP hint. `default`/`custom` are deliberately not selectable values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnergyPreference {
    /// `performance`.
    Performance,
    /// `balance_performance`.
    BalancePerformance,
    /// `balance_power`.
    BalancePower,
    /// `power`.
    Power,
}

impl EnergyPreference {
    /// All selectable values, from most to least performance.
    pub const ALL: [EnergyPreference; 4] = [
        Self::Performance,
        Self::BalancePerformance,
        Self::BalancePower,
        Self::Power,
    ];

    /// Exact sysfs token.
    pub fn sysfs(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::BalancePerformance => "balance_performance",
            Self::BalancePower => "balance_power",
            Self::Power => "power",
        }
    }

    /// Parse a sysfs token; `None` for `default`, `custom` or anything else.
    pub fn from_sysfs(raw: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|value| value.sysfs() == raw.trim())
    }

    /// Stable D-Bus wire value (1..=4); 0 is reserved for "not selectable".
    pub fn wire(self) -> u8 {
        match self {
            Self::Performance => 1,
            Self::BalancePerformance => 2,
            Self::BalancePower => 3,
            Self::Power => 4,
        }
    }

    /// Inverse of [`EnergyPreference::wire`].
    pub fn from_wire(raw: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.wire() == raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_and_sysfs_round_trip_and_reject_unselectable() {
        for value in EnergyPreference::ALL {
            assert_eq!(EnergyPreference::from_wire(value.wire()), Some(value));
            assert_eq!(EnergyPreference::from_sysfs(value.sysfs()), Some(value));
        }
        assert_eq!(EnergyPreference::from_wire(0), None);
        assert_eq!(EnergyPreference::from_wire(5), None);
        assert_eq!(EnergyPreference::from_sysfs("default"), None);
        assert_eq!(
            EnergyPreference::from_sysfs("balance_power\n"),
            Some(EnergyPreference::BalancePower)
        );
    }
}
