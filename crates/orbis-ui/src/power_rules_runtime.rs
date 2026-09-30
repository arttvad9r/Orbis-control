//! Worker-side bookkeeping for AC / battery rules.
//!
//! The tracker only decides *which rule* to apply; the worker performs the
//! writes through the same authorised paths as a manual change. The first
//! observed power source after start never applies a profile or refresh rule,
//! and loading the rules never writes hardware. GPU "Optimized" is a mode
//! rather than a rule: it also reconciles at the first observation, because
//! the queued Eco/Standard target only takes effect after a reboot.

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

/// What one observed power source triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerObservation {
    pub rule: Option<PowerRule>,
    pub gpu_ac: Option<bool>,
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

    /// Feed one observed power source. The rule is set when the source changed
    /// since the previous observation and rules are enabled; `gpu_ac` when GPU
    /// Optimized is on and the source is new or changed.
    pub fn observe(&mut self, ac_online: bool) -> PowerObservation {
        let previous = self.last_ac.replace(ac_online);
        let gpu_ac = (self.rules.gpu_optimized && previous != Some(ac_online)).then_some(ac_online);
        let rule = if previous.is_none() || previous == Some(ac_online) {
            None
        } else {
            self.applicable(ac_online)
        };
        PowerObservation { rule, gpu_ac }
    }

    /// Currently stored rules.
    pub fn rules(&self) -> PowerRules {
        self.rules
    }

    /// Last observed power source, if any.
    pub fn last_ac(&self) -> Option<bool> {
        self.last_ac
    }

    /// Store new rules. Returns the rule to apply now: the current source's rule
    /// when rules were just enabled or that rule was edited.
    pub fn set_rules(&mut self, new: PowerRules) -> Result<Option<PowerRule>, String> {
        // The pending full-charge restore belongs to the worker, not the editor.
        let new = PowerRules {
            full_charge_restore: self.rules.full_charge_restore,
            ..new
        };
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

    /// Remember the limit to restore and persist it before the limit is
    /// raised, so a crash or restart still restores it later.
    pub fn begin_full_charge(&mut self, restore_to: u8) -> Result<(), String> {
        let rules = PowerRules {
            full_charge_restore: Some(restore_to),
            ..self.rules
        };
        self.store(rules)
    }

    /// The limit to restore when a one-time full charge should end now: the
    /// battery reports full, or the charger is gone. Clears the pending state
    /// (the restore is attempted once; its outcome is reported, never retried).
    pub fn full_charge_due(&mut self, ac_online: Option<bool>, battery_full: bool) -> Option<u8> {
        let restore = self.rules.full_charge_restore?;
        let due = battery_full || ac_online == Some(false);
        if !due {
            return None;
        }
        self.finish_full_charge().ok().flatten().or(Some(restore))
    }

    /// End a one-time full charge now; returns the limit to restore.
    pub fn finish_full_charge(&mut self) -> Result<Option<u8>, String> {
        let Some(restore) = self.rules.full_charge_restore else {
            return Ok(None);
        };
        let rules = PowerRules {
            full_charge_restore: None,
            ..self.rules
        };
        self.store(rules).map(|()| Some(restore))
    }

    fn store(&mut self, rules: PowerRules) -> Result<(), String> {
        let saved = match &self.dir {
            Some(dir) => save_power_rules_to_dir(&rules, dir),
            None => save_power_rules(&rules),
        };
        match saved {
            Ok(()) => {
                self.rules = rules;
                self.error = None;
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                self.error = Some(message.clone());
                Err(message)
            }
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

    #[test]
    fn full_charge_restores_once_when_full_or_unplugged_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().to_path_buf());
        tracker.begin_full_charge(80).unwrap();
        assert_eq!(
            tracker.full_charge_due(Some(true), false),
            None,
            "still charging"
        );

        // Restart keeps the pending restore.
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().to_path_buf());
        assert_eq!(tracker.rules().full_charge_restore, Some(80));
        assert_eq!(
            tracker.full_charge_due(Some(true), true),
            Some(80),
            "battery full"
        );
        assert_eq!(
            tracker.full_charge_due(Some(false), true),
            None,
            "restored once"
        );

        tracker.begin_full_charge(60).unwrap();
        assert_eq!(
            tracker.full_charge_due(Some(false), false),
            Some(60),
            "unplugged"
        );

        tracker.begin_full_charge(70).unwrap();
        assert_eq!(tracker.finish_full_charge(), Ok(Some(70)), "cancel");
        assert_eq!(tracker.finish_full_charge(), Ok(None));
    }

    #[test]
    fn editing_rules_keeps_a_pending_full_charge() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().to_path_buf());
        tracker.begin_full_charge(80).unwrap();
        tracker.set_rules(PowerRules::default()).unwrap();
        assert_eq!(tracker.rules().full_charge_restore, Some(80));
    }
    use orbis_core::profile::PerformanceProfile;

    fn rule(profile: PerformanceProfile) -> PowerRule {
        PowerRule {
            profile: Some(profile),
            refresh_hz: None,
        }
    }

    fn rules(enabled: bool) -> PowerRules {
        PowerRules {
            enabled,
            ac: rule(PerformanceProfile::Turbo),
            battery: rule(PerformanceProfile::Silent),
            gpu_optimized: false,
            full_charge_restore: None,
        }
    }

    #[test]
    fn first_observation_and_unchanged_source_never_apply() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        tracker.set_rules(rules(true)).unwrap();
        assert_eq!(tracker.observe(true).rule, None);
        assert_eq!(tracker.observe(true).rule, None);
        assert_eq!(
            tracker.observe(false).rule,
            Some(rule(PerformanceProfile::Silent))
        );
        assert_eq!(
            tracker.observe(true).rule,
            Some(rule(PerformanceProfile::Turbo))
        );
    }

    #[test]
    fn disabled_rules_track_the_source_but_apply_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        tracker.observe(true);
        assert_eq!(tracker.observe(false).rule, None);
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
        assert_eq!(reloaded.observe(false).rule, None);
    }

    #[test]
    fn gpu_optimized_reconciles_on_first_observation_and_on_change_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        let mut optimized = rules(false);
        optimized.gpu_optimized = true;
        tracker.set_rules(optimized).unwrap();
        assert_eq!(tracker.observe(false).gpu_ac, Some(false));
        assert_eq!(tracker.observe(false).gpu_ac, None);
        assert_eq!(tracker.observe(true).gpu_ac, Some(true));
        assert_eq!(tracker.observe(true).rule, None);
    }

    #[test]
    fn gpu_optimized_off_never_asks_for_a_gpu_switch() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = PowerRulesTracker::load_from_dir(dir.path().into());
        assert_eq!(tracker.observe(false).gpu_ac, None);
        assert_eq!(tracker.observe(true).gpu_ac, None);
    }
}
