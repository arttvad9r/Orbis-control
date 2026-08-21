//! Restoration planning for temporary hardware/profile switches.
//!
//! Some firmware reads require a temporary mode/profile change. This module
//! makes restoration an explicit state-machine obligation instead of relying on
//! fallible straight-line code to remember the previous value.

use serde::{Deserialize, Serialize};

/// Restoration state for a temporary switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestorationState {
    /// No temporary switch has been attempted.
    Prepared,
    /// Temporary value is active; restoration is mandatory.
    TemporaryActive,
    /// Previous value was restored and verified.
    Restored,
    /// Restoration attempt failed or could not be verified.
    RestorationFailed {
        /// Technical reason.
        reason: String,
    },
}

/// Explicit temporary-switch plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorationPlan<T> {
    /// Authoritative value observed before the temporary switch.
    pub previous: T,
    /// Temporary target needed for the operation.
    pub temporary: T,
    /// Current restoration state.
    pub state: RestorationState,
}

impl<T> RestorationPlan<T> {
    /// Construct a plan from authoritative previous state.
    pub fn new(previous: T, temporary: T) -> Self {
        Self {
            previous,
            temporary,
            state: RestorationState::Prepared,
        }
    }

    /// Mark that the temporary state was successfully entered.
    pub fn mark_temporary_active(&mut self) {
        self.state = RestorationState::TemporaryActive;
    }

    /// Mark verified restoration.
    pub fn mark_restored(&mut self) {
        self.state = RestorationState::Restored;
    }

    /// Mark restoration failure.
    pub fn mark_restoration_failed(&mut self, reason: impl Into<String>) {
        self.state = RestorationState::RestorationFailed {
            reason: reason.into(),
        };
    }

    /// Whether callers still owe a restoration attempt.
    pub fn restoration_required(&self) -> bool {
        matches!(self.state, RestorationState::TemporaryActive)
    }
}

impl<T: PartialEq> RestorationPlan<T> {
    /// Feed authoritative observation after a restoration attempt.
    pub fn observe_restoration(&mut self, observed: &T) -> bool {
        if observed == &self.previous {
            self.mark_restored();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_activation_creates_explicit_restoration_debt() {
        let mut plan = RestorationPlan::new("balanced", "performance");
        assert!(!plan.restoration_required());
        plan.mark_temporary_active();
        assert!(plan.restoration_required());
    }

    #[test]
    fn only_authoritative_previous_value_clears_restoration_debt() {
        let mut plan = RestorationPlan::new(1u8, 2u8);
        plan.mark_temporary_active();
        assert!(!plan.observe_restoration(&2));
        assert!(plan.restoration_required());
        assert!(plan.observe_restoration(&1));
        assert_eq!(plan.state, RestorationState::Restored);
        assert!(!plan.restoration_required());
    }

    #[test]
    fn failed_restoration_remains_explicit() {
        let mut plan = RestorationPlan::new(1u8, 2u8);
        plan.mark_temporary_active();
        plan.mark_restoration_failed("backend disappeared");
        assert!(matches!(
            plan.state,
            RestorationState::RestorationFailed { .. }
        ));
    }
}
