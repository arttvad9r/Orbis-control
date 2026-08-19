//! Persistent user-level window geometry state.
//!
//! Window position is runtime state, not application configuration. It
//! lives under XDG state storage and contains no hardware settings.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::CONFIG_DIR_NAME;

/// File name for persisted window state.
pub const WINDOW_STATE_FILE: &str = "window-state.toml";
/// Current schema version for the independent window-state document.
pub const WINDOW_STATE_SCHEMA_VERSION: u32 = 1;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Persisted physical window position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowPositionState {
    /// X coordinate in physical screen coordinates.
    pub x: i32,
    /// Y coordinate in physical screen coordinates.
    pub y: i32,
}

/// Versioned window state stored separately from application config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowState {
    schema_version: u32,
    position: Option<WindowPositionState>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            position: None,
        }
    }
}

impl WindowState {
    /// Construct state containing the latest known window position.
    pub fn new(position: Option<WindowPositionState>) -> Self {
        Self {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            position,
        }
    }

    /// Schema version of this state document.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Persisted position, if one exists.
    pub fn position(&self) -> Option<WindowPositionState> {
        self.position
    }

    fn validate(&self) -> Result<(), WindowStateError> {
        if self.schema_version != WINDOW_STATE_SCHEMA_VERSION {
            return Err(WindowStateError::Schema(format!(
                "schema_version {} is not supported; expected {}",
                self.schema_version, WINDOW_STATE_SCHEMA_VERSION
            )));
        }
        Ok(())
    }
}

/// Failures while resolving the XDG state location.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WindowStatePathError {
    /// Explicit `XDG_STATE_HOME` was empty or relative.
    #[error("invalid XDG_STATE_HOME for window state: {0:?}; expected an absolute path")]
    InvalidXdgStateHome(PathBuf),
    /// No home directory was available for the XDG fallback.
    #[error("cannot resolve window state path: no home directory is available")]
    MissingHome,
    /// Fallback home directory was empty or relative.
    #[error("invalid home directory for window state fallback: {0:?}; expected an absolute path")]
    InvalidHome(PathBuf),
}

/// Hard storage failures for window state.
#[derive(Debug, Error)]
pub enum WindowStateError {
    /// State path resolution failed.
    #[error(transparent)]
    Path(#[from] WindowStatePathError),
    /// A filesystem operation failed.
    #[error("{operation} failed for {path:?}: {source}")]
    Io {
        /// Operation being attempted.
        operation: &'static str,
        /// Path involved in the failure.
        path: PathBuf,
        /// Original I/O error.
        #[source]
        source: io::Error,
    },
    /// TOML serialization failed before replacement.
    #[error("window state TOML serialization failed: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// In-memory state violated the owned schema.
    #[error("window state schema: {0}")]
    Schema(String),
    /// Unique same-directory temporary file allocation failed repeatedly.
    #[error("could not allocate a unique window state temporary file in {dir:?}")]
    TempFileExhausted {
        /// Directory where the temporary file must live.
        dir: PathBuf,
    },
}

/// Non-fatal invalid-source classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowStateWarningKind {
    /// Source was not syntactically valid TOML.
    MalformedToml(String),
    /// Mandatory schema version was absent or invalid.
    InvalidSchemaVersion,
    /// Source uses an unsupported schema version.
    UnsupportedVersion(u32),
    /// Current-version TOML did not match the strict owned schema.
    InvalidSchema(String),
}

/// Preserved invalid source information accompanying safe defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowStateWarning {
    /// Source file that was left untouched.
    pub path: PathBuf,
    /// Typed warning classification.
    pub kind: WindowStateWarningKind,
}

/// Proven source of loaded window state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowStateLoadSource {
    /// Loaded from a valid existing state file.
    Stored,
    /// Safe defaults were used because state was absent or invalid.
    Defaults,
}

/// Result of a non-destructive window-state load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowStateLoad {
    /// Safe runtime state snapshot.
    pub state: WindowState,
    /// Source of the returned state.
    pub source: WindowStateLoadSource,
    /// Invalid-source warning, if applicable.
    pub warning: Option<WindowStateWarning>,
}

/// Resolve the production window-state directory.
pub fn window_state_dir() -> Result<PathBuf, WindowStatePathError> {
    window_state_dir_with(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        dirs::home_dir(),
    )
}

/// Pure XDG state path resolver used by production code and tests.
pub fn window_state_dir_with(
    xdg_state_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, WindowStatePathError> {
    if let Some(xdg) = xdg_state_home {
        if xdg.as_os_str().is_empty() || !xdg.is_absolute() {
            return Err(WindowStatePathError::InvalidXdgStateHome(xdg));
        }
        return Ok(xdg.join(CONFIG_DIR_NAME));
    }

    match home {
        Some(home) if !home.as_os_str().is_empty() && home.is_absolute() => {
            Ok(home.join(".local").join("state").join(CONFIG_DIR_NAME))
        }
        Some(home) => Err(WindowStatePathError::InvalidHome(home)),
        None => Err(WindowStatePathError::MissingHome),
    }
}

/// Resolve the production `window-state.toml` path.
pub fn window_state_file() -> Result<PathBuf, WindowStatePathError> {
    Ok(window_state_dir()?.join(WINDOW_STATE_FILE))
}

/// Load state from the production XDG state location.
pub fn load_window_state() -> Result<WindowStateLoad, WindowStateError> {
    load_window_state_from_dir(&window_state_dir()?)
}

/// Save state to the production XDG state location.
pub fn save_window_state(state: &WindowState) -> Result<PathBuf, WindowStateError> {
    save_window_state_to_dir(state, &window_state_dir()?)
}

/// Load state from an explicit state directory.
///
/// Missing or invalid state returns safe defaults. Invalid source bytes
/// are never rewritten by the load path.
pub fn load_window_state_from_dir(dir: &Path) -> Result<WindowStateLoad, WindowStateError> {
    let path = dir.join(WINDOW_STATE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(default_load(None));
        }
        Err(source) => return Err(io_failure("read window state", path, source)),
    };

    decode_window_state(&path, &text)
}

/// Atomically save state in an explicit state directory.
pub fn save_window_state_to_dir(
    state: &WindowState,
    dir: &Path,
) -> Result<PathBuf, WindowStateError> {
    state.validate()?;
    let text = toml::to_string_pretty(state)?;
    fs::create_dir_all(dir)
        .map_err(|source| io_failure("create window state directory", dir.to_path_buf(), source))?;

    let final_path = dir.join(WINDOW_STATE_FILE);
    let existing_permissions = match fs::symlink_metadata(&final_path) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata.permissions()),
        Ok(_) => None,
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(io_failure(
                "inspect existing window state permissions",
                final_path.clone(),
                source,
            ));
        }
    };

    let (mut temp, temp_path) = create_unique_temp(dir)?;
    if let Some(permissions) = existing_permissions {
        if let Err(source) = temp.set_permissions(permissions) {
            drop(temp);
            let _ = fs::remove_file(&temp_path);
            return Err(io_failure(
                "set window state temporary file permissions",
                temp_path,
                source,
            ));
        }
    }

    for (operation, result) in [
        (
            "write window state temporary file",
            temp.write_all(text.as_bytes()),
        ),
        ("flush window state temporary file", temp.flush()),
        ("sync window state temporary file", temp.sync_all()),
    ] {
        if let Err(source) = result {
            drop(temp);
            let _ = fs::remove_file(&temp_path);
            return Err(io_failure(operation, temp_path, source));
        }
    }
    drop(temp);

    if let Err(source) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io_failure(
            "rename window state temporary file",
            final_path,
            source,
        ));
    }

    sync_parent_directory(dir)?;
    Ok(final_path)
}

fn decode_window_state(path: &Path, text: &str) -> Result<WindowStateLoad, WindowStateError> {
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return Ok(default_load(Some(WindowStateWarning {
                path: path.to_path_buf(),
                kind: WindowStateWarningKind::MalformedToml(error.to_string()),
            })));
        }
    };

    let Some(version) = value
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok())
    else {
        return Ok(default_load(Some(WindowStateWarning {
            path: path.to_path_buf(),
            kind: WindowStateWarningKind::InvalidSchemaVersion,
        })));
    };

    if version != WINDOW_STATE_SCHEMA_VERSION {
        return Ok(default_load(Some(WindowStateWarning {
            path: path.to_path_buf(),
            kind: WindowStateWarningKind::UnsupportedVersion(version),
        })));
    }

    let state: WindowState = match toml::from_str(text) {
        Ok(state) => state,
        Err(error) => {
            return Ok(default_load(Some(WindowStateWarning {
                path: path.to_path_buf(),
                kind: WindowStateWarningKind::InvalidSchema(error.to_string()),
            })));
        }
    };

    Ok(WindowStateLoad {
        state,
        source: WindowStateLoadSource::Stored,
        warning: None,
    })
}

fn default_load(warning: Option<WindowStateWarning>) -> WindowStateLoad {
    WindowStateLoad {
        state: WindowState::default(),
        source: WindowStateLoadSource::Defaults,
        warning,
    }
}

fn create_unique_temp(dir: &Path) -> Result<(File, PathBuf), WindowStateError> {
    for _ in 0..64 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(
            ".{WINDOW_STATE_FILE}.tmp.{}.{}.{}",
            std::process::id(),
            nanos,
            sequence
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(io_failure(
                    "create window state temporary file",
                    path,
                    source,
                ));
            }
        }
    }

    Err(WindowStateError::TempFileExhausted {
        dir: dir.to_path_buf(),
    })
}

#[cfg(unix)]
fn sync_parent_directory(dir: &Path) -> Result<(), WindowStateError> {
    let directory = File::open(dir).map_err(|source| {
        io_failure(
            "open window state directory for sync",
            dir.to_path_buf(),
            source,
        )
    })?;
    directory
        .sync_all()
        .map_err(|source| io_failure("sync window state directory", dir.to_path_buf(), source))
}

#[cfg(not(unix))]
fn sync_parent_directory(_dir: &Path) -> Result<(), WindowStateError> {
    Ok(())
}

fn io_failure(operation: &'static str, path: PathBuf, source: io::Error) -> WindowStateError {
    WindowStateError::Io {
        operation,
        path,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_state_path_and_home_fallback_are_separate_from_config() {
        let td = tempfile::tempdir().unwrap();
        let xdg = td.path().join("state");
        let home = td.path().join("home");
        assert_eq!(
            window_state_dir_with(Some(xdg.clone()), None).unwrap(),
            xdg.join(CONFIG_DIR_NAME)
        );
        assert_eq!(
            window_state_dir_with(None, Some(home.clone())).unwrap(),
            home.join(".local").join("state").join(CONFIG_DIR_NAME)
        );
    }

    #[test]
    fn relative_explicit_xdg_state_home_is_rejected() {
        let error = window_state_dir_with(Some(PathBuf::from("relative")), None)
            .expect_err("relative state home must fail");
        assert!(matches!(
            error,
            WindowStatePathError::InvalidXdgStateHome(_)
        ));
    }

    #[test]
    fn missing_file_returns_safe_default_without_creating_state() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("state");
        let load = load_window_state_from_dir(&dir).unwrap();
        assert_eq!(load.state, WindowState::default());
        assert_eq!(load.source, WindowStateLoadSource::Defaults);
        assert!(load.warning.is_none());
        assert!(!dir.join(WINDOW_STATE_FILE).exists());
    }

    #[test]
    fn position_roundtrip_preserves_signed_coordinates() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("state");
        let state = WindowState::new(Some(WindowPositionState { x: -1440, y: 37 }));
        save_window_state_to_dir(&state, &dir).unwrap();
        let load = load_window_state_from_dir(&dir).unwrap();
        assert_eq!(load.source, WindowStateLoadSource::Stored);
        assert_eq!(load.state.position(), state.position());
        assert_eq!(load.state.schema_version(), WINDOW_STATE_SCHEMA_VERSION);
    }

    #[test]
    fn malformed_file_returns_default_and_preserves_source() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("state");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(WINDOW_STATE_FILE);
        let original = "schema_version = 1\n[position\nx = 1\n";
        fs::write(&path, original).unwrap();
        let load = load_window_state_from_dir(&dir).unwrap();
        assert_eq!(load.state, WindowState::default());
        assert!(matches!(
            load.warning.as_ref().map(|warning| &warning.kind),
            Some(WindowStateWarningKind::MalformedToml(_))
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn unsupported_version_returns_default_and_preserves_source() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("state");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(WINDOW_STATE_FILE);
        let original = "schema_version = 2\n";
        fs::write(&path, original).unwrap();
        let load = load_window_state_from_dir(&dir).unwrap();
        assert_eq!(load.state, WindowState::default());
        assert_eq!(
            load.warning.as_ref().map(|warning| &warning.kind),
            Some(&WindowStateWarningKind::UnsupportedVersion(2))
        );
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn unknown_fields_are_invalid_and_preserved() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("state");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(WINDOW_STATE_FILE);
        let original = "schema_version = 1\nunexpected = true\n";
        fs::write(&path, original).unwrap();
        let load = load_window_state_from_dir(&dir).unwrap();
        assert_eq!(load.state, WindowState::default());
        assert!(matches!(
            load.warning.as_ref().map(|warning| &warning.kind),
            Some(WindowStateWarningKind::InvalidSchema(_))
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn new_state_file_is_owner_only_and_temp_is_cleaned() {
        use std::os::unix::fs::PermissionsExt;

        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("state");
        let path = save_window_state_to_dir(&WindowState::default(), &dir).unwrap();
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| {
                name.to_string_lossy()
                    .starts_with(".window-state.toml.tmp.")
            })
            .collect();
        assert!(leftovers.is_empty());
    }
}
