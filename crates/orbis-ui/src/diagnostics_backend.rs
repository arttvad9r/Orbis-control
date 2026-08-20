use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_ui::diagnostics_dto::DiagnosticsUiDto;
use orbis_ui::diagnostics_runtime::DiagnosticsRuntime;
use orbis_ui::diagnostics_window_model::DiagnosticsWindowModel;
use slint::ComponentHandle;

use crate::DiagnosticsWindow;

#[derive(Clone)]
struct DiagnosticsContext {
    runtime: tokio::runtime::Handle,
    source: DiagnosticsRuntime,
}

thread_local! {
    static CONTEXT: RefCell<Option<DiagnosticsContext>> = const { RefCell::new(None) };
}

pub(crate) fn initialize(
    runtime: tokio::runtime::Handle,
    session_connection: zbus::Connection,
    system_connection: zbus::Connection,
    capabilities: Arc<CapabilityRegistrySnapshot>,
    telemetry_freshness_threshold: Duration,
) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(DiagnosticsContext {
            runtime,
            source: DiagnosticsRuntime::new(
                session_connection,
                system_connection,
                capabilities,
                telemetry_freshness_threshold,
            ),
        });
    });
}

pub(crate) fn replace_capabilities(snapshot: Arc<CapabilityRegistrySnapshot>) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow().as_ref() {
            context.source.replace_capabilities(snapshot);
        }
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

fn apply_model(window: &DiagnosticsWindow, model: DiagnosticsWindowModel) {
    window.set_kernel_value(model.kernel.into());
    window.set_platform_value(model.platform.into());
    window.set_version_value(model.version.into());
    window.set_build_detail(model.build.into());
    window.set_system_detail(model.system.into());
    window.set_capabilities_text(model.capabilities.into());
    window.set_services_text(model.services.into());
    window.set_gpu_text(model.gpu.into());
    window.set_telemetry_text(model.telemetry.into());
    window.set_display_text(model.display.into());
    window.set_snapshot_meta(model.snapshot_meta.into());
}

pub(crate) fn refresh(window: &DiagnosticsWindow) {
    if window.get_refresh_pending() {
        return;
    }

    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        window.set_refresh_enabled(false);
        window.set_refresh_pending(false);
        window.set_local_status("Diagnostics runtime unavailable".into());
        return;
    };

    window.set_refresh_enabled(false);
    window.set_refresh_pending(true);
    window.set_local_status("Collecting read-only snapshot…".into());

    let weak = window.as_weak();
    context.runtime.spawn(async move {
        let snapshot = context.source.snapshot().await;
        let dto = DiagnosticsUiDto::from_snapshot(&snapshot);
        let model = DiagnosticsWindowModel::from_dto(&dto);

        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            apply_model(&window, model);
            window.set_refresh_pending(false);
            window.set_refresh_enabled(true);
            window.set_local_status("Snapshot refreshed".into());
            // These host actions intentionally remain unavailable until their
            // own handlers exist. A successful snapshot does not imply them.
            window.set_copy_enabled(false);
            window.set_logs_enabled(false);
            window.set_export_enabled(false);
        }) {
            tracing::warn!(error = ?error, "failed to publish diagnostics snapshot to UI");
        }
    });
}

pub(crate) fn wire_window(window: &DiagnosticsWindow) {
    window.set_refresh_enabled(CONTEXT.with(|slot| slot.borrow().is_some()));
    window.set_copy_enabled(false);
    window.set_logs_enabled(false);
    window.set_export_enabled(false);

    let weak = window.as_weak();
    window.on_refresh_requested(move || {
        if let Some(window) = weak.upgrade() {
            refresh(&window);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_application_is_pure_property_projection_contract() {
        // Keep this module's production boundary explicit: it depends only on
        // the read-only DiagnosticsRuntime and presentation projection types.
        let source = include_str!("diagnostics_backend.rs");
        assert!(!source.contains("WorkerCommand::Set"));
        assert!(!source.contains("set_fan_curve"));
        assert!(!source.contains("set_gpu_mode"));
        assert!(!source.contains("set_charge_limit"));
        assert!(!source.contains("set_performance"));
    }
}
