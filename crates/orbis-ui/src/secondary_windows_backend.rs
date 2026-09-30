//! Coordinator for the surviving desktop/lifecycle backend bridges.
//!
//! The dedicated secondary windows left the UI (spec §2.5): Extra and Settings
//! are sections of the single AppWindow now, Updates and Automation editing
//! surfaces are gone. This module keeps only the bridges that outlived them:
//! tray, window lifecycle/close-action, the Extra hardware section backend,
//! and the fail-closed action dialog.

#[path = "action_dialog_backend.rs"]
mod action_dialog_backend;
#[path = "extra_backend.rs"]
mod extra_backend;
#[path = "tray_backend.rs"]
mod tray_backend;
#[path = "window_lifecycle_backend.rs"]
mod window_lifecycle_backend;
#[path = "window_position_preferences_bridge.rs"]
mod window_position_preferences_bridge;

use crate::AppWindow;

pub(crate) fn initialize(runtime: tokio::runtime::Handle, session_connection: zbus::Connection) {
    extra_backend::initialize(runtime.clone(), session_connection);
    tray_backend::initialize(runtime);
}

/// True once a StatusNotifier host has registered the tray icon.
pub(crate) fn tray_ready() -> bool {
    tray_backend::is_ready()
}

pub(crate) fn clear() {
    tray_backend::clear();
    extra_backend::clear();
}

fn show_preview_dialog(kind: i32) -> Result<(), slint::PlatformError> {
    crate::show_preview_dialog(kind)?;
    crate::PREVIEW_DIALOG_WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref() {
            action_dialog_backend::wire(window, kind.clamp(0, 3));
        }
    });
    Ok(())
}

pub(crate) fn publish_tray_stats(state: &crate::controller::UiState) {
    tray_backend::publish_stats(&tray_backend::TrayStats {
        cpu_temp: state.cpu_temp.clone(),
        gpu_temp: state.gpu_temp.clone(),
        cpu_fan: state.cpu_fan_rpm.clone(),
        gpu_fan: state.gpu_fan_rpm.clone(),
        battery_percent: state.battery_percent.clone(),
        on_ac: match state.ac_online.as_str() {
            "On AC" => Some(true),
            "On battery" => Some(false),
            _ => None,
        },
        fresh: state.telemetry_fresh,
    });
}

pub(crate) fn wire_window(app: &AppWindow) {
    tray_backend::wire_app(app);
    window_lifecycle_backend::wire_app_window(app);

    // Extra hardware section: observations start immediately; the section is
    // reachable through the sidebar without any open-window step.
    extra_backend::wire_window(app);
    extra_backend::refresh(app);

    window_position_preferences_bridge::wire(app);

    app.on_preview_dialog_clicked(|kind| {
        if let Err(error) = show_preview_dialog(kind) {
            tracing::warn!(error = ?error, "failed to open fail-closed action dialog");
        }
    });
}

/// Close-request disposition shared by the native close path and the frameless
/// title bar button (spec §3): persist position, then honor CloseAction.
pub(crate) fn handle_close_request(app: &AppWindow) {
    window_lifecycle_backend::handle_close_request(app);
}

/// Settings section remember-position/close-action request handlers.
pub(crate) fn wire_position_preferences_bridge(app: &AppWindow) {
    window_position_preferences_bridge::wire(app);
}

#[cfg(test)]
mod tests {
    #[test]
    fn coordinator_contains_no_hardware_mutation_surface() {
        let source = include_str!("secondary_windows_backend.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "fan_curve("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "charge_limit("].concat(),
            ["set_", "keyboard_backlight("].concat(),
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "unexpected mutation token: {needle}"
            );
        }
    }
}
