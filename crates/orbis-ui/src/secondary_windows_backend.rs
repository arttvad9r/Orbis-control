//! Coordinator for the surviving desktop/lifecycle backend bridges.
//!
//! The dedicated secondary windows left the UI (spec §2.5): Extra and Settings
//! are sections of the single AppWindow now, Updates and Automation editing
//! surfaces are gone. This module keeps only the bridges that outlived them:
//! tray, window lifecycle/close-action, the Extra hardware section backend,
//! the Automation shadow observation core and the fail-closed action dialog.

#[path = "action_dialog_backend.rs"]
mod action_dialog_backend;
#[path = "automation_backend.rs"]
mod automation_backend;
#[path = "extra_backend.rs"]
mod extra_backend;
#[path = "tray_backend.rs"]
mod tray_backend;
#[path = "window_lifecycle_backend.rs"]
mod window_lifecycle_backend;
#[path = "window_position_preferences_bridge.rs"]
mod window_position_preferences_bridge;

use std::cell::RefCell;
use std::sync::Arc;
use std::time::SystemTime;

use crate::AppWindow;
use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_core::telemetry::Telemetry;
use orbis_ui::automation_capability::augment_snapshot_with_automation_shadow;
use orbis_ui::automation_lifecycle_revision::{
    AutomationLifecycleClock, AutomationLifecycleRevision,
};

thread_local! {
    static AUTOMATION_LIFECYCLE_CLOCK: RefCell<AutomationLifecycleClock> =
        RefCell::new(AutomationLifecycleClock::new());
}

fn reset_automation_lifecycle_clock() {
    AUTOMATION_LIFECYCLE_CLOCK.with(|clock| {
        *clock.borrow_mut() = AutomationLifecycleClock::new();
    });
}

#[expect(
    dead_code,
    reason = "diagnostic lifecycle accessor reserved for future consumers"
)]
pub(crate) fn current_automation_revision() -> AutomationLifecycleRevision {
    AUTOMATION_LIFECYCLE_CLOCK.with(|clock| clock.borrow().current())
}

fn advance_automation_revision() -> Option<AutomationLifecycleRevision> {
    AUTOMATION_LIFECYCLE_CLOCK.with(|clock| match clock.borrow_mut().advance() {
        Ok(revision) => Some(revision),
        Err(error) => {
            tracing::error!(
                ?error,
                "Automation lifecycle revision exhausted; execution remains disabled"
            );
            None
        }
    })
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    reset_automation_lifecycle_clock();
    automation_backend::initialize(runtime.clone());
    extra_backend::initialize(runtime.clone());
    tray_backend::initialize(runtime);
}

pub(crate) fn clear() {
    tray_backend::clear();
    automation_backend::clear();
    extra_backend::clear();
    reset_automation_lifecycle_clock();
}

pub(crate) fn replace_automation_capabilities(snapshot: Arc<CapabilityRegistrySnapshot>) {
    match augment_snapshot_with_automation_shadow(snapshot.as_ref()) {
        Ok(augmented) => automation_backend::replace_capabilities(Arc::new(augmented)),
        Err(error) => {
            tracing::error!(
                error = ?error,
                generation = snapshot.generation(),
                "failed to augment Automation shadow capability; using unaugmented snapshot fail-closed"
            );
            automation_backend::replace_capabilities(snapshot);
        }
    }
}

pub(crate) fn observe_prepare_for_sleep(start: bool, observed_at: SystemTime) {
    let outcome = automation_backend::observe_prepare_for_sleep(start, observed_at);
    tracing::debug!(start, outcome = ?outcome, "Automation resume lifecycle observation");
}

pub(crate) fn observe_automation_telemetry(telemetry: &Telemetry) {
    let Some(status) = automation_backend::observe_telemetry(telemetry) else {
        return;
    };

    let revision = advance_automation_revision();
    match revision {
        Some(revision) => tracing::info!(
            status,
            revision = revision.get(),
            "Automation shadow transition"
        ),
        None => tracing::info!(
            status,
            "Automation lifecycle revision exhausted · execution disabled"
        ),
    }
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

    #[test]
    fn coordinator_wires_only_surviving_surfaces() {
        let source = include_str!("secondary_windows_backend.rs");
        assert!(source.contains("tray_backend::wire_app(app)"));
        assert!(source.contains("wire_app_window(app)"));
        assert!(source.contains("extra_backend::wire_window(app)"));
        assert!(source.contains("window_position_preferences_bridge::wire(app)"));
        assert!(source.contains("app.on_preview_dialog_clicked"));
        assert!(source.contains("app.on_quit_clicked"));
        assert!(source.contains("slint::quit_event_loop()"));
        assert!(!source.contains(&["on_", "extra_clicked"].concat()));
        assert!(!source.contains(&["on_", "automation_clicked"].concat()));
        assert!(!source.contains(&["on_", "preferences_clicked"].concat()));
        assert!(!source.contains(&["on_", "updates_clicked"].concat()));
        assert!(!source.contains(&["app.on_", "perf_clicked"].concat()));
        assert!(!source.contains(&["app.on_", "charge_changed"].concat()));
        assert!(!source.contains(&["app.on_", "fans_clicked"].concat()));
    }

    #[test]
    fn shadow_bridge_is_observation_only_revision_bound_and_explicitly_read_only() {
        let source = include_str!("secondary_windows_backend.rs");
        assert!(source.contains("observe_prepare_for_sleep"));
        assert!(source.contains("observe_automation_telemetry"));
        assert!(source.contains("replace_automation_capabilities"));
        assert!(source.contains("augment_snapshot_with_automation_shadow"));
        assert!(source.contains("AutomationLifecycleClock"));
        assert!(source.contains("advance_automation_revision"));
    }
}
