#[path = "hardware_controls_backend.rs"]
pub(crate) mod hardware_controls_backend;
#[path = "resume_observer.rs"]
mod resume_observer;
#[path = "secondary_windows_backend.rs"]
mod secondary_windows_backend;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hardware_controls_backend::{HardwareProductControlClient, ProductWriteStatus};
use orbis_core::aura::AuraRgb;
use orbis_core::display_output::DisplayOutputSnapshot;
use orbis_core::keyboard_backlight::KeyboardBacklightState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    DisplayOutputProvider, KeyboardBacklightProvider, TelemetryProvider,
};
use orbis_providers::{
    AsusKeyboardBacklightProvider, SysfsTelemetryProvider, WaylandCompositorOutputSource,
    WaylandDisplayOutputProvider,
};
use slint::ComponentHandle;

use crate::AppWindow;

const READ_TIMEOUT: Duration = Duration::from_secs(2);
const AUTOMATION_READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct QuickControlsContext {
    runtime: tokio::runtime::Handle,
    refreshing: Arc<AtomicBool>,
    automation_observing: Arc<AtomicBool>,
    resume_observer_started: Arc<AtomicBool>,
    last_started: Arc<Mutex<Option<Instant>>>,
}

thread_local! {
    static CONTEXT: RefCell<Option<QuickControlsContext>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayUiState {
    state_ready: bool,
    mode: i32,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyboardUiState {
    state_ready: bool,
    control_ready: bool,
    brightness: i32,
    status: String,
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    secondary_windows_backend::initialize(runtime.clone());
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(QuickControlsContext {
            runtime,
            refreshing: Arc::new(AtomicBool::new(false)),
            automation_observing: Arc::new(AtomicBool::new(false)),
            resume_observer_started: Arc::new(AtomicBool::new(false)),
            last_started: Arc::new(Mutex::new(None)),
        });
    });
}

pub(crate) fn clear() {
    secondary_windows_backend::clear();
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

/// Forward one immutable capability generation to the Automation shadow
/// runtime. This compatibility function exists until secondary-window
/// backends get a dedicated top-level lifecycle coordinator.
pub(crate) fn replace_automation_capabilities(
    snapshot: Arc<orbis_capabilities::CapabilityRegistrySnapshot>,
) {
    secondary_windows_backend::replace_automation_capabilities(snapshot);
}

/// Forward one successful authoritative telemetry snapshot to the Automation
/// shadow runtime. No mutation is performed by this path.
pub(crate) fn observe_automation_telemetry(telemetry: &orbis_core::telemetry::Telemetry) {
    secondary_windows_backend::observe_automation_telemetry(telemetry);
}

/// Frameless title bar close button shares the native close-action policy.
pub(crate) fn handle_close_request(app: &AppWindow) {
    secondary_windows_backend::handle_close_request(app);
}

/// Settings section remember-position/close-action request handlers.
pub(crate) fn wire_position_preferences_bridge(app: &AppWindow) {
    secondary_windows_backend::wire_position_preferences_bridge(app);
}

pub(crate) fn wire_window(app: &AppWindow) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let runtime_ready = context.is_some();

    // Start the logind observer only after an AppWindow exists so every signal
    // can be marshalled back onto the Slint event-loop thread before touching
    // thread-local backend state. Offscreen/test builds without initialize()
    // never attempt a system-bus connection.
    if let Some(context) = context.as_ref() {
        if !context.resume_observer_started.swap(true, Ordering::AcqRel) {
            resume_observer::spawn(context.runtime.clone(), app.as_weak());
        }
    }

    app.set_display_state_ready(false);
    app.set_display_control_ready(false);
    app.set_display_mode(-1);
    app.set_display_status(
        if runtime_ready {
            "Reading display state…"
        } else {
            "Display backend unavailable"
        }
        .into(),
    );

    app.set_keyboard_state_ready(false);
    app.set_keyboard_control_ready(false);
    app.set_keyboard_brightness(-1);
    app.set_aura_control_ready(false);

    app.on_display_mode_requested(|mode| {
        tracing::warn!(
            requested_mode = mode,
            "display mode request ignored: production display backend is read-only"
        );
    });

    {
        let weak = app.as_weak();
        let request_context = context.clone();
        app.on_keyboard_brightness_requested(move |level| {
            let Some(context) = request_context.clone() else {
                tracing::warn!(
                    requested_level = level,
                    "keyboard request ignored: runtime absent"
                );
                return;
            };
            let Ok(level) = u8::try_from(level) else {
                tracing::warn!(
                    requested_level = level,
                    "keyboard request ignored: invalid level"
                );
                return;
            };
            let Some(app) = weak.upgrade() else {
                return;
            };
            if !app.get_keyboard_control_ready() {
                tracing::warn!(
                    requested_level = level,
                    "keyboard request ignored: write evidence unavailable"
                );
                return;
            }

            // Request-only semantics: disable editing immediately, but retain
            // the authoritative observed brightness until Hardware1 confirms a
            // fresh read-back. The client re-checks mutation status just before
            // the setter, so a stale UI readiness bit cannot authorize a write.
            app.set_keyboard_control_ready(false);
            let weak = app.as_weak();
            context.runtime.spawn(async move {
                let result = async {
                    let client = HardwareProductControlClient::connect_system().await?;
                    client.set_keyboard_backlight(level).await
                }
                .await;

                if let Err(error) = weak.upgrade_in_event_loop(move |app| {
                    match result {
                        Ok(observed) => {
                            app.set_keyboard_brightness(i32::from(observed));
                        }
                        Err(error) => {
                            tracing::warn!(error = ?error, "keyboard brightness mutation failed");
                        }
                    }
                    // Re-read both observed state and current mutation evidence;
                    // no success/readiness is inferred from the request itself.
                    refresh(&app, None);
                }) {
                    tracing::warn!(error = ?error, "failed to publish keyboard mutation result");
                }
            });
        });
    }

    {
        let weak = app.as_weak();
        let request_context = context.clone();
        app.on_aura_static_rgb_requested(move |r, g, b| {
            let Some(context) = request_context.clone() else {
                return;
            };
            let Some(app) = weak.upgrade() else {
                return;
            };
            if !app.get_aura_control_ready() {
                tracing::warn!("Aura request ignored: write evidence unavailable");
                return;
            }
            let rgb = AuraRgb {
                r: r as u8,
                g: g as u8,
                b: b as u8,
            };
            app.set_aura_control_ready(false);
            let result_weak = app.as_weak();
            context.runtime.spawn(async move {
                let result = async {
                    let client = HardwareProductControlClient::connect_system().await?;
                    client.set_aura_static_rgb(rgb).await
                }
                .await;
                if let Err(error) = result_weak.upgrade_in_event_loop(move |app| {
                    app.set_status(match result {
                        Ok(observed) => format!(
                            "Aura Static RGB accepted · config read-back {:?}",
                            observed.observed
                        )
                        .into(),
                        Err(error) => format!("Aura write failed · {error}").into(),
                    });
                    force_refresh(&app);
                }) {
                    tracing::warn!(error = ?error, "failed to publish Aura mutation result");
                }
            });
        });
    }

    // The current entrypoint invokes this function after legacy AppWindow
    // callbacks are registered. The coordinator replaces only Extra/Automation
    // open handlers, allowing their backend bridges to be wired without a
    // high-risk full rewrite of main.rs.
    secondary_windows_backend::wire_window(app);
}

pub(crate) fn force_refresh(app: &AppWindow) {
    observe_automation_from_sysfs(app);
    refresh(app, None);
}

pub(crate) fn refresh_if_due(app: &AppWindow, minimum_interval: Duration) {
    // main.rs invokes this hook for every WorkerEvent::TelemetryRefresh. Keep
    // Automation observation independent from the slower 10s visual Quick
    // Controls refresh so the two-sample power-source debounce sees each normal
    // telemetry cadence. This is a temporary read-only compatibility bridge
    // until the entrypoint can pass the original WorkerEvent telemetry object.
    observe_automation_from_sysfs(app);
    refresh(app, Some(minimum_interval));
}

fn observe_automation_from_sysfs(app: &AppWindow) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        return;
    };
    let Some(capabilities) = crate::diagnostics_backend::current_capabilities() else {
        return;
    };
    if context.automation_observing.swap(true, Ordering::AcqRel) {
        return;
    }

    let completion = context.automation_observing.clone();
    let weak = app.as_weak();
    context.runtime.spawn(async move {
        let provider = SysfsTelemetryProvider::default();
        let telemetry = tokio::time::timeout(AUTOMATION_READ_TIMEOUT, provider.snapshot()).await;
        completion.store(false, Ordering::Release);

        match telemetry {
            Ok(Ok(telemetry)) => {
                if let Err(error) = weak.upgrade_in_event_loop(move |_app| {
                    // Diagnostics owns the whole-swap capability snapshot and is
                    // updated by RegistryChange in main.rs. Clone exactly that
                    // immutable generation into shadow preflight before feeding
                    // the fresh typed telemetry sample.
                    replace_automation_capabilities(capabilities);
                    observe_automation_telemetry(&telemetry);
                }) {
                    tracing::warn!(error = ?error, "failed to publish Automation shadow observation");
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(error = ?error, "Automation shadow telemetry read unavailable");
            }
            Err(_) => {
                tracing::debug!("Automation shadow telemetry read timed out");
            }
        }
    });
}

fn refresh(app: &AppWindow, minimum_interval: Option<Duration>) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        app.set_display_state_ready(false);
        app.set_display_control_ready(false);
        app.set_display_status("Display backend unavailable".into());
        app.set_keyboard_state_ready(false);
        app.set_keyboard_control_ready(false);
        return;
    };

    if context.refreshing.swap(true, Ordering::AcqRel) {
        return;
    }

    let now = Instant::now();
    {
        let mut last = context
            .last_started
            .lock()
            .expect("quick-controls refresh timestamp lock poisoned");
        if let (Some(minimum), Some(previous)) = (minimum_interval, *last) {
            if now.duration_since(previous) < minimum {
                context.refreshing.store(false, Ordering::Release);
                return;
            }
        }
        *last = Some(now);
    }

    let weak = app.as_weak();
    let completion = context.refreshing.clone();
    context.runtime.spawn(async move {
        let (display_result, keyboard_result, keyboard_write_result, aura_write_result) = tokio::join!(
            read_display_state(),
            read_keyboard_state(),
            read_keyboard_write_status(),
            read_aura_write_status(),
        );

        let display = display_state(display_result);
        let keyboard = keyboard_state(keyboard_result, keyboard_write_result);
        let aura_write = aura_write_result.unwrap_or(ProductWriteStatus::Unknown);
        completion.store(false, Ordering::Release);

        if let Err(error) = weak.upgrade_in_event_loop(move |app| {
            app.set_display_state_ready(display.state_ready);
            app.set_display_control_ready(false);
            app.set_display_mode(display.mode);
            app.set_display_status(display.status.into());

            app.set_keyboard_state_ready(keyboard.state_ready);
            app.set_keyboard_control_ready(keyboard.control_ready);
            app.set_keyboard_brightness(keyboard.brightness);
            app.set_aura_control_ready(aura_write.is_supported());
        }) {
            tracing::warn!(error = ?error, "failed to publish quick-control state to UI");
        }
    });
}

async fn read_display_state() -> Result<DisplayOutputSnapshot, ProviderError> {
    let provider = WaylandDisplayOutputProvider::new(WaylandCompositorOutputSource::new());
    tokio::time::timeout(READ_TIMEOUT, provider.display_output_snapshot())
        .await
        .map_err(|_| ProviderError::Timeout("quick controls display read timed out".into()))?
}

async fn read_keyboard_state() -> Result<KeyboardBacklightState, ProviderError> {
    let provider = AsusKeyboardBacklightProvider::default();
    tokio::time::timeout(READ_TIMEOUT, provider.keyboard_backlight_state())
        .await
        .map_err(|_| ProviderError::Timeout("quick controls keyboard read timed out".into()))?
}

async fn read_keyboard_write_status() -> Result<ProductWriteStatus, ProviderError> {
    let client = HardwareProductControlClient::connect_system().await?;
    client.keyboard_status().await
}

async fn read_aura_write_status() -> Result<ProductWriteStatus, ProviderError> {
    let client = HardwareProductControlClient::connect_system().await?;
    client.aura_status().await
}

fn display_state(result: Result<DisplayOutputSnapshot, ProviderError>) -> DisplayUiState {
    match result {
        Ok(snapshot) => display_state_from_snapshot(&snapshot),
        Err(error) => DisplayUiState {
            state_ready: false,
            mode: -1,
            status: read_error_status("Display", &error),
        },
    }
}

fn display_state_from_snapshot(snapshot: &DisplayOutputSnapshot) -> DisplayUiState {
    match snapshot.outputs.as_slice() {
        [] => DisplayUiState {
            state_ready: true,
            mode: -1,
            status: "No active outputs".into(),
        },
        [output] => {
            let refresh_mhz = output.current_mode.current.refresh.get();
            DisplayUiState {
                state_ready: true,
                mode: display_mode_bucket(refresh_mhz),
                status: format!("{} · {} · read only", output.id, refresh_label(refresh_mhz)),
            }
        }
        outputs => DisplayUiState {
            state_ready: true,
            mode: -1,
            status: format!("{} outputs · target ambiguous · read only", outputs.len()),
        },
    }
}

fn display_mode_bucket(refresh_mhz: u32) -> i32 {
    match refresh_mhz {
        59_000..=61_000 => 1,
        118_000..=122_000 => 2,
        _ => -1,
    }
}

fn refresh_label(refresh_mhz: u32) -> String {
    if refresh_mhz == 0 {
        return "refresh unknown".into();
    }
    if refresh_mhz.is_multiple_of(1_000) {
        return format!("{} Hz", refresh_mhz / 1_000);
    }
    format!("{:.2} Hz", refresh_mhz as f64 / 1_000.0)
}

fn keyboard_state(
    result: Result<KeyboardBacklightState, ProviderError>,
    write_status: Result<ProductWriteStatus, ProviderError>,
) -> KeyboardUiState {
    match result {
        Ok(state) => {
            let (control_ready, write_label) = match write_status {
                Ok(status) => (status.is_supported(), status.short_label().to_string()),
                Err(error) => (false, write_error_label(&error).to_string()),
            };
            KeyboardUiState {
                state_ready: true,
                control_ready,
                brightness: i32::from(state.current.get()),
                status: format!(
                    "Level {}/{} · {write_label}",
                    state.current.get(),
                    state.max.get()
                ),
            }
        }
        Err(error) => KeyboardUiState {
            state_ready: false,
            control_ready: false,
            brightness: -1,
            status: read_error_status("Keyboard", &error),
        },
    }
}

fn read_error_status(prefix: &str, error: &ProviderError) -> String {
    match error {
        ProviderError::Unsupported(_) => format!("{prefix} unsupported"),
        ProviderError::PermissionDenied(_) => format!("{prefix} read denied"),
        ProviderError::BackendUnavailable(_) => format!("{prefix} backend unavailable"),
        ProviderError::Timeout(_) => format!("{prefix} read timed out"),
        _ => format!("{prefix} state unavailable"),
    }
}

fn write_error_label(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::Unsupported(_) => "write disabled",
        ProviderError::PermissionDenied(_) => "write denied",
        ProviderError::BackendUnavailable(_) => "write unavailable",
        ProviderError::Timeout(_) => "write status timed out",
        _ => "write status unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::display_output::{
        CurrentDisplayMode, DisplayMode, DisplayOutputId, DisplayOutputState,
    };
    use orbis_core::keyboard_backlight::KeyboardBrightnessLevel;
    use orbis_core::newtypes::RefreshMilliHz;

    fn output(id: &str, refresh_mhz: u32) -> DisplayOutputState {
        DisplayOutputState {
            id: DisplayOutputId::new(id),
            current_mode: CurrentDisplayMode {
                current: DisplayMode::new(
                    2560,
                    1600,
                    RefreshMilliHz::new(refresh_mhz).expect("refresh"),
                ),
                preferred: None,
            },
            available_modes: Vec::new(),
        }
    }

    #[test]
    fn maps_common_refresh_rates_without_rounding_source_state() {
        assert_eq!(display_mode_bucket(60_000), 1);
        assert_eq!(display_mode_bucket(59_940), 1);
        assert_eq!(display_mode_bucket(120_000), 2);
        assert_eq!(display_mode_bucket(119_880), 2);
        assert_eq!(display_mode_bucket(165_000), -1);
        assert_eq!(refresh_label(59_940), "59.94 Hz");
    }

    #[test]
    fn multiple_outputs_never_guess_quick_control_target() {
        let state = display_state_from_snapshot(&DisplayOutputSnapshot {
            outputs: vec![output("eDP-1", 120_000), output("DP-1", 60_000)],
        });
        assert!(state.state_ready);
        assert_eq!(state.mode, -1);
        assert!(state.status.contains("target ambiguous"));
    }

    #[test]
    fn single_output_publishes_authoritative_observed_mode() {
        let state = display_state_from_snapshot(&DisplayOutputSnapshot {
            outputs: vec![output("eDP-1", 119_880)],
        });
        assert!(state.state_ready);
        assert_eq!(state.mode, 2);
        assert!(state.status.contains("119.88 Hz"));
        assert!(state.status.contains("read only"));
    }

    #[test]
    fn keyboard_mapping_preserves_hardware_max_and_effective_write_status() {
        let state = keyboard_state(
            Ok(KeyboardBacklightState {
                current: KeyboardBrightnessLevel::new(2),
                max: KeyboardBrightnessLevel::new(4),
            }),
            Ok(ProductWriteStatus::Unsupported),
        );
        assert!(state.state_ready);
        assert!(!state.control_ready);
        assert_eq!(state.brightness, 2);
        assert!(state.status.contains("2/4"));
        assert!(state.status.contains("write disabled"));

        let writable = keyboard_state(
            Ok(KeyboardBacklightState {
                current: KeyboardBrightnessLevel::new(1),
                max: KeyboardBrightnessLevel::new(4),
            }),
            Ok(ProductWriteStatus::Supported),
        );
        assert!(writable.control_ready);
        assert!(writable.status.contains("write ready"));
    }

    #[test]
    fn automation_forwarders_are_observation_only() {
        let source = include_str!("quick_controls_backend.rs");
        assert!(source.contains("replace_automation_capabilities"));
        assert!(source.contains("observe_automation_telemetry"));
        assert!(source.contains("resume_observer::spawn"));
        assert!(source.contains("SysfsTelemetryProvider"));
        assert!(source.contains("AUTOMATION_READ_TIMEOUT"));
        let mutation = ["WorkerCommand::", "Set"].concat();
        assert!(!source.contains(&mutation));
    }

    #[test]
    fn keyboard_mutation_is_status_gated_and_readback_driven() {
        let source = include_str!("quick_controls_backend.rs");
        assert!(source.contains("read_keyboard_write_status"));
        assert!(source.contains("get_keyboard_control_ready"));
        assert!(source.contains("set_keyboard_backlight(level)"));
        assert!(source.contains("refresh(&app, None)"));
    }
}
