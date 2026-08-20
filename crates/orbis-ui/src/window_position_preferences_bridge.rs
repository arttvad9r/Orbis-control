//! Request-only Preferences bridge for main-window position and close lifecycle.

use orbis_config::CloseAction;
use slint::ComponentHandle;

use crate::{AppWindow, PreferencesWindow};

pub(crate) fn wire(window: &PreferencesWindow, app: &AppWindow) {
    // Base preferences bridge intentionally reports close-action read-only.
    // Promote only while a visible StatusNotifier host is actually registered.
    window.set_close_action_enabled(super::tray_backend::is_ready());

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
                window.set_close_action_enabled(super::tray_backend::is_ready());
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
            window.set_close_action_enabled(super::tray_backend::is_ready());
        });
    }

    {
        let weak = window.as_weak();
        window.on_close_action_changed(move |index| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !super::tray_backend::is_ready() || !window.get_close_action_enabled() {
                tracing::warn!(index, "Close Action request ignored: tray host unavailable");
                crate::sync_preferences_window(&window);
                window.set_close_action_enabled(false);
                return;
            }

            let action = match index {
                0 => CloseAction::Quit,
                1 => CloseAction::HideToTray,
                other => {
                    tracing::warn!(index = other, "Close Action request ignored: invalid index");
                    return;
                }
            };
            window.set_close_action_enabled(false);
            match crate::preferences_backend::persist_close_action(action) {
                Ok(preferences) => {
                    window.set_close_action(match preferences.window.close_action {
                        CloseAction::HideToTray => 1,
                        CloseAction::Quit | CloseAction::Ask => 0,
                    });
                    let ready = super::tray_backend::is_ready();
                    window.set_close_action_enabled(ready);
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
                    window.set_close_action_enabled(super::tray_backend::is_ready());
                    window.set_local_status("Could not save Close Action".into());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bridge_is_request_only_tray_gated_and_has_no_hardware_surface() {
        let source = include_str!("window_position_preferences_bridge.rs");
        assert!(source.contains("persist_remember_position"));
        assert!(source.contains("persist_position"));
        assert!(source.contains("position_runtime_supported"));
        assert!(source.contains("tray_backend::is_ready"));
        assert!(source.contains("persist_close_action"));
        assert!(!source.contains("WorkerCommand::Set"));
        assert!(!source.contains("Command::new"));
    }
}
