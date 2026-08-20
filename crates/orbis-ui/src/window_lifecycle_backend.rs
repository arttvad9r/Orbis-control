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

/// Conservative runtime evidence for absolute position restore.
///
/// Slint documents that `Window::set_position` is unavailable on windowing
/// systems such as Wayland. We therefore enable the preference only when the
/// process is clearly using an X11-style display and no Wayland display is
/// present. An explicit Slint Wayland backend also fails closed.
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

/// Restore the last stored physical position before the first normal show.
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
        Ok(load) => {
            tracing::warn!(warning = ?load.warning, "invalid window-state source preserved; position not restored");
        }
        Err(error) => {
            tracing::warn!(error = %error, "window-state load failed; position not restored");
        }
    }
}

/// Persist the current physical position when the setting and backend support
/// are both authoritative. Invalid existing state is preserved rather than
/// silently replaced.
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

/// Install the window-manager close policy.
///
/// Quit is fully supported. HideToTray is intentionally rejected until a tray
/// owner is registered; disappearing with no activation path would strand the
/// process. Ask also stays visible until a typed action context is available.
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
            CloseAction::Quit => CloseRequestResponse::HideWindow,
            CloseAction::HideToTray => {
                tracing::warn!("HideToTray requested without registered tray owner; keeping window shown");
                CloseRequestResponse::KeepWindowShown
            }
            CloseAction::Ask => {
                tracing::warn!("CloseAction::Ask requested without typed close confirmation context; keeping window shown");
                CloseRequestResponse::KeepWindowShown
            }
        }
    });
}

/// Lifecycle wiring invoked after the legacy AppWindow callbacks exist.
pub(crate) fn wire_app_window(app: &AppWindow) {
    restore_position(app);
    wire_close_request(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_keeps_wayland_and_tray_fail_closed() {
        let source = include_str!("window_lifecycle_backend.rs");
        assert!(source.contains("WAYLAND_DISPLAY"));
        assert!(source.contains("CloseAction::HideToTray"));
        assert!(source.contains("KeepWindowShown"));
        assert!(source.contains("load_window_state"));
        assert!(source.contains("save_window_state"));
        assert!(!source.contains("Command::new"));
    }
}
