//! Structured readiness and dependency evidence.
//!
//! Readiness is intentionally separate from observed hardware values. It
//! explains whether dependencies and permissions required for a feature are
//! currently usable, and why not.

use serde::{Deserialize, Serialize};

use crate::FeatureId;

/// Runtime state of one dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    /// Dependency is reachable and its required contract is available.
    Ready,
    /// Dependency exists but is currently inactive.
    Inactive,
    /// Dependency is not installed/present.
    NotAvailable,
    /// Dependency should exist but could not be reached within its contract.
    Unreachable,
    /// Dependency is not relevant on this machine/configuration.
    NotRelevant,
    /// Evidence is insufficient for a trustworthy classification.
    Unknown,
}

/// Permission evidence, deliberately independent from dependency state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    /// This readiness check does not require permission.
    NotRequired,
    /// Required permission is proven available.
    Granted,
    /// Required permission is proven denied.
    Denied,
    /// Permission evidence is unavailable.
    Unknown,
}

/// Structured readiness record for one dependency/backend/tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessItem {
    /// Stable diagnostic identifier such as `asusd` or `supergfxd`.
    pub id: String,
    /// Current dependency state.
    pub state: ReadinessState,
    /// Features for which this dependency matters.
    pub required_for: Vec<FeatureId>,
    /// Human-readable technical evidence; not localized UI copy.
    pub evidence: Vec<String>,
    /// Permission evidence kept separate from dependency reachability.
    pub permission: PermissionState,
}

impl ReadinessItem {
    /// Whether this item currently blocks the specified feature.
    pub fn blocks(&self, feature: FeatureId) -> bool {
        if !self.required_for.contains(&feature) {
            return false;
        }

        let dependency_ok = matches!(
            self.state,
            ReadinessState::Ready | ReadinessState::NotRelevant
        );
        let permission_ok = !matches!(self.permission, PermissionState::Denied | PermissionState::Unknown);
        !(dependency_ok && permission_ok)
    }
}

/// Whole readiness publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReadinessReport {
    /// Independent dependency records.
    pub items: Vec<ReadinessItem>,
}

impl ReadinessReport {
    /// Return all readiness records that currently block a feature.
    pub fn blockers_for(&self, feature: FeatureId) -> Vec<&ReadinessItem> {
        self.items.iter().filter(|item| item.blocks(feature)).collect()
    }

    /// Whether every relevant dependency is currently ready for a feature.
    pub fn is_ready_for(&self, feature: FeatureId) -> bool {
        self.blockers_for(feature).is_empty()
    }
}

/// One ownership claim for a mutually exclusive hardware/service resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipClaim {
    /// Resource identifier, for example `fan-control`.
    pub resource: String,
    /// Owner/backend identifier.
    pub owner: String,
    /// Whether the owner is currently active.
    pub active: bool,
}

/// Detected active ownership conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipConflict {
    /// Resource with multiple active owners.
    pub resource: String,
    /// Active owners in deterministic input order.
    pub owners: Vec<String>,
}

/// Detect multiple active owners for the same resource.
///
/// This is pure diagnostics: it never stops, starts or reconfigures another
/// service automatically.
pub fn detect_ownership_conflicts(claims: &[OwnershipClaim]) -> Vec<OwnershipConflict> {
    use std::collections::BTreeMap;

    let mut by_resource: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for claim in claims.iter().filter(|claim| claim.active) {
        by_resource
            .entry(claim.resource.as_str())
            .or_default()
            .push(claim.owner.clone());
    }

    by_resource
        .into_iter()
        .filter_map(|(resource, owners)| {
            (owners.len() > 1).then(|| OwnershipConflict {
                resource: resource.to_string(),
                owners,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        id: &str,
        state: ReadinessState,
        permission: PermissionState,
    ) -> ReadinessItem {
        ReadinessItem {
            id: id.into(),
            state,
            required_for: vec![FeatureId::GpuMux],
            evidence: Vec::new(),
            permission,
        }
    }

    #[test]
    fn permission_denial_blocks_even_when_dependency_is_ready() {
        let report = ReadinessReport {
            items: vec![item(
                "supergfxd",
                ReadinessState::Ready,
                PermissionState::Denied,
            )],
        };
        assert!(!report.is_ready_for(FeatureId::GpuMux));
        assert_eq!(report.blockers_for(FeatureId::GpuMux).len(), 1);
    }

    #[test]
    fn unrelated_dependency_does_not_block_feature() {
        let mut unrelated = item(
            "supergfxd",
            ReadinessState::Unreachable,
            PermissionState::Unknown,
        );
        unrelated.required_for = vec![FeatureId::GpuAccess];
        let report = ReadinessReport {
            items: vec![unrelated],
        };
        assert!(report.is_ready_for(FeatureId::GpuMux));
    }

    #[test]
    fn not_relevant_with_no_permission_requirement_is_non_blocking() {
        let report = ReadinessReport {
            items: vec![item(
                "nvidia-smi",
                ReadinessState::NotRelevant,
                PermissionState::NotRequired,
            )],
        };
        assert!(report.is_ready_for(FeatureId::GpuMux));
    }

    #[test]
    fn conflict_detector_is_read_only_and_deterministic() {
        let conflicts = detect_ownership_conflicts(&[
            OwnershipClaim {
                resource: "fan-control".into(),
                owner: "asusd".into(),
                active: true,
            },
            OwnershipClaim {
                resource: "fan-control".into(),
                owner: "other-daemon".into(),
                active: true,
            },
            OwnershipClaim {
                resource: "gpu".into(),
                owner: "supergfxd".into(),
                active: true,
            },
        ]);
        assert_eq!(
            conflicts,
            vec![OwnershipConflict {
                resource: "fan-control".into(),
                owners: vec!["asusd".into(), "other-daemon".into()],
            }]
        );
    }
}
