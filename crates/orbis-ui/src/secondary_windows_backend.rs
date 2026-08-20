//! Compatibility coordinator for secondary-window backend bridges.
//!
//! `main.rs` still owns the native secondary-window handles. The existing
//! `quick_controls_backend::{initialize,wire_window,clear}` lifecycle hook is
//! called after the legacy AppWindow callbacks are registered, so this module
//! uses that hook to replace only Extra/Automation open handlers without a
//! high-risk full replacement of the large entrypoint. The feature backends
//! remain separate files and this coordinator contains no hardware mutation.

#[path = "automation_backend.rs"]
mod automation_backend;
#[path = "extra_backend.rs"]
mod extra_backend;

use std::sync::Arc;

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_core::telemetry::Telemetry;
use slint::ComponentHandle;

use crate::{AppWindow, AutomationWindow, ExtraWindow, ThemeState};

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    automation_backend::initialize(runtime.clone());
    extra_backend::initialize(runtime);
}

pub(crate) fn clear() {
    automation_backend::clear();
    extra_backend::clear();
}

/// Publish one immutable capability generation to the Automation shadow
/// runtime. No capability is mutated or synthesized here.
pub(crate) fn replace_automation_capabilities(snapshot: Arc<CapabilityRegistrySnapshot>) {
    automation_backend::replace_capabilities(snapshot);
}

/// Feed one successful authoritative telemetry snapshot to Automation shadow
/// observation. A notice is surfaced only for a confirmed transition that
/// reached freshness/preflight evaluation; ordinary polling does not churn the
/// Automation status line.
pub(crate) fn observe_automation_telemetry(telemetry: &Telemetry) {
    let Some(status) = automation_backend::observe_telemetry(telemetry) else {
        return;
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

        // Preserve an editable unsaved draft while the persistence backend is
        // healthy. A failed initial load/save can recover on the next open by
        // reloading authoritative disk state.
        if !window.get_backend_ready() && !window.get_saving() {
            automation_backend::reload(window);
        }
        Ok(())
    })
}

/// Replace only the legacy Extra/Automation open callbacks.
///
/// `quick_controls_backend::wire_window` is called after `wire_callbacks` in
/// the current entrypoint, so these registrations become the active handlers.
/// No other AppWindow callback is changed here.
pub(crate) fn wire_window(app: &AppWindow) {
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn coordinator_contains_no_hardware_mutation_surface() {
        let source = include_str!("secondary_windows_backend.rs");
        for needle in [
            "WorkerCommand::Set",
            "set_fan_curve(",
            "set_gpu_mode(",
            "set_charge_limit(",
            "set_keyboard_backlight(",
        ] {
            assert!(!source.contains(needle), "unexpected mutation token: {needle}");
        }
    }

    #[test]
    fn coordinator_only_replaces_extra_and_automation_app_callbacks() {
        let source = include_str!("secondary_windows_backend.rs");
        assert!(source.contains("app.on_extra_clicked"));
        assert!(source.contains("app.on_automation_clicked"));
        assert!(!source.contains("app.on_perf_clicked"));
        assert!(!source.contains("app.on_charge_changed"));
        assert!(!source.contains("app.on_fans_clicked"));
    }

    #[test]
    fn shadow_bridge_is_observation_only() {
        let source = include_str!("secondary_windows_backend.rs");
        assert!(source.contains("observe_automation_telemetry"));
        assert!(source.contains("replace_automation_capabilities"));
        assert!(source.contains("set_runtime_ready(false)"));
    }
}
