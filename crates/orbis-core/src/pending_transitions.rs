//! Pending transition registry.
//!
//! Pending transitions are not cleared by lifecycle events alone. Events only
//! identify entries that should be authoritatively re-observed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ActionRequirement, FeatureId, LifecycleEvent};

/// One feature transition waiting for an external requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTransition {
    /// Feature whose desired target remains pending.
    pub feature: FeatureId,
    /// Stable technical target summary.
    pub target: String,
    /// External requirement.
    pub requirement: ActionRequirement,
    /// Caller-supplied creation timestamp in Unix milliseconds.
    pub created_at_ms: u64,
}

/// Pending transition registry keyed by feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PendingTransitionRegistry {
    entries: BTreeMap<FeatureId, PendingTransition>,
}

impl PendingTransitionRegistry {
    /// Insert or replace one feature's pending transition.
    pub fn record(&mut self, transition: PendingTransition) {
        self.entries.insert(transition.feature, transition);
    }

    /// Borrow a pending transition.
    pub fn get(&self, feature: FeatureId) -> Option<&PendingTransition> {
        self.entries.get(&feature)
    }

    /// Remove a transition only after higher-layer authoritative observation
    /// proves that it is resolved or explicitly cancelled.
    pub fn resolve(&mut self, feature: FeatureId) -> Option<PendingTransition> {
        self.entries.remove(&feature)
    }

    /// Iterate in deterministic feature order.
    pub fn iter(&self) -> impl Iterator<Item = (&FeatureId, &PendingTransition)> {
        self.entries.iter()
    }

    /// Features that should be re-observed after a lifecycle event.
    ///
    /// This method never resolves an entry by itself.
    pub fn reobserve_candidates(&self, event: &LifecycleEvent) -> Vec<FeatureId> {
        self.entries
            .values()
            .filter(|transition| match (&transition.requirement, event) {
                (ActionRequirement::Reboot, LifecycleEvent::Startup) => true,
                (ActionRequirement::Logout, LifecycleEvent::Startup) => true,
                (
                    _,
                    LifecycleEvent::CapabilityChanged {
                        feature: changed_feature,
                    },
                ) => transition.feature == *changed_feature,
                (_, LifecycleEvent::BackendRecovered { .. }) => true,
                _ => false,
            })
            .map(|transition| transition.feature)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reboot_pending_is_reobserved_not_auto_resolved_on_startup() {
        let mut registry = PendingTransitionRegistry::default();
        registry.record(PendingTransition {
            feature: FeatureId::GpuMux,
            target: "ultimate".into(),
            requirement: ActionRequirement::Reboot,
            created_at_ms: 1,
        });

        assert_eq!(
            registry.reobserve_candidates(&LifecycleEvent::Startup),
            vec![FeatureId::GpuMux]
        );
        assert!(registry.get(FeatureId::GpuMux).is_some());
    }

    #[test]
    fn resume_does_not_claim_reboot_requirement_was_satisfied() {
        let mut registry = PendingTransitionRegistry::default();
        registry.record(PendingTransition {
            feature: FeatureId::GpuMux,
            target: "ultimate".into(),
            requirement: ActionRequirement::Reboot,
            created_at_ms: 1,
        });
        assert!(registry.reobserve_candidates(&LifecycleEvent::Resume).is_empty());
    }

    #[test]
    fn capability_change_only_reobserves_matching_feature() {
        let mut registry = PendingTransitionRegistry::default();
        registry.record(PendingTransition {
            feature: FeatureId::Performance,
            target: "turbo".into(),
            requirement: ActionRequirement::Confirmation,
            created_at_ms: 1,
        });
        registry.record(PendingTransition {
            feature: FeatureId::GpuMux,
            target: "ultimate".into(),
            requirement: ActionRequirement::Reboot,
            created_at_ms: 2,
        });

        assert_eq!(
            registry.reobserve_candidates(&LifecycleEvent::CapabilityChanged {
                feature: FeatureId::Performance
            }),
            vec![FeatureId::Performance]
        );
    }
}
