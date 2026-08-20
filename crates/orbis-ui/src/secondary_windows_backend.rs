//! Compatibility coordinator for secondary-window backend bridges.
//!
//! `main.rs` still owns the native secondary-window handles. The existing
//! `quick_controls_backend::{initialize,wire_window,clear}` lifecycle hook is
//! called after the legacy AppWindow callbacks are registered, so this module
//! installs the narrow secondary/lifecycle bridges without duplicating hardware
//! mutation ownership.

#[path = "action_dialog_backend.rs"]
mod action_dialog_backend;
#[path = "automation_backend.rs"]
mod automation_backend;
#[path = "extra_backend.rs"]
mod extra_backend;
#[path = "tray_backend.rs"]
mod tray_backend;
#[path = "updates_backend.rs"]
mod updates_backend;
#[path = "window_lifecycle_backend.rs"]
mod window_lifecycle_backend;
#[path = "window_position_preferences_bridge.rs"]
mod window_position_preferences_bridge;

use std::cell::RefCell;
use std::sync::Arc;
use std::time::SystemTime;

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_core::telemetry::Telemetry;
use orbis_ui::automation_capability::augment_snapshot_with_automation_shadow;
use orbis_ui::automation_lifecycle_revision::{
    AutomationLifecycleClock, AutomationLifecycleRevision,
};
use slint::ComponentHandle;

use crate::{AppWindow, AutomationWindow, ExtraWindow, ThemeState, UpdatesWindow};

thread_local! {
    static AUTOMATION_LIFECYCLE_CLOCK: RefCell<AutomationLifecycleClock> =
        RefCell::new(AutomationLifecycleClock::new());
}

fn reset_automation_lifecycle_clock() {
    AUTOMATION_LIFECYCLE_CLOCK.with(|clock| {
        *clock.borrow_mut() = AutomationLifecycleClock::new();
    });
}

pub(crate) fn current_automation_revision() -> AutomationLifecycleRevision {
    AUTOMATION_LIFECYCLE_CLOCK.with(|clock| clock.borrow().current())
}

fn advance_automation_revision() -> Option<AutomationLifecycleRevision> {
    AUTOMATION_LIFECYCLE_CLOCK.with(|clock| match clock.borrow_mut().advance() {
        Ok(revision) => Some(revision),
        Err(error) => {
            tracing::error!(?error, "Automation lifecycle revision exhausted; execution remains disabled");
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
                ?error,
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
    let status = match revision {
        Some(revision) => format!("{status} · revision {}", revision.get()),
        None => "Automation lifecycle revision exhausted · execution disabled".to_string(),
    };

    crate::AUTOMATION_WINDOW.with(|slot| {
        let slot = slot.borrow();
        let Some(window) = slot.as_ref() else {
            return;
        };
        if !window.get_saving() {
            window.set_runtime_ready(false);
            window.set_status(status.into());
        }
    });
}

fn show_extra_window() -> Result<(), slint::PlatformError> {
    crate::EXTRA_WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let window = ExtraWindow::new()?;
            extra_backend::wire_window(&window);
            *slot = Some(window);
        }
        let window = slot.as_ref().expect("ExtraWindow initialized");
        window
            .global::<ThemeState>()
            .set_mode(crate::current_theme_mode());
        window.show()?;
        extra_backend::refresh(window);
        Ok(())
    })
}

fn show_automation_window() -> Result<(), slint::PlatformError> {
    crate::AUTOMATION_WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let window = AutomationWindow::new()?;
            automation_backend::wire_window(&window);
            *slot = Some(window);
        }
        let window = slot.as_ref().expect("AutomationWindow initialized");
        window
            .global::<ThemeState>()
            .set_mode(crate::current_theme_mode());
        window.show()?;
        if !window.get_backend_ready() && !window.get_saving() {
            automation_backend::reload(window);
        }
        Ok(())
    })
}

fn show_preferences_window(app: &AppWindow) -> Result<(), slint::PlatformError> {
    crate::show_preferences_window(app)?;
    crate::PREFERENCES_WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref() {
            window_position_preferences_bridge::wire(window, app);
        }
    });
    Ok(())
}

fn show_updates_window(app: &AppWindow) -> Result<(), slint::PlatformError> {
    crate::UPDATES_WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let window = UpdatesWindow::new()?;
            updates_backend::wire(&window);
            *slot = Some(window);
        }
        let window = slot.as_ref().expect("UpdatesWindow initialized");
        window.set_version(app.get_ui_state().version.clone());
        window
            .global::<ThemeState>()
            .set_mode(crate::current_theme_mode());
        updates_backend::refresh(window);
        window.show()
    })
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

    app.on_extra_clicked(|| {
        if let Err(error) = show_extra_window() {
            tracing::warn!(error = ?error, "failed to open wired ExtraWindow");
        }
    });

    app.on_automation_clicked(|| {
        if let Err(error) = show_automation_window() {
            tracing::warn!(error = ?error, "failed to open wired AutomationWindow");
        }
    });

    {
        let weak = app.as_weak();
        app.on_preferences_clicked(move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if let Err(error) = show_preferences_window(&app) {
                tracing::warn!(error = ?error, "failed to open wired PreferencesWindow");
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_updates_clicked(move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if let Err(error) = show_updates_window(&app) {
                tracing::warn!(error = ?error, "failed to open wired UpdatesWindow");
            }
        });
    }

    app.on_preview_dialog_clicked(|kind| {
        if let Err(error) = show_preview_dialog(kind) {
            tracing::warn!(error = ?error, "failed to open fail-closed action dialog");
        }
    });

    // Explicit Quit is never reinterpreted as CloseAction::HideToTray. It
    // persists position best-effort and terminates even when a tray host exists.
    {
        let weak = app.as_weak();
        app.on_quit_clicked(move || {
            if let Some(app) = weak.upgrade() {
                let _ = window_lifecycle_backend::persist_position(&app);
            }
            if let Err(error) = slint::quit_event_loop() {
                tracing::warn!(error = ?error, "explicit Quit could not terminate Slint event loop");
            }
        });
    }
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
            assert!(!source.contains(&needle), "unexpected mutation token: {needle}");
        }
    }

    #[test]
    fn coordinator_replaces_secondary_and_desktop_lifecycle_callbacks() {
        let source = include_str!("secondary_windows_backend.rs");
        assert!(source.contains("app.on_extra_clicked"));
        assert!(source.contains("app.on_automation_clicked"));
        assert!(source.contains("app.on_preferences_clicked"));
        assert!(source.contains("app.on_updates_clicked"));
        assert!(source.contains("app.on_preview_dialog_clicked"));
        assert!(source.contains("app.on_quit_clicked"));
        assert!(source.contains("updates_backend::wire"));
        assert!(source.contains("action_dialog_backend::wire"));
        assert!(source.contains("slint::quit_event_loop()"));
        assert!(source.contains("tray_backend::wire_app(app)"));
        assert!(source.contains("wire_app_window(app)"));
        assert!(!source.contains("app.on_perf_clicked"));
        assert!(!source.contains("app.on_charge_changed"));
        assert!(!source.contains("app.on_fans_clicked"));
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
        assert!(source.contains("set_runtime_ready(false)"));
    }
}
