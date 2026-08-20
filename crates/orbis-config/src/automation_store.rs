//! Hardened read-only loading for the typed Automation policy.
//!
//! Both the UI editor and unattended runtime must interpret `desired-state.toml`
//! identically. In particular, malformed/current-version-invalid input is
//! preserved by the generic store and must not silently become editable or
//! executable defaults.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{
    AutomationPolicy, DESIRED_STATE_FILE, DesiredStateError, DesiredStateWarningKind,
    OrbisDesiredState, desired_state_file, load_desired_state, load_desired_state_from_dir,
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

/// Exact-content identity of the persisted Automation source.
///
/// Bytes are deliberately private and `Debug` never renders them. Equality is
/// exact rather than mtime/hash based, so an atomic save that happens to retain
/// file length/timestamp cannot be missed. A missing file has its own identity
/// because it represents the valid default/off policy.
#[derive(Clone, PartialEq, Eq)]
pub struct AutomationPolicySourceFingerprint {
    bytes: Option<Vec<u8>>,
}

impl AutomationPolicySourceFingerprint {
    /// Whether the authoritative source file did not exist at observation time.
    pub fn is_missing(&self) -> bool {
        self.bytes.is_none()
    }

    /// Byte length without exposing persisted configuration contents.
    pub fn byte_len(&self) -> usize {
        self.bytes.as_ref().map_or(0, Vec::len)
    }
}

impl fmt::Debug for AutomationPolicySourceFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_missing() {
            formatter.write_str("AutomationPolicySourceFingerprint::Missing")
        } else {
            formatter
                .debug_struct("AutomationPolicySourceFingerprint::Present")
                .field("byte_len", &self.byte_len())
                .finish()
        }
    }
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

/// Read exact persisted-source identity from the production desired-state path.
///
/// This performs no TOML parsing and never creates/writes the file. Callers use
/// it only to decide whether a previously authoritative typed policy may be
/// stale and therefore needs hardened reloading.
pub fn automation_policy_source_fingerprint(
) -> Result<AutomationPolicySourceFingerprint, DesiredStateError> {
    let path = desired_state_file()?;
    fingerprint_file(&path)
}

/// Read exact persisted-source identity from an injected desired-state directory.
pub fn automation_policy_source_fingerprint_from_dir(
    dir: &Path,
) -> Result<AutomationPolicySourceFingerprint, DesiredStateError> {
    fingerprint_file(&dir.join(DESIRED_STATE_FILE))
}

fn fingerprint_file(path: &Path) -> Result<AutomationPolicySourceFingerprint, DesiredStateError> {
    match fs::read(path) {
        Ok(bytes) => Ok(AutomationPolicySourceFingerprint { bytes: Some(bytes) }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Ok(AutomationPolicySourceFingerprint { bytes: None })
        }
        Err(source) => Err(DesiredStateError::Io {
            operation: "read Automation desired-state fingerprint",
            path: path.to_path_buf(),
            source,
        }),
    }
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
        let fingerprint = automation_policy_source_fingerprint_from_dir(dir.path()).unwrap();
        assert!(fingerprint.is_missing());
        assert_eq!(fingerprint.byte_len(), 0);
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
        let fingerprint = automation_policy_source_fingerprint_from_dir(dir.path()).unwrap();
        assert!(!fingerprint.is_missing());
        assert!(fingerprint.byte_len() > 0);
    }

    #[test]
    fn malformed_existing_source_is_preserved_and_rejected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(DESIRED_STATE_FILE), "not = [valid").unwrap();

        let error = load_automation_policy_from_dir(dir.path()).unwrap_err();
        assert!(matches!(
            error,
            AutomationPolicyLoadError::PreservedSource {
                kind: DesiredStateWarningKind::MalformedToml(_),
                ..
            }
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join(DESIRED_STATE_FILE)).unwrap(),
            "not = [valid"
        );
    }

    #[test]
    fn exact_fingerprint_detects_same_length_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DESIRED_STATE_FILE);
        fs::write(&path, b"aaaa").unwrap();
        let before = automation_policy_source_fingerprint_from_dir(dir.path()).unwrap();
        fs::write(&path, b"bbbb").unwrap();
        let after = automation_policy_source_fingerprint_from_dir(dir.path()).unwrap();
        assert_eq!(before.byte_len(), after.byte_len());
        assert_ne!(before, after);
    }

    #[test]
    fn fingerprint_debug_never_exposes_source_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let secret = "sensitive-policy-marker";
        fs::write(dir.path().join(DESIRED_STATE_FILE), secret).unwrap();
        let fingerprint = automation_policy_source_fingerprint_from_dir(dir.path()).unwrap();
        let debug = format!("{fingerprint:?}");
        assert!(!debug.contains(secret));
        assert!(debug.contains("byte_len"));
    }
}
