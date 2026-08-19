//! Production storage for safe, user-level UI preferences.
//!
//! This module deliberately excludes automation policy, desired hardware state,
//! experimental gates, desktop autostart ownership, and runtime hardware state.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CONFIG_DIR_NAME, CONFIG_VERSION};

/// File name for the production safe-preferences store.
pub const PREFERENCES_FILE: &str = "preferences.toml";

/// Current version of the independent preferences schema.
pub const PREFERENCES_SCHEMA_VERSION: u32 = 1;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Persisted application theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Dark application theme.
    #[default]
    Dark,
    /// Light application theme.
    Light,
}

/// Persisted behavior for the main-window close action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    /// Hide the application to its tray integration.
    HideToTray,
    /// Quit the application.
    ///
    /// Until tray behavior is production-wired, Quit is the conservative
    /// standalone default. Legacy close_to_tray=true is imported explicitly.
    #[default]
    Quit,
    /// Ask the user each time.
    Ask,
}

/// Safe appearance preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AppearancePreferences {
    /// Selected Dark or Light theme.
    pub theme: ThemePreference,
}

/// Safe window-behavior preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowPreferences {
    /// Behavior requested when the main window is closed.
    pub close_action: CloseAction,
    /// Whether the application should start minimized once runtime wiring exists.
    pub start_minimized: bool,
    /// Whether separately stored window geometry may be restored.
    pub remember_position: bool,
}

impl Default for WindowPreferences {
    fn default() -> Self {
        Self {
            close_action: CloseAction::default(),
            start_minimized: false,
            remember_position: true,
        }
    }
}

/// Versioned safe-preferences document stored in `preferences.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferencesConfig {
    /// Mandatory independent preferences schema version.
    pub schema_version: u32,
    /// Appearance-only preferences.
    pub appearance: AppearancePreferences,
    /// Window/application behavior preferences.
    pub window: WindowPreferences,
}

impl Default for PreferencesConfig {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            appearance: AppearancePreferences::default(),
            window: WindowPreferences::default(),
        }
    }
}

impl PreferencesConfig {
    /// Validate the exact schema version before persistence.
    pub fn validate(&self) -> Result<(), PreferencesError> {
        if self.schema_version > PREFERENCES_SCHEMA_VERSION {
            return Err(PreferencesError::UnsupportedVersion {
                found: self.schema_version,
                supported: PREFERENCES_SCHEMA_VERSION,
            });
        }
        if self.schema_version != PREFERENCES_SCHEMA_VERSION {
            return Err(PreferencesError::Schema(format!(
                "schema_version {} is not supported; expected {}",
                self.schema_version, PREFERENCES_SCHEMA_VERSION
            )));
        }
        Ok(())
    }
}

/// Typed failures while resolving the production preferences location.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PreferencesPathError {
    /// `XDG_CONFIG_HOME` was set but was empty or not absolute.
    #[error("invalid XDG_CONFIG_HOME for preferences: {0:?}; expected a non-empty absolute path")]
    InvalidXdgConfigHome(PathBuf),
    /// No home directory was available for the XDG fallback.
    #[error("cannot resolve preferences path: no home directory is available")]
    MissingHome,
    /// The fallback home directory was empty or not absolute.
    #[error("invalid home directory for preferences fallback: {0:?}; expected an absolute path")]
    InvalidHome(PathBuf),
}

/// Hard failures from the production safe-preferences store.
#[derive(Debug, Error)]
pub enum PreferencesError {
    /// Production path resolution failed.
    #[error(transparent)]
    Path(#[from] PreferencesPathError),
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
    /// TOML serialization failed before the destination was replaced.
    #[error("preferences TOML serialization failed: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// A newer binary owns the file and this binary must not rewrite it.
    #[error("unsupported preferences schema version {found}; supported version is {supported}")]
    UnsupportedVersion {
        /// Version found on disk or supplied for save.
        found: u32,
        /// Version understood by this binary.
        supported: u32,
    },
    /// The in-memory document violates the current schema contract.
    #[error("preferences schema: {0}")]
    Schema(String),
    /// Exclusive creation repeatedly collided with existing temporary files.
    #[error("could not allocate a unique preferences temporary file in {dir:?}")]
    TempFileExhausted {
        /// Directory where the temporary file must live.
        dir: PathBuf,
    },
}

/// Non-fatal load conditions that fall back to safe runtime defaults.
#[derive(Clone, PartialEq, Eq)]
pub enum PreferencesWarningKind {
    /// The file was not syntactically valid TOML.
    MalformedToml(String),
    /// The mandatory `schema_version` key was absent.
    MissingSchemaVersion,
    /// `schema_version` was present but not a non-negative `u32` integer.
    InvalidSchemaVersion,
    /// An older unsupported preferences schema was encountered.
    UnsupportedOlderVersion(u32),
    /// Current-version TOML did not match the strict owned schema.
    InvalidSchema(String),
    /// Legacy `config.toml` could not be safely imported.
    LegacyImport(String),
}

impl PreferencesWarningKind {
    /// Fixed category safe for logs and support diagnostics.
    ///
    /// Embedded parser/schema messages may contain excerpts from user-owned
    /// configuration and must never be emitted through production logging.
    pub const fn log_category(&self) -> &'static str {
        match self {
            Self::MalformedToml(_) => "malformed_toml",
            Self::MissingSchemaVersion => "missing_schema_version",
            Self::InvalidSchemaVersion => "invalid_schema_version",
            Self::UnsupportedOlderVersion(_) => "unsupported_older_version",
            Self::InvalidSchema(_) => "invalid_schema",
            Self::LegacyImport(_) => "legacy_import",
        }
    }
}

impl std::fmt::Debug for PreferencesWarningKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.log_category())
    }
}

/// A typed non-fatal preferences load warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferencesWarning {
    /// Preserved source file that caused the warning.
    pub path: PathBuf,
    /// Typed warning classification.
    pub kind: PreferencesWarningKind,
}

/// Proven source of the returned preferences snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesLoadSource {
    /// Loaded from an existing valid `preferences.toml`.
    Stored,
    /// Safe defaults were used because no valid preferences were available.
    Defaults,
    /// Safe UI fields were imported from legacy `config.toml` and persisted.
    LegacyImported,
}

/// Result of loading production preferences without hiding recoverable warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferencesLoad {
    /// Safe runtime preferences snapshot.
    pub preferences: PreferencesConfig,
    /// Source that produced the snapshot.
    pub source: PreferencesLoadSource,
    /// Recoverable load/import warning, if any.
    pub warning: Option<PreferencesWarning>,
}

/// Resolve the production preferences directory.
///
/// A set `XDG_CONFIG_HOME` must be a non-empty absolute path. If it is unset,
/// `$HOME/.config` is used. Invalid explicit XDG values are errors and never
/// degrade to the current working directory.
pub fn preferences_dir() -> Result<PathBuf, PreferencesPathError> {
    preferences_dir_with(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        dirs::home_dir(),
    )
}

/// Pure path resolver used by production code and targeted tests.
pub fn preferences_dir_with(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, PreferencesPathError> {
    if let Some(xdg) = xdg_config_home {
        if xdg.as_os_str().is_empty() || !xdg.is_absolute() {
            return Err(PreferencesPathError::InvalidXdgConfigHome(xdg));
        }
        return Ok(xdg.join(CONFIG_DIR_NAME));
    }

    match home {
        Some(home) if !home.as_os_str().is_empty() && home.is_absolute() => {
            Ok(home.join(".config").join(CONFIG_DIR_NAME))
        }
        Some(home) => Err(PreferencesPathError::InvalidHome(home)),
        None => Err(PreferencesPathError::MissingHome),
    }
}

/// Resolve the production `preferences.toml` path.
pub fn preferences_file() -> Result<PathBuf, PreferencesPathError> {
    Ok(preferences_dir()?.join(PREFERENCES_FILE))
}

/// Load preferences from the production XDG location.
///
/// If `preferences.toml` is absent, a one-way safe import from legacy
/// `config.toml` is attempted. Loading never touches hardware.
pub fn load_preferences() -> Result<PreferencesLoad, PreferencesError> {
    load_preferences_from_dir(&preferences_dir()?)
}

/// Save preferences to the production XDG location using atomic replacement.
pub fn save_preferences(config: &PreferencesConfig) -> Result<PathBuf, PreferencesError> {
    save_preferences_to_dir(config, &preferences_dir()?)
}

/// Load safe preferences from an explicit directory.
///
/// Missing files return defaults. Malformed or invalid current files are
/// preserved and return defaults plus a typed warning. Future schema versions
/// are a hard error and are never rewritten. If no preferences file exists, a
/// valid legacy `config.toml` contributes only its safe `ui.*` fields.
pub fn load_preferences_from_dir(dir: &Path) -> Result<PreferencesLoad, PreferencesError> {
    let preferences_path = dir.join(PREFERENCES_FILE);
    match fs::read_to_string(&preferences_path) {
        Ok(text) => return decode_preferences(&preferences_path, &text),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(io_failure("read preferences", preferences_path, source));
        }
    }

    let legacy_path = dir.join(crate::paths::CONFIG_FILE);
    let legacy_text = match fs::read_to_string(&legacy_path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(default_load(None));
        }
        Err(source) => return Err(io_failure("read legacy config", legacy_path, source)),
    };

    match import_legacy_preferences(&legacy_text) {
        Ok(preferences) => {
            save_preferences_to_dir(&preferences, dir)?;
            Ok(PreferencesLoad {
                preferences,
                source: PreferencesLoadSource::LegacyImported,
                warning: None,
            })
        }
        Err(kind) => Ok(default_load(Some(PreferencesWarning {
            path: legacy_path,
            kind,
        }))),
    }
}

/// Save safe preferences to an explicit directory.
///
/// Serialization is completed before the existing file is touched. The new
/// bytes are written to an exclusively created unique temporary file in the
/// same directory, synced, and then renamed over `preferences.toml`.
pub fn save_preferences_to_dir(
    config: &PreferencesConfig,
    dir: &Path,
) -> Result<PathBuf, PreferencesError> {
    config.validate()?;
    let text = toml::to_string_pretty(config)?;

    fs::create_dir_all(dir)
        .map_err(|source| io_failure("create preferences directory", dir.to_path_buf(), source))?;

    let final_path = dir.join(PREFERENCES_FILE);
    let existing_permissions = match fs::symlink_metadata(&final_path) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata.permissions()),
        Ok(_) => None,
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(io_failure(
                "inspect existing preferences permissions",
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
                "set preferences temporary file permissions",
                temp_path,
                source,
            ));
        }
    }

    if let Err(source) = temp.write_all(text.as_bytes()) {
        drop(temp);
        let _ = fs::remove_file(&temp_path);
        return Err(io_failure(
            "write preferences temporary file",
            temp_path,
            source,
        ));
    }
    if let Err(source) = temp.flush() {
        drop(temp);
        let _ = fs::remove_file(&temp_path);
        return Err(io_failure(
            "flush preferences temporary file",
            temp_path,
            source,
        ));
    }
    if let Err(source) = temp.sync_all() {
        drop(temp);
        let _ = fs::remove_file(&temp_path);
        return Err(io_failure(
            "sync preferences temporary file",
            temp_path,
            source,
        ));
    }
    drop(temp);

    if let Err(source) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io_failure(
            "rename preferences temporary file",
            final_path,
            source,
        ));
    }

    sync_parent_directory(dir)?;
    Ok(final_path)
}

fn sync_parent_directory(dir: &Path) -> Result<(), PreferencesError> {
    let directory = File::open(dir).map_err(|source| {
        io_failure(
            "open preferences directory for sync",
            dir.to_path_buf(),
            source,
        )
    })?;
    directory
        .sync_all()
        .map_err(|source| io_failure("sync preferences directory", dir.to_path_buf(), source))
}

fn default_load(warning: Option<PreferencesWarning>) -> PreferencesLoad {
    PreferencesLoad {
        preferences: PreferencesConfig::default(),
        source: PreferencesLoadSource::Defaults,
        warning,
    }
}

fn decode_preferences(path: &Path, text: &str) -> Result<PreferencesLoad, PreferencesError> {
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return Ok(default_load(Some(PreferencesWarning {
                path: path.to_path_buf(),
                kind: PreferencesWarningKind::MalformedToml(error.to_string()),
            })));
        }
    };

    let Some(raw_version) = value.get("schema_version") else {
        return Ok(default_load(Some(PreferencesWarning {
            path: path.to_path_buf(),
            kind: PreferencesWarningKind::MissingSchemaVersion,
        })));
    };
    let Some(raw_version) = raw_version.as_integer() else {
        return Ok(default_load(Some(PreferencesWarning {
            path: path.to_path_buf(),
            kind: PreferencesWarningKind::InvalidSchemaVersion,
        })));
    };
    let Ok(version) = u32::try_from(raw_version) else {
        return Ok(default_load(Some(PreferencesWarning {
            path: path.to_path_buf(),
            kind: PreferencesWarningKind::InvalidSchemaVersion,
        })));
    };

    if version > PREFERENCES_SCHEMA_VERSION {
        return Err(PreferencesError::UnsupportedVersion {
            found: version,
            supported: PREFERENCES_SCHEMA_VERSION,
        });
    }
    if version < PREFERENCES_SCHEMA_VERSION {
        return Ok(default_load(Some(PreferencesWarning {
            path: path.to_path_buf(),
            kind: PreferencesWarningKind::UnsupportedOlderVersion(version),
        })));
    }

    let preferences: PreferencesConfig = match toml::from_str(text) {
        Ok(preferences) => preferences,
        Err(error) => {
            return Ok(default_load(Some(PreferencesWarning {
                path: path.to_path_buf(),
                kind: PreferencesWarningKind::InvalidSchema(error.to_string()),
            })));
        }
    };

    if let Err(error) = preferences.validate() {
        return Ok(default_load(Some(PreferencesWarning {
            path: path.to_path_buf(),
            kind: PreferencesWarningKind::InvalidSchema(error.to_string()),
        })));
    }

    Ok(PreferencesLoad {
        preferences,
        source: PreferencesLoadSource::Stored,
        warning: None,
    })
}

#[derive(Debug, Deserialize)]
struct LegacyConfigEnvelope {
    config_version: u32,
    #[serde(default)]
    ui: LegacyUiConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct LegacyUiConfig {
    theme: String,
    close_to_tray: bool,
    start_minimized: bool,
    remember_position: bool,
}

impl Default for LegacyUiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            close_to_tray: true,
            start_minimized: false,
            remember_position: true,
        }
    }
}

fn import_legacy_preferences(text: &str) -> Result<PreferencesConfig, PreferencesWarningKind> {
    let legacy: LegacyConfigEnvelope = toml::from_str(text)
        .map_err(|error| PreferencesWarningKind::LegacyImport(error.to_string()))?;

    if legacy.config_version != CONFIG_VERSION {
        return Err(PreferencesWarningKind::LegacyImport(format!(
            "unsupported legacy config_version {}; expected {}",
            legacy.config_version, CONFIG_VERSION
        )));
    }

    let theme = match legacy.ui.theme.as_str() {
        "dark" => ThemePreference::Dark,
        "light" => ThemePreference::Light,
        other => {
            return Err(PreferencesWarningKind::LegacyImport(format!(
                "unsupported legacy ui.theme {other:?}"
            )));
        }
    };

    Ok(PreferencesConfig {
        schema_version: PREFERENCES_SCHEMA_VERSION,
        appearance: AppearancePreferences { theme },
        window: WindowPreferences {
            close_action: if legacy.ui.close_to_tray {
                CloseAction::HideToTray
            } else {
                CloseAction::Quit
            },
            start_minimized: legacy.ui.start_minimized,
            remember_position: legacy.ui.remember_position,
        },
    })
}

fn create_unique_temp(dir: &Path) -> Result<(File, PathBuf), PreferencesError> {
    for _ in 0..64 {
        let path = unique_temp_path(dir);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(io_failure(
                    "create preferences temporary file",
                    path,
                    source,
                ));
            }
        }
    }

    Err(PreferencesError::TempFileExhausted {
        dir: dir.to_path_buf(),
    })
}

fn unique_temp_path(dir: &Path) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    dir.join(format!(
        ".{PREFERENCES_FILE}.tmp.{}.{}.{}",
        std::process::id(),
        nanos,
        sequence
    ))
}

fn io_failure(operation: &'static str, path: PathBuf, source: io::Error) -> PreferencesError {
    PreferencesError::Io {
        operation,
        path,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_current(path: &Path, config: &PreferencesConfig) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, toml::to_string_pretty(config).expect("serialize")).expect("write");
    }

    #[test]
    fn xdg_and_home_fallback_paths() {
        let td = temp_dir();
        let xdg = td.path().join("xdg");
        let home = td.path().join("home");

        assert_eq!(
            preferences_dir_with(Some(xdg.clone()), None).unwrap(),
            xdg.join(CONFIG_DIR_NAME)
        );
        assert_eq!(
            preferences_dir_with(None, Some(home.clone())).unwrap(),
            home.join(".config").join(CONFIG_DIR_NAME)
        );
    }

    #[test]
    fn relative_xdg_path_is_rejected() {
        let error = preferences_dir_with(Some(PathBuf::from("relative/config")), None)
            .expect_err("relative XDG_CONFIG_HOME must fail");
        assert!(matches!(
            error,
            PreferencesPathError::InvalidXdgConfigHome(_)
        ));
    }

    #[test]
    fn missing_preferences_returns_defaults_without_creating_file() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        let load = load_preferences_from_dir(&dir).unwrap();

        assert_eq!(load.preferences, PreferencesConfig::default());
        assert_eq!(load.source, PreferencesLoadSource::Defaults);
        assert!(load.warning.is_none());
        assert!(!dir.join(PREFERENCES_FILE).exists());
    }

    #[test]
    fn current_schema_roundtrip() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        let config = PreferencesConfig {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            appearance: AppearancePreferences {
                theme: ThemePreference::Light,
            },
            window: WindowPreferences {
                close_action: CloseAction::Ask,
                start_minimized: true,
                remember_position: false,
            },
        };

        save_preferences_to_dir(&config, &dir).unwrap();
        let load = load_preferences_from_dir(&dir).unwrap();
        assert_eq!(load.preferences, config);
        assert_eq!(load.source, PreferencesLoadSource::Stored);
        assert!(load.warning.is_none());
    }

    #[test]
    fn future_schema_version_is_error_and_never_rewritten() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PREFERENCES_FILE);
        let original = "schema_version = 2\n\n[appearance]\ntheme = \"dark\"\n\n[window]\nclose_action = \"quit\"\nstart_minimized = false\nremember_position = true\n";
        fs::write(&path, original).unwrap();

        let error = load_preferences_from_dir(&dir).expect_err("future version must fail");
        assert!(matches!(
            error,
            PreferencesError::UnsupportedVersion {
                found: 2,
                supported: PREFERENCES_SCHEMA_VERSION
            }
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn malformed_preferences_are_preserved_with_safe_defaults() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PREFERENCES_FILE);
        let original = "schema_version = 1\n[appearance]\ntheme = [\n";
        fs::write(&path, original).unwrap();

        let load = load_preferences_from_dir(&dir).unwrap();
        assert_eq!(load.preferences, PreferencesConfig::default());
        assert_eq!(load.source, PreferencesLoadSource::Defaults);
        assert!(matches!(
            load.warning.as_ref().map(|warning| &warning.kind),
            Some(PreferencesWarningKind::MalformedToml(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn atomic_save_uses_unique_same_directory_temp() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        fs::create_dir_all(&dir).unwrap();

        let first = unique_temp_path(&dir);
        let second = unique_temp_path(&dir);
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(dir.as_path()));
        assert_eq!(second.parent(), Some(dir.as_path()));

        let fixed_shared_tmp = dir.join(format!("{PREFERENCES_FILE}.tmp"));
        fs::write(&fixed_shared_tmp, "sentinel").unwrap();
        save_preferences_to_dir(&PreferencesConfig::default(), &dir).unwrap();
        assert_eq!(fs::read_to_string(&fixed_shared_tmp).unwrap(), "sentinel");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".preferences.toml.tmp."))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn new_preferences_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let td = temp_dir();
        let dir = td.path().join("cfg");
        let path = save_preferences_to_dir(&PreferencesConfig::default(), &dir).unwrap();

        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let td = temp_dir();
        let dir = td.path().join("cfg");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PREFERENCES_FILE);
        fs::write(&path, "placeholder").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        save_preferences_to_dir(&PreferencesConfig::default(), &dir).unwrap();

        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn legacy_safe_fields_import_with_close_mapping() {
        let td = temp_dir();

        for (name, close_to_tray, expected_close) in [
            ("tray", true, CloseAction::HideToTray),
            ("quit", false, CloseAction::Quit),
        ] {
            let dir = td.path().join(name);
            fs::create_dir_all(&dir).unwrap();
            let legacy_path = dir.join(crate::paths::CONFIG_FILE);
            let legacy = format!(
                "config_version = 1\n\n[ui]\ntheme = \"light\"\nclose_to_tray = {close_to_tray}\nstart_minimized = true\nremember_position = false\n"
            );
            fs::write(&legacy_path, &legacy).unwrap();

            let load = load_preferences_from_dir(&dir).unwrap();
            assert_eq!(load.source, PreferencesLoadSource::LegacyImported);
            assert_eq!(load.preferences.appearance.theme, ThemePreference::Light);
            assert_eq!(load.preferences.window.close_action, expected_close);
            assert!(load.preferences.window.start_minimized);
            assert!(!load.preferences.window.remember_position);
            assert!(dir.join(PREFERENCES_FILE).exists());
            assert_eq!(fs::read_to_string(&legacy_path).unwrap(), legacy);
        }
    }

    #[test]
    fn legacy_non_preference_domains_are_not_imported() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        fs::create_dir_all(&dir).unwrap();
        let legacy = r#"config_version = 1

[ui]
theme = "dark"
close_to_tray = false
start_minimized = false
remember_position = true

[battery]
charge_limit = 99

[automation]
enabled = true
power_event_delay_ms = 424242
marker = "do-not-import"

[experimental]
enabled = true
undervolting = true
raw_wmi = true
"#;
        fs::write(dir.join(crate::paths::CONFIG_FILE), legacy).unwrap();

        let load = load_preferences_from_dir(&dir).unwrap();
        assert_eq!(load.source, PreferencesLoadSource::LegacyImported);

        let persisted = fs::read_to_string(dir.join(PREFERENCES_FILE)).unwrap();
        for forbidden in [
            "battery",
            "charge_limit",
            "automation",
            "power_event_delay_ms",
            "experimental",
            "undervolting",
            "raw_wmi",
            "do-not-import",
        ] {
            assert!(
                !persisted.contains(forbidden),
                "unexpected legacy field: {forbidden}"
            );
        }
    }

    #[test]
    fn invalid_current_schema_uses_defaults_and_preserves_source() {
        let td = temp_dir();
        let dir = td.path().join("cfg");
        let path = dir.join(PREFERENCES_FILE);
        let mut config = PreferencesConfig::default();
        config.appearance.theme = ThemePreference::Light;
        write_current(&path, &config);
        let original = fs::read_to_string(&path).unwrap();

        let invalid = original.replace("theme = \"light\"", "theme = \"system\"");
        fs::write(&path, &invalid).unwrap();
        let load = load_preferences_from_dir(&dir).unwrap();

        assert_eq!(load.preferences, PreferencesConfig::default());
        assert!(matches!(
            load.warning.as_ref().map(|warning| &warning.kind),
            Some(PreferencesWarningKind::InvalidSchema(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }

    #[test]
    fn warning_debug_redacts_embedded_diagnostics() {
        const SECRET: &str = "ORBIS_SYNTHETIC_SECRET_MARKER_7f3b";

        for kind in [
            PreferencesWarningKind::MalformedToml(SECRET.into()),
            PreferencesWarningKind::InvalidSchema(SECRET.into()),
            PreferencesWarningKind::LegacyImport(SECRET.into()),
        ] {
            let rendered = format!("{kind:?}");
            assert_eq!(rendered, kind.log_category());
            assert!(!rendered.contains(SECRET));
        }

        let warning = PreferencesWarning {
            path: PathBuf::from("/tmp/preferences.toml"),
            kind: PreferencesWarningKind::MalformedToml(SECRET.into()),
        };
        assert!(!format!("{warning:?}").contains(SECRET));
    }
}
