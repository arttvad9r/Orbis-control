//! Request-only Preferences bridge for main-window position memory.

use slint::ComponentHandle;

use crate::{AppWindow, PreferencesWindow};

pub(crate) fn wire(window: &PreferencesWindow, app: &AppWindow) {
    let window_weak = window.as_weak();
    let app_weak = app.as_weak();
    window.on_remember_position_changed(move |enabled| {
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        let Some(app) = app_weak.upgrade() else {
            return;
        };

        if !super::window_lifecycle_backend::position_runtime_supported()
            || !window.get_remember_position_enabled()
        {
            tracing::warn!(enabled, "Remember Position request ignored: positioning unavailable");
            crate::sync_preferences_window(&window);
            return;
        }

        window.set_remember_position_enabled(false);
        match crate::preferences_backend::persist_remember_position(enabled) {
            Ok(preferences) => {
                window.set_remember_position(preferences.window.remember_position);
                window.set_remember_position_enabled(
                    super::window_lifecycle_backend::position_runtime_supported(),
                );
                if preferences.window.remember_position {
                    let _ = super::window_lifecycle_backend::persist_position(&app);
                }
                window.set_local_status(
                    if preferences.window.remember_position {
                        "Window position memory enabled · X11 lifecycle active"
                    } else {
                        "Window position memory disabled"
                    }
                    .into(),
                );
            }
            Err(error) => {
                tracing::warn!(error = %error, "Remember Position preference save failed");
                crate::sync_preferences_window(&window);
                window.set_local_status("Could not save Remember Position".into());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn bridge_is_request_only_and_has_no_hardware_surface() {
        let source = include_str!("window_position_preferences_bridge.rs");
        assert!(source.contains("persist_remember_position"));
        assert!(source.contains("persist_position"));
        assert!(source.contains("position_runtime_supported"));
        assert!(!source.contains("WorkerCommand::Set"));
        assert!(!source.contains("Command::new"));
    }
}
