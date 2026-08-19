//! Pure desired/observed/pending domain state.
//!
//! This module contains no provider calls, persistence, timers, or
//! reconciliation execution. Transitions are explicit and inert.

use serde::{Deserialize, Serialize};

use crate::ActionRequirement;

/// Desired value selected by higher-level policy or user intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DesiredValue<T> {
    /// No desired value is currently owned.
    #[default]
    Unset,
    /// A concrete value is desired.
    Set(T),
}

/// Authoritatively observed runtime value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ObservedValue<T> {
    /// No authoritative observation is currently available.
    #[default]
    Unknown,
    /// A concrete value has been observed.
    Known(T),
}

/// Explicit pending target awaiting a later requirement or confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingValue<T> {
    /// Target value associated with the pending transition.
    pub target: T,
    /// Requirement that must be satisfied or acknowledged.
    pub requirement: ActionRequirement,
}

impl<T> PendingValue<T> {
    /// Construct a typed pending target.
    pub fn new(target: T, requirement: ActionRequirement) -> Self {
        Self { target, requirement }
    }
}

/// Independent desired, observed and pending state for one typed setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DesiredObservedState<T> {
    /// Desired intent; never inferred from observed state.
    pub desired: DesiredValue<T>,
    /// Authoritative observation; never inferred from desired state.
    pub observed: ObservedValue<T>,
    /// Explicit pending transition, if one has been recorded.
    pub pending: Option<PendingValue<T>>,
}

impl<T> DesiredObservedState<T> {
    /// Record a desired value without mutating observation or pending state.
    pub fn set_desired(&mut self, value: T) {
        self.desired = DesiredValue::Set(value);
    }

    /// Clear desired intent without mutating observation or pending state.
    pub fn clear_desired(&mut self) {
        self.desired = DesiredValue::Unset;
    }

    /// Record an authoritative observation without reconciling anything.
    pub fn set_observed(&mut self, value: T) {
        self.observed = ObservedValue::Known(value);
    }

    /// Forget the current observation without changing intent or pending state.
    pub fn clear_observed(&mut self) {
        self.observed = ObservedValue::Unknown;
    }

    /// Record a pending target explicitly.
    pub fn set_pending(&mut self, pending: PendingValue<T>) {
        self.pending = Some(pending);
    }

    /// Clear pending state explicitly.
    pub fn clear_pending(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_defaults_to_unset_unknown_and_not_pending() {
        let state = DesiredObservedState::<u8>::default();
        assert_eq!(state.desired, DesiredValue::Unset);
        assert_eq!(state.observed, ObservedValue::Unknown);
        assert_eq!(state.pending, None);
    }

    #[test]
    fn desired_transition_does_not_claim_observation_or_pending() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        assert_eq!(state.desired, DesiredValue::Set(80));
        assert_eq!(state.observed, ObservedValue::Unknown);
        assert_eq!(state.pending, None);
    }

    #[test]
    fn observation_never_auto_clears_pending_even_when_values_match() {
        let mut state = DesiredObservedState::<u8>::default();
        state.set_desired(80);
        state.set_pending(PendingValue::new(80, ActionRequirement::None));
        state.set_observed(80);
        assert_eq!(state.observed, ObservedValue::Known(80));
        assert_eq!(state.pending, Some(PendingValue::new(80, ActionRequirement::None)));
    }

    #[test]
    fn explicit_clear_operations_are_field_local() {
        let mut state = DesiredObservedState::<u8> {
            desired: DesiredValue::Set(60),
            observed: ObservedValue::Known(55),
            pending: Some(PendingValue::new(60, ActionRequirement::Reboot)),
        };
        state.clear_pending();
        assert_eq!(state.desired, DesiredValue::Set(60));
        assert_eq!(state.observed, ObservedValue::Known(55));
        assert_eq!(state.pending, None);
        state.clear_observed();
        assert_eq!(state.desired, DesiredValue::Set(60));
        assert_eq!(state.observed, ObservedValue::Unknown);
        state.clear_desired();
        assert_eq!(state.desired, DesiredValue::Unset);
    }

    #[test]
    fn serde_keeps_desired_observed_and_pending_distinct() {
        let state = DesiredObservedState {
            desired: DesiredValue::Set(2u8),
            observed: ObservedValue::Known(1u8),
            pending: Some(PendingValue::new(2u8, ActionRequirement::Logout)),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: DesiredObservedState<u8> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }
}
