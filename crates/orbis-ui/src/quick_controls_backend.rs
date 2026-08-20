use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbis_core::display_output::DisplayOutputSnapshot;
use orbis_core::keyboard_backlight::KeyboardBacklightState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{DisplayOutputProvider, KeyboardBacklightProvider};
use orbis_providers::{
    AsusKeyboardBacklightProvider, WaylandCompositorOutputSource, WaylandDisplayOutputProvider,
};
use slint::ComponentHandle;

use crate::AppWindow;

#[derive(Clone)]
struct QuickControlsContext {
    runtime: tokio::runtime::Handle,
    refreshing: Arc<AtomicBool>,
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
    brightness: i32,
    status: String,
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(QuickControlsContext {
            runtime,
            refreshing: Arc::new(AtomicBool::new(false)),
            last_started: Arc::new(Mutex::new(None)),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn wire_window(app: &AppWindow) {
    app.set_display_state_ready(false);
    app.set_display_control_ready(false);
    app.set_display_mode(-1);
    app.set_display_status("Reading display state…".into());

    app.set_keyboard_state_ready(false);
    app.set_keyboard_control_ready(false);
    app.set_keyboard_brightness(-1);
    app.set_keyboard_status("Reading keyboard state…".into());

    app.on_display_mode_requested(|mode| {
        tracing::warn!(
            requested_mode = mode,
            "display mode request ignored: production display backend is read-only"
        );
    });
    app.on_keyboard_brightness_requested(|level| {
        tracing::warn!(
            requested_level = level,
            "keyboard brightness request ignored: production mutation is product-policy disabled"
        );
    });
}

pub(crate) fn force_refresh(app: &AppWindow) {
    refresh(app, None);
}

pub(crate) fn refresh_if_due(app: &AppWindow, minimum_interval: Duration) {
    refresh(app, Some(minimum_interval));
}

fn refresh(app: &AppWindow, minimum_interval: Option<Duration>) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        app.set_display_state_ready(false);
        app.set_display_control_ready(false);
        app.set_display_status("Display backend unavailable".into());
        app.set_keyboard_state_ready(false);
        app.set_keyboard_control_ready(false);
        app.set_keyboard_status("Keyboard backend unavailable".into());
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
        let display_provider =
            WaylandDisplayOutputProvider::new(WaylandCompositorOutputSource::new());
        let keyboard_provider = AsusKeyboardBacklightProvider::default();

        let (display_result, keyboard_result) = tokio::join!(
            display_provider.display_output_snapshot(),
            keyboard_provider.keyboard_backlight_state(),
        );

        let display = display_state(display_result);
        let keyboard = keyboard_state(keyboard_result);
        completion.store(false, Ordering::Release);

        if let Err(error) = weak.upgrade_in_event_loop(move |app| {
            app.set_display_state_ready(display.state_ready);
            app.set_display_control_ready(false);
            app.set_display_mode(display.mode);
            app.set_display_status(display.status.into());

            app.set_keyboard_state_ready(keyboard.state_ready);
            // Hardware1 has a typed keyboard mutation implementation, but the
            // production daemon intentionally composes a disabled backend and
            // reports Unsupported. Keep the UI request surface fail-closed.
            app.set_keyboard_control_ready(false);
            app.set_keyboard_brightness(keyboard.brightness);
            app.set_keyboard_status(keyboard.status.into());
        }) {
            tracing::warn!(error = ?error, "failed to publish quick-control state to UI");
        }
    });
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
                status: format!(
                    "{} · {} · read only",
                    output.id,
                    refresh_label(refresh_mhz)
                ),
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
    if refresh_mhz % 1_000 == 0 {
        return format!("{} Hz", refresh_mhz / 1_000);
    }
    format!("{:.2} Hz", refresh_mhz as f64 / 1_000.0)
}

fn keyboard_state(result: Result<KeyboardBacklightState, ProviderError>) -> KeyboardUiState {
    match result {
        Ok(state) => KeyboardUiState {
            state_ready: true,
            brightness: i32::from(state.current.get()),
            status: format!(
                "Level {}/{} · write disabled",
                state.current.get(),
                state.max.get()
            ),
        },
        Err(error) => KeyboardUiState {
            state_ready: false,
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
        _ => format!("{prefix} state unavailable"),
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
    fn keyboard_mapping_preserves_hardware_max_and_never_claims_write() {
        let state = keyboard_state(Ok(KeyboardBacklightState {
            current: KeyboardBrightnessLevel::new(2),
            max: KeyboardBrightnessLevel::new(4),
        }));
        assert!(state.state_ready);
        assert_eq!(state.brightness, 2);
        assert!(state.status.contains("2/4"));
        assert!(state.status.contains("write disabled"));
    }
}
