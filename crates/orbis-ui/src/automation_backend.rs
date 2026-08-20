use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use orbis_config::{
    AutomationPolicy, DesiredDisplayPolicy, DesiredGpuPolicy, DesiredLightingPolicy,
    DesiredPerformancePolicy, DesiredStateDocument, OrbisDesiredState, load_desired_state,
    save_desired_state,
};
use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;
use slint::ComponentHandle;

use crate::AutomationWindow;

#[derive(Clone)]
struct AutomationContext {
    runtime: tokio::runtime::Handle,
    draft: Arc<Mutex<AutomationPolicy>>,
}

thread_local! {
    static CONTEXT: RefCell<Option<AutomationContext>> = const { RefCell::new(None) };
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(AutomationContext {
            runtime,
            draft: Arc::new(Mutex::new(AutomationPolicy::default())),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

fn performance_index(value: DesiredPerformancePolicy) -> i32 {
    match value {
        DesiredPerformancePolicy::Profile(PerformanceProfile::Silent) => 0,
        DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced) => 1,
        DesiredPerformancePolicy::Profile(PerformanceProfile::Turbo) => 2,
        DesiredPerformancePolicy::KeepCurrent => 3,
    }
}

fn performance_from_index(index: i32) -> Option<DesiredPerformancePolicy> {
    match index {
        0 => Some(DesiredPerformancePolicy::Profile(PerformanceProfile::Silent)),
        1 => Some(DesiredPerformancePolicy::Profile(PerformanceProfile::Balanced)),
        2 => Some(DesiredPerformancePolicy::Profile(PerformanceProfile::Turbo)),
        3 => Some(DesiredPerformancePolicy::KeepCurrent),
        _ => None,
    }
}

fn gpu_index(value: DesiredGpuPolicy) -> i32 {
    match value {
        DesiredGpuPolicy::Mode(GpuMode::Eco) => 0,
        DesiredGpuPolicy::Mode(GpuMode::Standard) => 1,
        DesiredGpuPolicy::Mode(GpuMode::Optimized) => 2,
        DesiredGpuPolicy::KeepCurrent => 3,
        // Ultimate is intentionally not a policy choice in this UI because it
        // may require a reboot/confirmation path and must never be synthesized.
        DesiredGpuPolicy::Mode(GpuMode::Ultimate) => 3,
    }
}

fn gpu_from_index(index: i32) -> Option<DesiredGpuPolicy> {
    match index {
        0 => Some(DesiredGpuPolicy::Mode(GpuMode::Eco)),
        1 => Some(DesiredGpuPolicy::Mode(GpuMode::Standard)),
        2 => Some(DesiredGpuPolicy::Mode(GpuMode::Optimized)),
        3 => Some(DesiredGpuPolicy::KeepCurrent),
        _ => None,
    }
}

fn display_index(value: DesiredDisplayPolicy) -> i32 {
    match value {
        DesiredDisplayPolicy::Hz60 => 0,
        DesiredDisplayPolicy::Hz120 => 1,
        DesiredDisplayPolicy::Auto => 2,
        DesiredDisplayPolicy::KeepCurrent => 3,
    }
}

fn display_from_index(index: i32) -> Option<DesiredDisplayPolicy> {
    match index {
        0 => Some(DesiredDisplayPolicy::Hz60),
        1 => Some(DesiredDisplayPolicy::Hz120),
        2 => Some(DesiredDisplayPolicy::Auto),
        3 => Some(DesiredDisplayPolicy::KeepCurrent),
        _ => None,
    }
}

fn lighting_index(value: DesiredLightingPolicy) -> i32 {
    match value {
        DesiredLightingPolicy::Off => 0,
        DesiredLightingPolicy::Dim => 1,
        DesiredLightingPolicy::Normal => 2,
        DesiredLightingPolicy::KeepCurrent => 3,
    }
}

fn lighting_from_index(index: i32) -> Option<DesiredLightingPolicy> {
    match index {
        0 => Some(DesiredLightingPolicy::Off),
        1 => Some(DesiredLightingPolicy::Dim),
        2 => Some(DesiredLightingPolicy::Normal),
        3 => Some(DesiredLightingPolicy::KeepCurrent),
        _ => None,
    }
}

fn publish_policy(window: &AutomationWindow, policy: &AutomationPolicy, status: &str) {
    window.set_enabled(policy.enabled);
    window.set_ac_profile(performance_index(policy.ac.performance));
    window.set_battery_profile(performance_index(policy.battery.performance));
    window.set_ac_gpu(gpu_index(policy.ac.gpu));
    window.set_battery_gpu(gpu_index(policy.battery.gpu));
    window.set_ac_display(display_index(policy.ac.display));
    window.set_battery_display(display_index(policy.battery.display));
    window.set_ac_lighting(lighting_index(policy.ac.lighting));
    window.set_battery_lighting(lighting_index(policy.battery.lighting));
    window.set_on_resume(policy.on_resume);
    window.set_on_ac_change(policy.on_ac_change);
    window.set_reconcile_only(policy.reconcile_only);
    window.set_notify_transitions(policy.notify_transitions);
    window.set_runtime_ready(false);
    window.set_backend_ready(true);
    window.set_saving(false);
    window.set_status(status.into());
}

fn load_policy() -> anyhow::Result<AutomationPolicy> {
    let load = load_desired_state::<OrbisDesiredState>()
        .context("load typed desired state for Automation")?;
    if let Some(warning) = load.warning {
        anyhow::bail!(
            "preserved desired-state source {:?} ({:?}); refusing editable Automation state",
            warning.path,
            warning.kind
        );
    }
    Ok(load.state.into_desired().automation)
}

fn save_policy(policy: AutomationPolicy) -> anyhow::Result<AutomationPolicy> {
    let load = load_desired_state::<OrbisDesiredState>()
        .context("reload typed desired state before Automation save")?;
    if let Some(warning) = load.warning {
        anyhow::bail!(
            "preserved desired-state source {:?} ({:?}); refusing overwrite",
            warning.path,
            warning.kind
        );
    }

    let mut desired = load.state.into_desired();
    desired.automation = policy;
    save_desired_state(&DesiredStateDocument::new(desired))
        .context("persist Automation policy desired state")?;

    load_policy().context("authoritative Automation policy read-back")
}

fn with_draft(mutator: impl FnOnce(&mut AutomationPolicy)) -> bool {
    CONTEXT.with(|slot| {
        let Some(context) = slot.borrow().as_ref().cloned() else {
            return false;
        };
        match context.draft.lock() {
            Ok(mut draft) => {
                mutator(&mut draft);
                true
            }
            Err(_) => {
                tracing::warn!("Automation draft lock poisoned");
                false
            }
        }
    })
}

fn current_draft() -> Option<AutomationPolicy> {
    CONTEXT.with(|slot| {
        let context = slot.borrow().as_ref()?.clone();
        match context.draft.lock() {
            Ok(draft) => Some(draft.clone()),
            Err(_) => {
                tracing::warn!("Automation draft lock poisoned");
                None
            }
        }
    })
}

fn republish_draft(window: &AutomationWindow) {
    if let Some(draft) = current_draft() {
        publish_policy(
            window,
            &draft,
            "Policy draft changed · Save persists only; runtime reconciliation unavailable",
        );
    } else {
        window.set_backend_ready(false);
        window.set_runtime_ready(false);
        window.set_status("Automation draft unavailable".into());
    }
}

pub(crate) fn reload(window: &AutomationWindow) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        window.set_backend_ready(false);
        window.set_runtime_ready(false);
        window.set_status("Automation persistence backend unavailable".into());
        return;
    };

    window.set_backend_ready(false);
    window.set_runtime_ready(false);
    window.set_saving(true);
    window.set_status("Loading Automation policy…".into());

    let weak = window.as_weak();
    let draft_store = context.draft.clone();
    context.runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(load_policy).await;
        if let Err(error) = weak.upgrade_in_event_loop(move |window| match result {
            Ok(Ok(policy)) => {
                let stored = match draft_store.lock() {
                    Ok(mut draft) => {
                        *draft = policy.clone();
                        true
                    }
                    Err(_) => false,
                };
                if stored {
                    publish_policy(
                        &window,
                        &policy,
                        "Policy loaded · persistence ready · runtime reconciliation unavailable",
                    );
                } else {
                    window.set_saving(false);
                    window.set_backend_ready(false);
                    window.set_runtime_ready(false);
                    window.set_status("Automation draft unavailable".into());
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "Automation policy load failed");
                window.set_saving(false);
                window.set_backend_ready(false);
                window.set_runtime_ready(false);
                window.set_status("Automation policy preserved/unavailable · editing disabled".into());
            }
            Err(error) => {
                tracing::warn!(error = %error, "Automation policy load task failed");
                window.set_saving(false);
                window.set_backend_ready(false);
                window.set_runtime_ready(false);
                window.set_status("Automation policy load failed".into());
            }
        }) {
            tracing::warn!(error = ?error, "failed to publish Automation policy load result");
        }
    });
}

fn save(window: &AutomationWindow) {
    if !window.get_backend_ready() || window.get_saving() {
        return;
    }
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        window.set_backend_ready(false);
        window.set_runtime_ready(false);
        window.set_status("Automation persistence backend unavailable".into());
        return;
    };
    let Some(policy) = current_draft() else {
        window.set_backend_ready(false);
        window.set_runtime_ready(false);
        window.set_status("Automation draft unavailable".into());
        return;
    };

    window.set_saving(true);
    window.set_status("Saving policy draft…".into());
    let weak = window.as_weak();
    let draft_store = context.draft.clone();
    context.runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(move || save_policy(policy)).await;
        if let Err(error) = weak.upgrade_in_event_loop(move |window| match result {
            Ok(Ok(read_back)) => {
                let stored = match draft_store.lock() {
                    Ok(mut draft) => {
                        *draft = read_back.clone();
                        true
                    }
                    Err(_) => false,
                };
                if stored {
                    publish_policy(
                        &window,
                        &read_back,
                        "Policy saved and read back · runtime reconciliation unavailable",
                    );
                } else {
                    window.set_saving(false);
                    window.set_backend_ready(false);
                    window.set_runtime_ready(false);
                    window.set_status("Policy saved but draft state unavailable".into());
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "Automation policy save failed");
                window.set_saving(false);
                window.set_backend_ready(false);
                window.set_runtime_ready(false);
                window.set_status("Automation policy save failed · reload required".into());
            }
            Err(error) => {
                tracing::warn!(error = %error, "Automation policy save task failed");
                window.set_saving(false);
                window.set_backend_ready(false);
                window.set_runtime_ready(false);
                window.set_status("Automation policy save failed · reload required".into());
            }
        }) {
            tracing::warn!(error = ?error, "failed to publish Automation save result");
        }
    });
}

pub(crate) fn wire_window(window: &AutomationWindow) {
    window.set_backend_ready(false);
    window.set_runtime_ready(false);
    window.set_saving(false);
    window.set_status("Loading Automation policy…".into());

    // Runtime execution does not exist yet. The requested enable transition is
    // registered fail-closed so a future UI regression cannot turn persistence
    // into apparent active automation.
    window.on_enabled_requested(|requested| {
        tracing::warn!(
            requested,
            "Automation enable request ignored: runtime reconciliation is not connected"
        );
    });

    macro_rules! wire_index {
        ($callback:ident, $field:expr, $map:ident) => {{
            let weak = window.as_weak();
            window.$callback(move |index| {
                let Some(value) = $map(index) else {
                    tracing::warn!(index, "invalid Automation policy index ignored");
                    return;
                };
                if with_draft(|draft| $field(draft, value)) {
                    if let Some(window) = weak.upgrade() {
                        republish_draft(&window);
                    }
                }
            });
        }};
    }

    wire_index!(on_ac_profile_requested, |draft: &mut AutomationPolicy, value| draft.ac.performance = value, performance_from_index);
    wire_index!(on_battery_profile_requested, |draft: &mut AutomationPolicy, value| draft.battery.performance = value, performance_from_index);
    wire_index!(on_ac_gpu_requested, |draft: &mut AutomationPolicy, value| draft.ac.gpu = value, gpu_from_index);
    wire_index!(on_battery_gpu_requested, |draft: &mut AutomationPolicy, value| draft.battery.gpu = value, gpu_from_index);
    wire_index!(on_ac_display_requested, |draft: &mut AutomationPolicy, value| draft.ac.display = value, display_from_index);
    wire_index!(on_battery_display_requested, |draft: &mut AutomationPolicy, value| draft.battery.display = value, display_from_index);
    wire_index!(on_ac_lighting_requested, |draft: &mut AutomationPolicy, value| draft.ac.lighting = value, lighting_from_index);
    wire_index!(on_battery_lighting_requested, |draft: &mut AutomationPolicy, value| draft.battery.lighting = value, lighting_from_index);

    macro_rules! wire_bool {
        ($callback:ident, $field:ident) => {{
            let weak = window.as_weak();
            window.$callback(move |value| {
                if with_draft(|draft| draft.$field = value) {
                    if let Some(window) = weak.upgrade() {
                        republish_draft(&window);
                    }
                }
            });
        }};
    }

    wire_bool!(on_on_resume_requested, on_resume);
    wire_bool!(on_on_ac_change_requested, on_ac_change);
    wire_bool!(on_reconcile_only_requested, reconcile_only);
    wire_bool!(on_notify_transitions_requested, notify_transitions);

    {
        let weak = window.as_weak();
        window.on_reset_requested(move || {
            if with_draft(|draft| *draft = AutomationPolicy::default()) {
                if let Some(window) = weak.upgrade() {
                    republish_draft(&window);
                }
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_save_requested(move || {
            if let Some(window) = weak.upgrade() {
                save(&window);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_index_roundtrips_are_exact_and_ultimate_is_never_generated() {
        for index in 0..=3 {
            let value = performance_from_index(index).unwrap();
            assert_eq!(performance_index(value), index);

            let gpu = gpu_from_index(index).unwrap();
            assert!(!matches!(gpu, DesiredGpuPolicy::Mode(GpuMode::Ultimate)));
            assert_eq!(gpu_index(gpu), index);

            let display = display_from_index(index).unwrap();
            assert_eq!(display_index(display), index);

            let lighting = lighting_from_index(index).unwrap();
            assert_eq!(lighting_index(lighting), index);
        }
    }

    #[test]
    fn persistence_backend_contains_no_reconciliation_or_provider_commands() {
        let source = include_str!("automation_backend.rs");
        for needle in [
            "WorkerCommand::Set",
            "set_profile(",
            "set_mode(",
            "set_refresh_rate(",
            "set_keyboard_backlight(",
            "set_fan_curve(",
        ] {
            assert!(!source.contains(needle), "unexpected execution token: {needle}");
        }
        assert!(source.contains("set_runtime_ready(false)"));
    }
}
