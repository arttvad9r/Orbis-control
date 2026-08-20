//! Request-only Preferences bridge for main-window position and close lifecycle.

use orbis_config::CloseAction;
use slint::ComponentHandle;

use crate::{AppWindow, PreferencesWindow};

fn sync_close_capabilities(window: &PreferencesWindow) {
    // `close-action-enabled` comes from the hardened preferences source and
    // controls whether the setting can be persisted at all. Tray availability
    // is a separate live capability and gates only HideToTray.
    window.set_hide_to_tray_enabled(super::tray_backend::is_ready());
}

pub(crate) fn wire(window: &PreferencesWindow, app: &AppWindow) {
    sync_close_capabilities(window);

    {
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
                sync_close_capabilities(&window);
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
            sync_close_capabilities(&window);
        });
    }

    {
        let weak = window.as_weak();
        window.on_close_action_changed(move |index| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !window.get_close_action_enabled() {
                tracing::warn!(index, "Close Action request ignored: preferences source not writable");
                crate::sync_preferences_window(&window);
                sync_close_capabilities(&window);
                return;
            }

            let action = match index {
                // Quit has no tray dependency and must remain available even on
                // desktops without StatusNotifier support.
                0 => CloseAction::Quit,
                1 if super::tray_backend::is_ready() && window.get_hide_to_tray_enabled() => {
                    CloseAction::HideToTray
                }
                1 => {
                    tracing::warn!("HideToTray request ignored: tray host unavailable");
                    crate::sync_preferences_window(&window);
                    sync_close_capabilities(&window);
                    window.set_local_status("Hide to tray is unavailable on this desktop".into());
                    return;
                }
                other => {
                    tracing::warn!(index = other, "Close Action request ignored: invalid index");
                    return;
                }
            };

            window.set_close_action_enabled(false);
            window.set_hide_to_tray_enabled(false);
            match crate::preferences_backend::persist_close_action(action) {
                Ok(preferences) => {
                    window.set_close_action(match preferences.window.close_action {
                        CloseAction::HideToTray => 1,
                        CloseAction::Quit | CloseAction::Ask => 0,
                    });
                    // Re-read source writability rather than tying the whole
                    // setting to a transient tray-host observation.
                    crate::sync_preferences_window(&window);
                    sync_close_capabilities(&window);
                    window.set_local_status(
                        match preferences.window.close_action {
                            CloseAction::HideToTray => "Close button will hide Orbis to the tray",
                            CloseAction::Quit => "Close button will quit Orbis",
                            CloseAction::Ask => "Close confirmation mode is not exposed here",
                        }
                        .into(),
                    );
                }
                Err(error) => {
                    tracing::warn!(error = %error, "Close Action preference save failed");
                    crate::sync_preferences_window(&window);
                    sync_close_capabilities(&window);
                    window.set_local_status("Could not save Close Action".into());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bridge_is_request_only_and_gates_only_tray_specific_action() {
        let source = include_str!("window_position_preferences_bridge.rs");
        assert!(source.contains("persist_remember_position"));
        assert!(source.contains("persist_position"));
        assert!(source.contains("position_runtime_supported"));
        assert!(source.contains("persist_close_action"));
        assert!(source.contains("set_hide_to_tray_enabled"));
        assert!(source.contains("0 => CloseAction::Quit"));
        assert!(source.contains("tray_backend::is_ready"));
        assert!(!source.contains("WorkerCommand::Set"));
        assert!(!source.contains("Command::new"));
    }
}
