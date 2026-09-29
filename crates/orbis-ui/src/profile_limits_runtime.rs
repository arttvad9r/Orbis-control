//! Worker-side bookkeeping for per-profile power limits.
//!
//! The tracker only decides *what* should be re-applied when the observed
//! performance profile changes; the worker performs the writes through the same
//! authorised power-limit path as a manual apply. Loading never writes hardware.

use std::path::PathBuf;

#[cfg(test)]
use orbis_config::load_profile_limits_from_dir;
use orbis_config::{
    ProfileLimits, load_profile_limits, save_profile_limits, save_profile_limits_to_dir,
};
use orbis_core::cpu_tuning::EnergyPreference;
use orbis_core::limits::PowerLimitField;
use orbis_core::nvidia_tuning::NvidiaField;
use orbis_core::profile::PerformanceProfile;

/// What the UI shows about the currently active profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLimitsView {
    pub profile: PerformanceProfile,
    pub auto_apply: bool,
    pub saved: Vec<(PowerLimitField, i32)>,
    pub epp: Option<EnergyPreference>,
    pub cpu_boost: Option<bool>,
    pub nvidia: Vec<(NvidiaField, i32)>,
    pub curve_optimizer: Option<i32>,
    pub error: Option<String>,
}

/// Everything to re-apply after a profile change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileReplay {
    pub limits: Vec<(PowerLimitField, i32)>,
    pub epp: Option<EnergyPreference>,
    pub cpu_boost: Option<bool>,
    pub nvidia: Vec<(NvidiaField, i32)>,
    pub curve_optimizer: Option<i32>,
}

impl ProfileReplay {
    pub fn is_empty(&self) -> bool {
        self.limits.is_empty()
            && self.epp.is_none()
            && self.cpu_boost.is_none()
            && self.nvidia.is_empty()
            && self.curve_optimizer.is_none()
    }
}

pub struct ProfileLimitsTracker {
    dir: Option<PathBuf>,
    limits: ProfileLimits,
    error: Option<String>,
    current: Option<PerformanceProfile>,
}

impl ProfileLimitsTracker {
    pub fn load() -> Self {
        Self::from_load(None, load_profile_limits().map_err(|e| e.to_string()))
    }

    #[cfg(test)]
    pub fn load_from_dir(dir: PathBuf) -> Self {
        let loaded = load_profile_limits_from_dir(&dir).map_err(|e| e.to_string());
        Self::from_load(Some(dir), loaded)
    }

    fn from_load(dir: Option<PathBuf>, loaded: Result<ProfileLimits, String>) -> Self {
        let (limits, error) = match loaded {
            Ok(limits) => (limits, None),
            Err(message) => {
                tracing::warn!("profile limits unreadable, auto-apply disabled: {message}");
                (ProfileLimits::default(), Some(message))
            }
        };
        Self {
            dir,
            limits,
            error,
            current: None,
        }
    }

    pub fn current(&self) -> Option<PerformanceProfile> {
        self.current
    }

    pub fn view(&self) -> Option<ProfileLimitsView> {
        let profile = self.current?;
        let set = self.limits.get(profile);
        Some(ProfileLimitsView {
            profile,
            auto_apply: set.auto_apply && self.error.is_none(),
            saved: set.entries(),
            epp: set.epp,
            cpu_boost: set.cpu_boost,
            nvidia: set.nvidia_entries(),
            curve_optimizer: set.curve_optimizer,
            error: self.error.clone(),
        })
    }

    /// Record the authoritative current profile. Returns the values to re-apply
    /// when the profile changed from a previously observed one and auto-apply is
    /// on for the new profile. The very first observation never applies.
    pub fn observe(&mut self, profile: PerformanceProfile) -> ProfileReplay {
        let previous = self.current.replace(profile);
        if previous.is_none() || previous == Some(profile) || self.error.is_some() {
            return ProfileReplay::default();
        }
        let set = self.limits.get(profile);
        if set.auto_apply {
            ProfileReplay {
                limits: set.entries(),
                epp: set.epp,
                cpu_boost: set.cpu_boost,
                nvidia: set.nvidia_entries(),
                curve_optimizer: set.curve_optimizer,
            }
        } else {
            ProfileReplay::default()
        }
    }

    pub fn set_auto_apply(&mut self, enabled: bool) -> Result<(), String> {
        let profile = self.current.ok_or("current profile is not known yet")?;
        if self.error.is_some() {
            return Err("stored profile limits are unreadable".into());
        }
        let mut next = self.limits.clone();
        next.get_mut(profile).auto_apply = enabled;
        self.save(next)
    }

    /// Remember a value the user just applied, only when auto-apply is on for
    /// the active profile.
    pub fn record_applied(&mut self, field: &PowerLimitField, value: i32) {
        let Some(profile) = self.current else { return };
        let Some(key) = orbis_config::limit_key(field) else {
            return;
        };
        if self.error.is_some() || !self.limits.get(profile).auto_apply {
            return;
        }
        if self.limits.get(profile).values.get(key) == Some(&value) {
            return;
        }
        let mut next = self.limits.clone();
        next.get_mut(profile).values.insert(key.into(), value);
        if let Err(message) = self.save(next) {
            tracing::warn!("profile limits could not be saved: {message}");
        }
    }

    pub fn record_epp(&mut self, epp: EnergyPreference) {
        self.record(|set| set.epp != Some(epp), |set| set.epp = Some(epp));
    }

    pub fn record_cpu_boost(&mut self, enabled: bool) {
        self.record(
            |set| set.cpu_boost != Some(enabled),
            |set| set.cpu_boost = Some(enabled),
        );
    }

    pub fn record_curve_optimizer(&mut self, offset: i32) {
        self.record(
            |set| set.curve_optimizer != Some(offset),
            |set| set.curve_optimizer = Some(offset),
        );
    }

    pub fn record_nvidia(&mut self, field: NvidiaField, value: i32) {
        self.record(
            |set| set.nvidia(field) != Some(value),
            |set| set.set_nvidia(field, value),
        );
    }

    fn record(
        &mut self,
        changed: impl Fn(&orbis_config::ProfileLimitSet) -> bool,
        update: impl FnOnce(&mut orbis_config::ProfileLimitSet),
    ) {
        let Some(profile) = self.current else { return };
        if self.error.is_some() || !self.limits.get(profile).auto_apply {
            return;
        }
        if !changed(self.limits.get(profile)) {
            return;
        }
        let mut next = self.limits.clone();
        update(next.get_mut(profile));
        if let Err(message) = self.save(next) {
            tracing::warn!("profile CPU tuning could not be saved: {message}");
        }
    }

    fn save(&mut self, next: ProfileLimits) -> Result<(), String> {
        let result = match &self.dir {
            Some(dir) => save_profile_limits_to_dir(&next, dir),
            None => save_profile_limits(&next),
        };
        result.map_err(|e| e.to_string())?;
        self.limits = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> (tempfile::TempDir, ProfileLimitsTracker) {
        let dir = tempfile::tempdir().unwrap();
        let tracker = ProfileLimitsTracker::load_from_dir(dir.path().to_path_buf());
        (dir, tracker)
    }

    #[test]
    fn values_are_stored_only_while_auto_apply_is_on_and_replayed_on_change() {
        let (dir, mut tracker) = tracker();
        assert!(tracker.set_auto_apply(true).is_err(), "profile unknown yet");
        assert!(tracker.observe(PerformanceProfile::Balanced).is_empty());

        tracker.record_applied(&PowerLimitField::Spl, 40);
        assert!(tracker.view().unwrap().saved.is_empty(), "auto-apply off");

        tracker.set_auto_apply(true).unwrap();
        tracker.record_applied(&PowerLimitField::Spl, 40);
        tracker.record_applied(&PowerLimitField::Other("x".into()), 1);
        assert_eq!(
            tracker.view().unwrap().saved,
            vec![(PowerLimitField::Spl, 40)]
        );

        tracker.record_epp(EnergyPreference::Power);
        tracker.record_cpu_boost(false);
        assert!(tracker.observe(PerformanceProfile::Turbo).is_empty());
        let expected = ProfileReplay {
            limits: vec![(PowerLimitField::Spl, 40)],
            epp: Some(EnergyPreference::Power),
            cpu_boost: Some(false),
            nvidia: Vec::new(),
            curve_optimizer: None,
        };
        assert_eq!(tracker.observe(PerformanceProfile::Balanced), expected);
        assert!(tracker.observe(PerformanceProfile::Balanced).is_empty());

        let mut reloaded = ProfileLimitsTracker::load_from_dir(dir.path().to_path_buf());
        assert!(reloaded.observe(PerformanceProfile::Turbo).is_empty());
        assert_eq!(reloaded.observe(PerformanceProfile::Balanced), expected);
    }

    #[test]
    fn unreadable_store_disables_replay_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("profile-limits.toml"), "silent = 3").unwrap();
        let mut tracker = ProfileLimitsTracker::load_from_dir(dir.path().to_path_buf());
        tracker.observe(PerformanceProfile::Silent);
        assert!(tracker.set_auto_apply(true).is_err());
        assert!(tracker.view().unwrap().error.is_some());
        assert!(tracker.observe(PerformanceProfile::Turbo).is_empty());
    }
}
