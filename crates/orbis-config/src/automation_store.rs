//! Hardened read-only loading for the typed Automation policy.
//!
//! Both the UI editor and the shadow runtime must interpret `desired-state.toml`
//! identically. In particular, malformed/current-version-invalid input is
//! preserved by the generic store and must not silently become editable or
//! executable defaults.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{
    AutomationPolicy, DesiredStateError, DesiredStateWarningKind, OrbisDesiredState,
    load_desired_state, load_desired_state_from_dir,
};

/// Failure to obtain an Automation policy that is safe to use as runtime
/// desired intent.
#[derive(Debug, Error)]
pub enum AutomationPolicyLoadError {
    /// Hard storage/path/schema-version failure from the generic desired-state
    /// store.
    #[error(transparent)]
    Storage(#[from] DesiredStateError),
    /// The generic store preserved an invalid source and returned safe defaults.
    /// Those defaults are suitable for inert fallback but are not evidence of
    /// user intent, so Automation refuses to treat them as a runtime policy.
    #[error("preserved invalid Automation desired-state source at {path:?}: {kind:?}")]
    PreservedSource {
        /// Preserved source path.
        path: PathBuf,
        /// Exact non-destructive warning classification.
        kind: DesiredStateWarningKind,
    },
}

/// Load the production Automation policy from the hardened desired-state store.
///
/// A missing file is a valid default/off policy. A malformed or schema-invalid
/// existing file is rejected rather than converted into apparent user intent.
pub fn load_automation_policy() -> Result<AutomationPolicy, AutomationPolicyLoadError> {
    let load = load_desired_state::<OrbisDesiredState>()?;
    policy_from_load(load)
}

/// Load an Automation policy from an explicit desired-state directory.
///
/// This is the same production semantics with an injected directory for tests
/// and offline validation.
pub fn load_automation_policy_from_dir(
    dir: &Path,
) -> Result<AutomationPolicy, AutomationPolicyLoadError> {
    let load = load_desired_state_from_dir::<OrbisDesiredState>(dir)?;
    policy_from_load(load)
}

fn policy_from_load(
    load: crate::DesiredStateLoad<OrbisDesiredState>,
) -> Result<AutomationPolicy, AutomationPolicyLoadError> {
    if let Some(warning) = load.warning {
        return Err(AutomationPolicyLoadError::PreservedSource {
            path: warning.path,
            kind: warning.kind,
        });
    }
    Ok(load.state.into_desired().automation)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{DesiredStateDocument, save_desired_state_to_dir};

    #[test]
    fn missing_file_is_inert_default_policy() {
        let dir = tempfile::tempdir().unwrap();
        let policy = load_automation_policy_from_dir(dir.path()).unwrap();
        assert_eq!(policy, AutomationPolicy::default());
        assert!(!policy.enabled);
    }

    #[test]
    fn valid_typed_policy_roundtrips_through_shared_loader() {
        let dir = tempfile::tempdir().unwrap();
        let mut desired = OrbisDesiredState::default();
        desired.automation.enabled = true;
        desired.automation.on_ac_change = true;
        save_desired_state_to_dir(&DesiredStateDocument::new(desired.clone()), dir.path()).unwrap();

        let policy = load_automation_policy_from_dir(dir.path()).unwrap();
        assert_eq!(policy, desired.automation);
    }

    #[test]
    fn malformed_existing_source_is_preserved_and_rejected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(crate::DESIRED_STATE_FILE), "not = [valid").unwrap();

        let error = load_automation_policy_from_dir(dir.path()).unwrap_err();
        assert!(matches!(
            error,
            AutomationPolicyLoadError::PreservedSource {
                kind: DesiredStateWarningKind::MalformedToml(_),
                ..
            }
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join(crate::DESIRED_STATE_FILE)).unwrap(),
            "not = [valid"
        );
    }
}
