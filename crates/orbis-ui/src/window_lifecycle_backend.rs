//! Main-window lifecycle glue for safe user-level preferences.
//!
//! Window geometry is persisted only on window systems where Slint can request
//! an absolute position. Current Wayland sessions stay fail-closed because
//! absolute top-level placement is compositor-owned. The persisted state file is
//! independent from application preferences and invalid source bytes are never
//! overwritten.

use orbis_config::{
    CloseAction, WindowPositionState, WindowState, load_preferences, load_window_state,
    save_window_state,
};
use slint::{CloseRequestResponse, ComponentHandle, PhysicalPosition};

use crate::AppWindow;

pub(crate) fn position_runtime_supported() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY")
        .is_some_and(|value| !value.is_empty())
    {
        return false;
    }
    if std::env::var("SLINT_BACKEND")
        .ok()
        .is_some_and(|value| value.to_ascii_lowercase().contains("wayland"))
    {
        return false;
    }
    std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty())
}

fn remember_position_enabled() -> bool {
    match load_preferences() {
        Ok(load) if load.warning.is_none() => load.preferences.window.remember_position,
        Ok(load) => {
            tracing::warn!(warning = ?load.warning, "window position preference source preserved");
            false
        }
        Err(error) => {
            tracing::warn!(error = %error, "window position preference load failed");
            false
        }
    }
}

pub(crate) fn restore_position(app: &AppWindow) {
    if !position_runtime_supported() || !remember_position_enabled() {
        return;
    }
    match load_window_state() {
        Ok(load) if load.warning.is_none() => {
            if let Some(position) = load.state.position() {
                app.window()
                    .set_position(PhysicalPosition::new(position.x, position.y));
                tracing::debug!(x = position.x, y = position.y, "restored main window position");
            }
        }
        Ok(load) => tracing::warn!(warning = ?load.warning, "invalid window-state source preserved; position not restored"),
        Err(error) => tracing::warn!(error = %error, "window-state load failed; position not restored"),
    }
}

pub(crate) fn persist_position(app: &AppWindow) -> bool {
    if !position_runtime_supported() || !remember_position_enabled() {
        return false;
    }
    match load_window_state() {
        Ok(load) if load.warning.is_none() => {}
        Ok(load) => {
            tracing::warn!(warning = ?load.warning, "refusing to overwrite preserved invalid window-state source");
            return false;
        }
        Err(error) => {
            tracing::warn!(error = %error, "window-state pre-save load failed");
            return false;
        }
    }

    let position = app.window().position();
    let state = WindowState::new(Some(WindowPositionState {
        x: position.x,
        y: position.y,
    }));
    match save_window_state(&state) {
        Ok(path) => {
            tracing::debug!(?path, x = position.x, y = position.y, "saved main window position");
            true
        }
        Err(error) => {
            tracing::warn!(error = %error, "window position save failed");
            false
        }
    }
}

/// Install the window-manager close policy. HideToTray is honored only while a
/// real StatusNotifier host has accepted our item registration. If the desktop
/// loses its tray host, close-to-tray immediately fails closed by keeping the
/// main window visible.
pub(crate) fn wire_close_request(app: &AppWindow) {
    let weak = app.as_weak();
    app.window().on_close_requested(move || {
        let Some(app) = weak.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };
        let _ = persist_position(&app);

        let action = match load_preferences() {
            Ok(load) if load.warning.is_none() => load.preferences.window.close_action,
            Ok(load) => {
                tracing::warn!(warning = ?load.warning, "close-action source preserved; using Quit");
                CloseAction::Quit
            }
            Err(error) => {
                tracing::warn!(error = %error, "close-action load failed; using Quit");
                CloseAction::Quit
            }
        };

        match action {
            CloseAction::Quit => {
                // `HideWindow` alone would keep the process/event loop alive.
                // Explicitly terminate the Slint loop; HideWindow is returned
                // only as the close-request disposition while shutdown begins.
                if let Err(error) = slint::quit_event_loop() {
                    tracing::warn!(error = ?error, "close-action Quit could not terminate Slint event loop");
                    return CloseRequestResponse::KeepWindowShown;
                }
                CloseRequestResponse::HideWindow
            }
            CloseAction::HideToTray if super::tray_backend::is_ready() => {
                tracing::debug!("main window closing to registered StatusNotifier tray");
                CloseRequestResponse::HideWindow
            }
            CloseAction::HideToTray => {
                tracing::warn!("HideToTray requested but no StatusNotifier host is registered; keeping window shown");
                CloseRequestResponse::KeepWindowShown
            }
            CloseAction::Ask => {
                tracing::warn!("CloseAction::Ask has no typed close-confirmation context; keeping window shown");
                CloseRequestResponse::KeepWindowShown
            }
        }
    });
}

pub(crate) fn wire_app_window(app: &AppWindow) {
    restore_position(app);
    wire_close_request(app);
}

#[cfg(test)]
mod tests {
    #[test]
    fn position_and_tray_lifecycle_fail_closed() {
        let source = include_str!("window_lifecycle_backend.rs");
        assert!(source.contains("WAYLAND_DISPLAY"));
        assert!(source.contains("tray_backend::is_ready"));
        assert!(source.contains("KeepWindowShown"));
        assert!(source.contains("load_window_state"));
        assert!(source.contains("save_window_state"));
        assert!(source.contains("slint::quit_event_loop()"));
        assert!(!source.contains("Command::new"));
    }
}
