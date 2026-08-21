//! Policy-oriented automation.
//!
//! Unlike the legacy action-oriented rule type, this engine never executes a
//! command. A matching rule only selects a preset id, which is desired intent
//! consumed later by normal reconciliation.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::PowerSource;

/// Runtime event that may select a policy preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyEvent {
    /// Power source changed.
    PowerSourceChanged {
        /// Observed power source.
        source: PowerSource,
    },
    /// System resumed from sleep.
    Resume,
    /// Process started.
    ProcessStarted {
        /// Stable executable identity.
        executable: String,
    },
    /// Process stopped.
    ProcessStopped {
        /// Stable executable identity.
        executable: String,
    },
    /// GameMode activation changed.
    GameModeChanged {
        /// Whether GameMode is active.
        active: bool,
    },
    /// External display presence changed.
    ExternalDisplayChanged {
        /// Whether an external display is connected.
        connected: bool,
    },
}

/// Trigger predicate for a policy rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyTrigger {
    /// Match a specific power source transition.
    PowerSource {
        /// Required power source.
        source: PowerSource,
    },
    /// Match resume.
    Resume,
    /// Match process start.
    ProcessStarted {
        /// Required executable identity.
        executable: String,
    },
    /// Match process stop.
    ProcessStopped {
        /// Required executable identity.
        executable: String,
    },
    /// Match GameMode state.
    GameMode {
        /// Required GameMode state.
        active: bool,
    },
    /// Match external display presence.
    ExternalDisplay {
        /// Required external display state.
        connected: bool,
    },
}

/// Optional condition evaluated against already-observed runtime context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyCondition {
    /// Current power source must match.
    PowerSource {
        /// Required power source.
        source: PowerSource,
    },
    /// Process must currently be running.
    ProcessRunning {
        /// Required executable identity.
        executable: String,
    },
    /// GameMode state must match.
    GameMode {
        /// Required GameMode state.
        active: bool,
    },
    /// External display presence must match.
    ExternalDisplay {
        /// Required external display state.
        connected: bool,
    },
}

/// Observed context used only for rule matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyContext {
    /// Current power source.
    pub power_source: PowerSource,
    /// Running executables represented by stable names/paths chosen upstream.
    pub running_processes: BTreeSet<String>,
    /// Observed GameMode state.
    pub game_mode_active: bool,
    /// Whether at least one external display is connected.
    pub external_display_connected: bool,
}

impl Default for PolicyContext {
    fn default() -> Self {
        Self {
            power_source: PowerSource::Unknown,
            running_processes: BTreeSet::new(),
            game_mode_active: false,
            external_display_connected: false,
        }
    }
}

/// One automation rule that selects a preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Stable rule id.
    pub id: String,
    /// Trigger.
    pub trigger: PolicyTrigger,
    /// Additional observed conditions.
    pub conditions: Vec<PolicyCondition>,
    /// Preset id to select when matched.
    pub preset_id: String,
    /// Higher number wins when several rules match the same event.
    pub priority: u8,
    /// Rule enabled state.
    pub enabled: bool,
}

/// Result of one policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySelection {
    /// Rule that won.
    pub rule_id: String,
    /// Desired preset id.
    pub preset_id: String,
    /// Winning priority.
    pub priority: u8,
}

fn trigger_matches(trigger: &PolicyTrigger, event: &PolicyEvent) -> bool {
    match (trigger, event) {
        (
            PolicyTrigger::PowerSource { source: expected },
            PolicyEvent::PowerSourceChanged { source },
        ) => expected == source,
        (PolicyTrigger::Resume, PolicyEvent::Resume) => true,
        (
            PolicyTrigger::ProcessStarted {
                executable: expected,
            },
            PolicyEvent::ProcessStarted { executable },
        ) => expected == executable,
        (
            PolicyTrigger::ProcessStopped {
                executable: expected,
            },
            PolicyEvent::ProcessStopped { executable },
        ) => expected == executable,
        (PolicyTrigger::GameMode { active: expected }, PolicyEvent::GameModeChanged { active }) => {
            expected == active
        }
        (
            PolicyTrigger::ExternalDisplay {
                connected: expected,
            },
            PolicyEvent::ExternalDisplayChanged { connected },
        ) => expected == connected,
        _ => false,
    }
}

fn condition_matches(condition: &PolicyCondition, context: &PolicyContext) -> bool {
    match condition {
        PolicyCondition::PowerSource { source } => context.power_source == *source,
        PolicyCondition::ProcessRunning { executable } => {
            context.running_processes.contains(executable)
        }
        PolicyCondition::GameMode { active } => context.game_mode_active == *active,
        PolicyCondition::ExternalDisplay { connected } => {
            context.external_display_connected == *connected
        }
    }
}

/// Select the highest-priority matching preset.
///
/// Equal-priority ties are resolved by stable rule id to make the outcome
/// deterministic. No hardware action or command execution occurs here.
pub fn select_policy_preset(
    rules: &[PolicyRule],
    event: &PolicyEvent,
    context: &PolicyContext,
) -> Option<PolicySelection> {
    rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|rule| trigger_matches(&rule.trigger, event))
        .filter(|rule| {
            rule.conditions
                .iter()
                .all(|condition| condition_matches(condition, context))
        })
        .max_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| right.id.cmp(&left.id))
        })
        .map(|rule| PolicySelection {
            rule_id: rule.id.clone(),
            preset_id: rule.preset_id.clone(),
            priority: rule.priority,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highest_priority_matching_rule_selects_preset_only() {
        let rules = vec![
            PolicyRule {
                id: "battery-default".into(),
                trigger: PolicyTrigger::PowerSource {
                    source: PowerSource::Battery,
                },
                conditions: Vec::new(),
                preset_id: "silent".into(),
                priority: 10,
                enabled: true,
            },
            PolicyRule {
                id: "battery-game".into(),
                trigger: PolicyTrigger::PowerSource {
                    source: PowerSource::Battery,
                },
                conditions: vec![PolicyCondition::GameMode { active: true }],
                preset_id: "battery-gaming".into(),
                priority: 20,
                enabled: true,
            },
        ];
        let context = PolicyContext {
            power_source: PowerSource::Battery,
            game_mode_active: true,
            ..Default::default()
        };
        let selected = select_policy_preset(
            &rules,
            &PolicyEvent::PowerSourceChanged {
                source: PowerSource::Battery,
            },
            &context,
        )
        .unwrap();
        assert_eq!(selected.preset_id, "battery-gaming");
    }

    #[test]
    fn disabled_rule_never_matches() {
        let rules = vec![PolicyRule {
            id: "resume".into(),
            trigger: PolicyTrigger::Resume,
            conditions: Vec::new(),
            preset_id: "balanced".into(),
            priority: 255,
            enabled: false,
        }];
        assert!(
            select_policy_preset(&rules, &PolicyEvent::Resume, &PolicyContext::default()).is_none()
        );
    }

    #[test]
    fn equal_priority_tie_is_deterministic_by_rule_id() {
        let rules = vec![
            PolicyRule {
                id: "b".into(),
                trigger: PolicyTrigger::Resume,
                conditions: Vec::new(),
                preset_id: "two".into(),
                priority: 10,
                enabled: true,
            },
            PolicyRule {
                id: "a".into(),
                trigger: PolicyTrigger::Resume,
                conditions: Vec::new(),
                preset_id: "one".into(),
                priority: 10,
                enabled: true,
            },
        ];
        let selected =
            select_policy_preset(&rules, &PolicyEvent::Resume, &PolicyContext::default()).unwrap();
        assert_eq!(selected.rule_id, "a");
        assert_eq!(selected.preset_id, "one");
    }

    #[test]
    fn unknown_power_context_is_not_guessed() {
        let rules = vec![PolicyRule {
            id: "resume-on-ac".into(),
            trigger: PolicyTrigger::Resume,
            conditions: vec![PolicyCondition::PowerSource {
                source: PowerSource::Ac,
            }],
            preset_id: "performance".into(),
            priority: 10,
            enabled: true,
        }];
        assert!(
            select_policy_preset(&rules, &PolicyEvent::Resume, &PolicyContext::default()).is_none()
        );
    }
}
