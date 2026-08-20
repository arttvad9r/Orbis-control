//! Race-safe synchronization of persisted Automation policy into the worker
//! driver.
//!
//! A UI Save is an atomic file replacement, but the worker must not rely on an
//! mtime notification or a separate UI message to notice it. This synchronizer
//! compares exact-content fingerprints around the shared hardened loader:
//!
//! `fingerprint_before -> load typed policy -> fingerprint_after`
//!
//! Only a stable before/after identity may replace worker intent. Any unstable or
//! unreadable source invalidates the cached policy fail-closed; a later telemetry
//! tick may retry. Unchanged fingerprints avoid repeated TOML parsing.

use orbis_config::{
    AutomationPolicyLoadError, AutomationPolicySourceFingerprint,
    automation_policy_source_fingerprint, load_automation_policy,
};

use crate::automation_worker_driver::{
    AutomationPolicyRevision, AutomationPolicyRevisionError, AutomationWorkerDriver,
};

/// Outcome of one read-only persisted-policy synchronization attempt.
#[derive(Debug)]
pub enum AutomationPolicySyncOutcome {
    /// Exact source bytes match the previously accepted observation.
    Unchanged,
    /// Stable hardened policy was loaded and installed in the worker driver.
    Loaded {
        /// New worker policy revision.
        revision: AutomationPolicyRevision,
        /// Whether the loaded policy is globally enabled.
        enabled: bool,
    },
    /// Stable source exists but cannot be accepted as typed user intent. Cached
    /// policy is cleared; details are diagnostic only.
    SourceRejected {
        /// Worker revision after clearing prior intent, if one was present.
        revision: Option<AutomationPolicyRevision>,
        /// Hardened load error.
        error: AutomationPolicyLoadError,
    },
    /// Fingerprint could not be read. Cached policy is cleared and no source
    /// identity is remembered so a later tick retries.
    FingerprintUnavailable {
        /// Worker revision after clearing prior intent, if one was present.
        revision: Option<AutomationPolicyRevision>,
        /// Redacted storage/path detail.
        detail: String,
    },
    /// Source bytes changed between the two observations. No newly loaded policy
    /// is accepted; any prior cached policy is cleared and next tick retries.
    SourceChangedDuringRead {
        /// Worker revision after clearing prior intent, if one was present.
        revision: Option<AutomationPolicyRevision>,
    },
    /// Policy revision space exhausted while invalidating/installing intent.
    RevisionExhausted,
}

/// Unique exact-source observer for one worker-owned Automation driver.
///
/// Deliberately not `Clone`: two synchronizers with different remembered source
/// fingerprints could make reload decisions inconsistent for one driver.
#[derive(Debug, Default)]
pub struct AutomationPolicySynchronizer {
    accepted_fingerprint: Option<AutomationPolicySourceFingerprint>,
}

impl AutomationPolicySynchronizer {
    /// Construct with no accepted source identity; first call performs a load.
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow the last stable fingerprint accepted/rejected from disk.
    pub fn accepted_fingerprint(&self) -> Option<&AutomationPolicySourceFingerprint> {
        self.accepted_fingerprint.as_ref()
    }

    /// Synchronize the driver only when exact persisted bytes changed.
    pub fn synchronize(
        &mut self,
        driver: &mut AutomationWorkerDriver,
    ) -> AutomationPolicySyncOutcome {
        let before = match automation_policy_source_fingerprint() {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                self.accepted_fingerprint = None;
                let revision = match clear_if_present(driver) {
                    Ok(revision) => revision,
                    Err(AutomationPolicyRevisionError::SequenceExhausted) => {
                        return AutomationPolicySyncOutcome::RevisionExhausted;
                    }
                };
                return AutomationPolicySyncOutcome::FingerprintUnavailable {
                    revision,
                    detail: error.to_string(),
                };
            }
        };

        if self.accepted_fingerprint.as_ref() == Some(&before) {
            return AutomationPolicySyncOutcome::Unchanged;
        }

        let loaded = load_automation_policy();
        let after = match automation_policy_source_fingerprint() {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                self.accepted_fingerprint = None;
                let revision = match clear_if_present(driver) {
                    Ok(revision) => revision,
                    Err(AutomationPolicyRevisionError::SequenceExhausted) => {
                        return AutomationPolicySyncOutcome::RevisionExhausted;
                    }
                };
                return AutomationPolicySyncOutcome::FingerprintUnavailable {
                    revision,
                    detail: error.to_string(),
                };
            }
        };

        if before != after {
            self.accepted_fingerprint = None;
            let revision = match clear_if_present(driver) {
                Ok(revision) => revision,
                Err(AutomationPolicyRevisionError::SequenceExhausted) => {
                    return AutomationPolicySyncOutcome::RevisionExhausted;
                }
            };
            return AutomationPolicySyncOutcome::SourceChangedDuringRead { revision };
        }

        self.accepted_fingerprint = Some(after);
        match loaded {
            Ok(policy) => {
                let enabled = policy.enabled;
                match driver.replace_persisted_policy(policy) {
                    Ok(revision) => AutomationPolicySyncOutcome::Loaded { revision, enabled },
                    Err(AutomationPolicyRevisionError::SequenceExhausted) => {
                        AutomationPolicySyncOutcome::RevisionExhausted
                    }
                }
            }
            Err(error) => {
                let revision = match clear_if_present(driver) {
                    Ok(revision) => revision,
                    Err(AutomationPolicyRevisionError::SequenceExhausted) => {
                        return AutomationPolicySyncOutcome::RevisionExhausted;
                    }
                };
                AutomationPolicySyncOutcome::SourceRejected { revision, error }
            }
        }
    }
}

fn clear_if_present(
    driver: &mut AutomationWorkerDriver,
) -> Result<Option<AutomationPolicyRevision>, AutomationPolicyRevisionError> {
    if driver.persisted_policy().is_none() {
        return Ok(None);
    }
    driver.clear_persisted_policy().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synchronizer_is_non_clone_unique_source_owner() {
        let source = include_str!("automation_policy_sync.rs");
        assert!(source.contains("accepted_fingerprint"));
        assert!(!source.contains("#[derive(Debug, Clone"));
    }

    #[test]
    fn source_uses_exact_before_after_fingerprints_and_hardened_loader() {
        let source = include_str!("automation_policy_sync.rs");
        let first = source.find("let before =").unwrap();
        let load = source.find("let loaded = load_automation_policy()").unwrap();
        let after = source.find("let after =").unwrap();
        let compare = source.find("if before != after").unwrap();
        assert!(first < load && load < after && after < compare);
        assert!(source.contains("automation_policy_source_fingerprint()"));
    }

    #[test]
    fn unstable_or_unreadable_source_clears_cached_intent() {
        let source = include_str!("automation_policy_sync.rs");
        assert!(source.matches("clear_if_present(driver)").count() >= 3);
        assert!(source.contains("SourceChangedDuringRead"));
        assert!(source.contains("FingerprintUnavailable"));
        assert!(source.contains("SourceRejected"));
    }

    #[test]
    fn synchronizer_has_no_mutation_or_slint_surface() {
        let source = include_str!("automation_policy_sync.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "performance("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["Command", "::new("].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected mutation/process token: {needle}");
        }
        assert!(!source.contains("slint::"));
    }
}
