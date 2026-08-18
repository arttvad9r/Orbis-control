from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old[:80]!r}")
    p.write_text(text.replace(old, new, 1))


cargo = Path("crates/orbis-ui/Cargo.toml")
text = cargo.read_text()
needle = 'orbis-core = { workspace = true }\n'
if text.count(needle) != 1:
    raise SystemExit("orbis-ui Cargo.toml dependency anchor changed")
cargo.write_text(text.replace(needle, needle + 'orbis-config = { workspace = true }\n', 1))

main = Path("crates/orbis-ui/src/main.rs")
text = main.read_text()
needle = 'use orbis_core::action::{ActionRequirement, ApplyResult};\n'
imports = '''use orbis_config::{
    PreferencesConfig, PreferencesError, PreferencesLoad, PreferencesWarning, ThemePreference,
    load_preferences, save_preferences,
};
'''
if text.count(needle) != 1:
    raise SystemExit("main.rs import anchor changed")
text = text.replace(needle, imports + needle, 1)

old = '''fn current_theme_light() -> bool {
    THEME_LIGHT.with(Cell::get)
}
'''
new = '''fn current_theme_light() -> bool {
    THEME_LIGHT.with(Cell::get)
}

fn current_theme_mode() -> ThemeMode {
    theme_mode(current_theme_light())
}

fn set_current_theme_light(light: bool) {
    THEME_LIGHT.with(|state| state.set(light));
}

fn initial_theme_light_with(
    load: impl FnOnce() -> Result<PreferencesLoad, PreferencesError>,
) -> bool {
    match load() {
        Ok(load) => {
            if let Some(warning) = &load.warning {
                tracing::warn!(
                    path = ?warning.path,
                    kind = ?warning.kind,
                    "preferences load warning; using safe runtime theme"
                );
            }
            matches!(load.preferences.appearance.theme, ThemePreference::Light)
        }
        Err(error) => {
            tracing::warn!(error = %error, "preferences load failed; using dark theme");
            false
        }
    }
}

fn initialize_runtime_theme_with(
    load: impl FnOnce() -> Result<PreferencesLoad, PreferencesError>,
) -> bool {
    let light = initial_theme_light_with(load);
    set_current_theme_light(light);
    light
}

fn initialize_runtime_theme() {
    initialize_runtime_theme_with(load_preferences);
}

#[derive(Debug)]
enum ThemePersistenceFailure {
    Load(PreferencesError),
    Preserve(PreferencesWarning),
    Save(PreferencesError),
}

impl std::fmt::Display for ThemePersistenceFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(error) => write!(f, "failed to load preferences before saving theme: {error}"),
            Self::Preserve(warning) => write!(
                f,
                "refusing to overwrite preserved preferences source {:?}: {:?}",
                warning.path, warning.kind
            ),
            Self::Save(error) => write!(f, "failed to save theme preference: {error}"),
        }
    }
}

impl std::error::Error for ThemePersistenceFailure {}

fn persist_theme_with<L, S>(light: bool, load: L, save: S) -> Result<(), ThemePersistenceFailure>
where
    L: FnOnce() -> Result<PreferencesLoad, PreferencesError>,
    S: FnOnce(&PreferencesConfig) -> Result<std::path::PathBuf, PreferencesError>,
{
    let load = load().map_err(ThemePersistenceFailure::Load)?;
    if let Some(warning) = load.warning {
        return Err(ThemePersistenceFailure::Preserve(warning));
    }

    let mut preferences = load.preferences;
    preferences.appearance.theme = if light {
        ThemePreference::Light
    } else {
        ThemePreference::Dark
    };
    save(&preferences).map_err(ThemePersistenceFailure::Save)?;
    Ok(())
}

fn persist_theme(light: bool) -> Result<(), ThemePersistenceFailure> {
    persist_theme_with(light, load_preferences, save_preferences)
}

fn apply_theme_if_open<T>(
    window: Option<&T>,
    light: bool,
    apply: impl FnOnce(&T, ThemeMode),
) {
    if let Some(window) = window {
        apply(window, theme_mode(light));
    }
}
'''
if text.count(old) != 1:
    raise SystemExit("main.rs theme state anchor changed")
text = text.replace(old, new, 1)
text = text.replace(
    '.set_mode(theme_mode(current_theme_light()));',
    '.set_mode(current_theme_mode());',
)
text = text.replace(
    '    THEME_LIGHT.with(|state| state.set(light));\n    app.global::<ThemeState>().set_mode(theme_mode(light));',
    '    set_current_theme_light(light);\n    app.global::<ThemeState>().set_mode(theme_mode(light));',
    1,
)

for slot in [
    "FANS_WINDOW",
    "EXTRA_WINDOW",
    "AUTOMATION_WINDOW",
    "PREFERENCES_WINDOW",
    "DIAGNOSTICS_WINDOW",
    "UPDATES_WINDOW",
    "PREVIEW_DIALOG_WINDOW",
]:
    old_block = f'''    {slot}.with(|slot| {{
        if let Some(window) = slot.borrow().as_ref() {{
            window.global::<ThemeState>().set_mode(theme_mode(light));
        }}
    }});'''
    new_block = f'''    {slot}.with(|slot| {{
        let slot = slot.borrow();
        apply_theme_if_open(slot.as_ref(), light, |window, mode| {{
            window.global::<ThemeState>().set_mode(mode);
        }});
    }});'''
    if text.count(old_block) != 1:
        raise SystemExit(f"main.rs fan-out anchor changed for {slot}")
    text = text.replace(old_block, new_block, 1)

old_callback = '''            let app_weak = app.as_weak();
            window.on_theme_changed(move |light| {
                if let Some(app) = app_weak.upgrade() {
                    apply_theme_to_all(&app, light);
                }
            });'''
new_callback = '''            let app_weak = app.as_weak();
            let window_weak = window.as_weak();
            window.on_theme_changed(move |light| {
                if let Some(app) = app_weak.upgrade() {
                    apply_theme_to_all(&app, light);
                }

                match persist_theme(light) {
                    Ok(()) => {
                        if let Some(window) = window_weak.upgrade() {
                            let status = if light {
                                "Light theme active · saved"
                            } else {
                                "Dark theme active · saved"
                            };
                            window.set_local_status(status.into());
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "theme preference save failed; runtime theme remains active"
                        );
                        if let Some(window) = window_weak.upgrade() {
                            window.set_local_status(
                                "Theme active for this session · save failed; see logs".into(),
                            );
                        }
                    }
                }
            });'''
if text.count(old_callback) != 1:
    raise SystemExit("main.rs preferences theme callback anchor changed")
text = text.replace(old_callback, new_callback, 1)

startup = '    init_tracing();\n    let runtime = tokio::runtime::Runtime::new()?;'
startup_new = '    init_tracing();\n    initialize_runtime_theme();\n    let runtime = tokio::runtime::Runtime::new()?;'
if text.count(startup) != 1:
    raise SystemExit("main.rs startup anchor changed")
text = text.replace(startup, startup_new, 1)
main.write_text(text)

preferences_ui = Path("ui/audited/preferences-window.slint")
text = preferences_ui.read_text()
replacements = {
    'property <string> local-status: "Theme changes apply immediately · persistence not connected";':
        'property <string> local-status: "Theme changes save immediately · other controls remain preview-only";',
    'root.local-status = light ? "Light theme active · session only" : "Dark theme active · session only";':
        'root.local-status = light ? "Light theme active · saving…" : "Dark theme active · saving…";',
    'clicked => { root.local-status = "Preferences saved locally · theme persistence not connected"; }':
        'clicked => { root.local-status = "Other preferences remain preview-only · theme saves immediately"; }',
}
for old_text, new_text in replacements.items():
    if text.count(old_text) != 1:
        raise SystemExit(f"preferences-window.slint anchor changed: {old_text}")
    text = text.replace(old_text, new_text, 1)
preferences_ui.write_text(text)

tests = Path("crates/orbis-ui/src/main_tests.rs")
test_text = r'''

#[test]
fn missing_preferences_initialize_dark_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let light = initialize_runtime_theme_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(!light);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn persisted_dark_initializes_dark_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let preferences = orbis_config::PreferencesConfig::default();
    orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save dark preferences");

    let light = initialize_runtime_theme_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(!light);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn persisted_light_initializes_light_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut preferences = orbis_config::PreferencesConfig::default();
    preferences.appearance.theme = orbis_config::ThemePreference::Light;
    orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save light preferences");

    let light = initialize_runtime_theme_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(light);
    assert!(matches!(current_theme_mode(), ThemeMode::Light));
}

#[test]
fn theme_toggle_persists_only_theme_field() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut preferences = orbis_config::PreferencesConfig::default();
    preferences.window.close_action = orbis_config::CloseAction::Ask;
    preferences.window.start_minimized = true;
    preferences.window.remember_position = false;
    orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save fixture");

    persist_theme_with(
        true,
        || orbis_config::load_preferences_from_dir(td.path()),
        |preferences| orbis_config::save_preferences_to_dir(preferences, td.path()),
    )
    .expect("persist light theme");

    let reloaded = orbis_config::load_preferences_from_dir(td.path()).expect("reload preferences");
    assert_eq!(
        reloaded.preferences.appearance.theme,
        orbis_config::ThemePreference::Light
    );
    assert_eq!(
        reloaded.preferences.window.close_action,
        orbis_config::CloseAction::Ask
    );
    assert!(reloaded.preferences.window.start_minimized);
    assert!(!reloaded.preferences.window.remember_position);
}

#[test]
fn all_open_theme_targets_receive_same_runtime_theme() {
    let targets = [
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
    ];

    for target in &targets {
        apply_theme_if_open(Some(target), true, |target, mode| {
            target.set(matches!(mode, ThemeMode::Light));
        });
    }

    assert!(targets.iter().all(Cell::get));
}

#[test]
fn new_window_theme_is_derived_from_current_runtime_theme() {
    set_current_theme_light(true);
    assert!(matches!(current_theme_mode(), ThemeMode::Light));

    set_current_theme_light(false);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn theme_save_error_preserves_runtime_theme_and_is_diagnosable() {
    set_current_theme_light(true);

    let result = persist_theme_with(
        true,
        || {
            Ok(orbis_config::PreferencesLoad {
                preferences: orbis_config::PreferencesConfig::default(),
                source: orbis_config::PreferencesLoadSource::Defaults,
                warning: None,
            })
        },
        |_| {
            Err(orbis_config::PreferencesError::Io {
                operation: "test theme save",
                path: std::path::PathBuf::from("/test/preferences.toml"),
                source: std::io::Error::other("simulated save failure"),
            })
        },
    );

    let error = result.expect_err("save must fail");
    assert!(error.to_string().contains("failed to save theme preference"));
    assert!(current_theme_light());
    // Theme persistence has no worker/provider input, so this failure path cannot
    // enqueue a Hardware1/sessiond/hardwared operation.
}
'''
original = tests.read_text()
marker = "fn missing_preferences_initialize_dark_before_first_render()"
if marker in original:
    raise SystemExit("theme persistence tests already present")
tests.write_text(original + test_text)
