//! Worker-side bookkeeping for AC / battery rules.
//!
//! The tracker only decides *which rule* to apply; the worker performs the
//! writes through the same authorised paths as a manual change. The first
//! observed power source after start never applies anything, and loading the
//! rules never writes hardware.

use std::path::PathBuf;

#[cfg(test)]
use orbis_config::load_power_rules_from_dir;
use orbis_config::{
    PowerRule, PowerRules, load_power_rules, save_power_rules, save_power_rules_to_dir,
};

/// What the UI shows about the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerRulesView {
    pub rules: PowerRules,
    pub error: Option<String>,
}

pub struct PowerRulesTracker {
    dir: Option<PathBuf>,
    rules: PowerRules,
    error: Option<String>,
    last_ac: Option<bool>,
}

impl PowerRulesTracker {
    pub fn load() -> Self {
        Self::from_load(None, load_power_rules().map_err(|e| e.to_string()))
    }

    #[cfg(test)]
    pub fn load_from_dir(dir: PathBuf) -> Self {
        let loaded = load_power_rules_from_dir(&dir).map_err(|e| e.to_string());
        Self::from_load(Some(dir), loaded)
    }

    fn from_load(dir: Option<PathBuf>, loaded: Result<PowerRules, String>) -> Self {
        let (rules, error) = match loaded {
            Ok(rules) => (rules, None),
            Err(message) => {
                tracing::warn!("power rules unreadable, rules disabled: {message}");
                (PowerRules::default(), Some(message))
            }
        };
        Self {
            dir,
            rules,
            error,
            last_ac: None,
        }
    }

    pub fn view(&self) -> PowerRulesView {
        PowerRulesView {
            rules: self.rules,
            error: self.error.clone(),
        }
    }

    /// Feed one observed power source. Returns the rule to apply when the source
    /// changed since the previous observation and rules are enabled.
    pub fn observe(&mut self, ac_online: bool) -> Option<PowerRule> {
        let previous = self.last_ac.replace(ac_online);
        if previous.is_none() || previous == Some(ac_online) {
            return None;
        }
        self.applicable(ac_online)
    }

    /// Store new rules. Returns the rule to apply now: the current source's rule
    /// when rules were just enabled or that rule was edited.
    pub fn set_rules(&mut self, new: PowerRules) -> Result<Option<PowerRule>, String> {
        let saved = match &self.dir {
            Some(dir) => save_power_rules_to_dir(&new, dir),
            None => save_power_rules(&new),
        };
        if let Err(error) = saved {
            let message = error.to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        let old = std::mem::replace(&mut self.rules, new);
        self.error = None;
        let Some(ac) = self.last_ac else {
            return Ok(None);
        };
        if !old.enabled || old.rule(ac) != new.rule(ac) {
            Ok(self.applicable(ac))
        } else {
            Ok(None)
        }
    }

    fn applicable(&self, ac: bool) -> Option<PowerRule> {
        let rule = self.rules.rule(ac);
        (self.rules.enabled && !rule.is_empty()).then_some(rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::profile::PerformanceProfile;

    fn rule(profile: PerformanceProfile) -> PowerRule {
        PowerRule {
            profile: Some(profile),
        }
    }

    fn rules(enabled: bool) -> PowerRules {
        PowerRules {
            enabled,
            ac: rule(PerformanceProfile::Turbo),
            battery: rule(PerformanceProfile::Silent),
        }
    }

    #[test]
    fn first_observation_and_unchanged_source_never_apply() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        tracker.set_rules(rules(true)).unwrap();
        assert_eq!(tracker.observe(true), None);
        assert_eq!(tracker.observe(true), None);
        assert_eq!(
            tracker.observe(false),
            Some(rule(PerformanceProfile::Silent))
        );
        assert_eq!(tracker.observe(true), Some(rule(PerformanceProfile::Turbo)));
    }

    #[test]
    fn disabled_rules_track_the_source_but_apply_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        tracker.observe(true);
        assert_eq!(tracker.observe(false), None);
        assert_eq!(
            tracker.set_rules(rules(true)).unwrap(),
            Some(rule(PerformanceProfile::Silent))
        );
    }

    #[test]
    fn editing_the_other_source_does_not_reapply_the_current_rule() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        tracker.set_rules(rules(true)).unwrap();
        tracker.observe(false);
        let mut edited = rules(true);
        edited.ac = rule(PerformanceProfile::Balanced);
        assert_eq!(tracker.set_rules(edited).unwrap(), None);
        edited.battery = rule(PerformanceProfile::Balanced);
        assert_eq!(
            tracker.set_rules(edited).unwrap(),
            Some(rule(PerformanceProfile::Balanced))
        );
    }

    #[test]
    fn rules_persist_and_reload_without_applying() {
        let dir = tempfile::tempdir().unwrap();
        PowerRulesTracker::load_from_dir(dir.path().into())
            .set_rules(rules(true))
            .unwrap();
        let mut reloaded = PowerRulesTracker::load_from_dir(dir.path().into());
        assert_eq!(reloaded.view().rules, rules(true));
        assert_eq!(reloaded.observe(false), None);
    }
}
