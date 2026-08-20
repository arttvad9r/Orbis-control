use std::fmt;

use orbis_config::{
    AutostartError, AutostartStatus, CloseAction, PreferencesConfig, PreferencesError,
    PreferencesLoad, PreferencesWarning, autostart_status, disable_autostart, enable_autostart,
    load_preferences, save_preferences,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutostartUiState {
    pub enabled: bool,
    pub writable: bool,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowPreferencesUiState {
    pub start_minimized: bool,
    pub start_minimized_writable: bool,
    pub remember_position: bool,
    pub remember_position_writable: bool,
    pub close_action: i32,
    pub close_action_writable: bool,
    pub status: &'static str,
}

#[derive(Debug)]
pub(crate) enum PreferencesMutationError {
    Load(PreferencesError),
    Preserve(PreferencesWarning),
    Save(PreferencesError),
}

impl fmt::Display for PreferencesMutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => write!(f, "failed to load preferences before update: {error}"),
            Self::Preserve(warning) => write!(
                f,
                "refusing to overwrite preserved preferences source {:?}: {:?}",
                warning.path, warning.kind
            ),
            Self::Save(error) => write!(f, "failed to save preferences update: {error}"),
        }
    }
}

impl std::error::Error for PreferencesMutationError {}

pub(crate) fn map_autostart_status(status: AutostartStatus) -> AutostartUiState {
    match status {
        AutostartStatus::Missing => AutostartUiState {
            enabled: false,
            writable: true,
            status: "Disabled",
        },
        AutostartStatus::Enabled => AutostartUiState {
            enabled: true,
            writable: true,
            status: "Enabled",
        },
        AutostartStatus::Invalid => AutostartUiState {
            enabled: false,
            writable: true,
            status: "Entry differs · enable to repair",
        },
    }
}

pub(crate) fn read_autostart_state() -> Result<AutostartUiState, AutostartError> {
    autostart_status().map(map_autostart_status)
}

pub(crate) fn set_autostart(enabled: bool) -> Result<AutostartUiState, AutostartError> {
    if enabled {
        let _ = enable_autostart()?;
    } else {
        let _ = disable_autostart()?;
    }
    read_autostart_state()
}

fn close_action_index(action: CloseAction) -> i32 {
    match action {
        CloseAction::Quit => 0,
        CloseAction::HideToTray => 1,
        CloseAction::Ask => 0,
    }
}

pub(crate) fn map_window_preferences(load: PreferencesLoad) -> WindowPreferencesUiState {
    let has_warning = load.warning.is_some();
    WindowPreferencesUiState {
        start_minimized: load.preferences.window.start_minimized,
        start_minimized_writable: !has_warning,
        remember_position: load.preferences.window.remember_position,
        remember_position_writable: false,
        close_action: close_action_index(load.preferences.window.close_action),
        close_action_writable: false,
        status: if has_warning {
            "Preferences source preserved · editing disabled"
        } else {
            "Startup behavior connected · tray/position pending"
        },
    }
}

pub(crate) fn read_window_preferences_state() -> Result<WindowPreferencesUiState, PreferencesError> {
    load_preferences().map(map_window_preferences)
}

fn mutate_preferences_with<L, S, M>(
    load: L,
    save: S,
    mutate: M,
) -> Result<PreferencesConfig, PreferencesMutationError>
where
    L: FnOnce() -> Result<PreferencesLoad, PreferencesError>,
    S: FnOnce(&PreferencesConfig) -> Result<std::path::PathBuf, PreferencesError>,
    M: FnOnce(&mut PreferencesConfig),
{
    let load = load().map_err(PreferencesMutationError::Load)?;
    if let Some(warning) = load.warning {
        return Err(PreferencesMutationError::Preserve(warning));
    }

    let mut preferences = load.preferences;
    mutate(&mut preferences);
    save(&preferences).map_err(PreferencesMutationError::Save)?;
    Ok(preferences)
}

pub(crate) fn persist_start_minimized(
    enabled: bool,
) -> Result<PreferencesConfig, PreferencesMutationError> {
    mutate_preferences_with(load_preferences, save_preferences, |preferences| {
        preferences.window.start_minimized = enabled;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_mapping_never_claims_invalid_entry_is_enabled() {
        let state = map_autostart_status(AutostartStatus::Invalid);
        assert!(!state.enabled);
        assert!(state.writable);
        assert!(state.status.contains("repair"));
    }

    #[test]
    fn warning_preserves_preferences_and_disables_editing() {
        let load = PreferencesLoad {
            preferences: PreferencesConfig::default(),
            source: orbis_config::PreferencesLoadSource::Defaults,
            warning: Some(PreferencesWarning {
                path: std::path::PathBuf::from("/test/preferences.toml"),
                kind: orbis_config::PreferencesWarningKind::InvalidSchema("test".into()),
            }),
        };
        let state = map_window_preferences(load);
        assert!(!state.start_minimized_writable);
        assert!(!state.remember_position_writable);
        assert!(!state.close_action_writable);
    }

    #[test]
    fn start_minimized_update_preserves_other_fields() {
        let td = tempfile::tempdir().expect("tempdir");
        let mut preferences = PreferencesConfig::default();
        preferences.appearance.theme = orbis_config::ThemePreference::Light;
        preferences.window.remember_position = false;
        preferences.window.close_action = CloseAction::Ask;
        orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save fixture");

        let updated = mutate_preferences_with(
            || orbis_config::load_preferences_from_dir(td.path()),
            |preferences| orbis_config::save_preferences_to_dir(preferences, td.path()),
            |preferences| preferences.window.start_minimized = true,
        )
        .expect("save start minimized");

        assert!(updated.window.start_minimized);
        assert!(!updated.window.remember_position);
        assert_eq!(updated.window.close_action, CloseAction::Ask);
        assert_eq!(updated.appearance.theme, orbis_config::ThemePreference::Light);
    }
}
