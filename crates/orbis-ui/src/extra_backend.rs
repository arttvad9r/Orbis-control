use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbis_core::aura::{AuraMode, AuraSpeed};
use orbis_core::display::PanelOverdriveState;
use orbis_core::keyboard_backlight::KeyboardBacklightState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{AuraProvider, KeyboardBacklightProvider, PanelOverdriveProvider};
use orbis_providers::{
    AsusArmouryPanelOverdriveProvider, AsusAuraProvider, AsusKeyboardBacklightProvider,
};
use slint::ComponentHandle;

use crate::ExtraWindow;

const READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct ExtraContext {
    runtime: tokio::runtime::Handle,
    refreshing: Arc<AtomicBool>,
}

thread_local! {
    static CONTEXT: RefCell<Option<ExtraContext>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyboardObserved {
    ready: bool,
    brightness: i32,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuraObserved {
    ready: bool,
    effect: i32,
    speed: i32,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PanelObserved {
    ready: bool,
    enabled: bool,
    status: String,
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ExtraContext {
            runtime,
            refreshing: Arc::new(AtomicBool::new(false)),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

fn reset_readiness(window: &ExtraWindow) {
    window.set_backend_ready(false);
    window.set_keyboard_state_ready(false);
    window.set_keyboard_control_ready(false);
    window.set_aura_state_ready(false);
    window.set_aura_control_ready(false);
    window.set_panel_overdrive_state_ready(false);
    window.set_panel_overdrive_control_ready(false);
}

pub(crate) fn wire_window(window: &ExtraWindow) {
    reset_readiness(window);
    window.set_keyboard_brightness(-1);
    window.set_keyboard_effect(-1);
    window.set_keyboard_speed(-1);
    window.set_status(
        if CONTEXT.with(|slot| slot.borrow().is_some()) {
            "Reading advanced hardware observations…"
        } else {
            "Advanced controls backend unavailable"
        }
        .into(),
    );

    {
        let weak = window.as_weak();
        window.on_reload_requested(move || {
            if let Some(window) = weak.upgrade() {
                refresh(&window);
            }
        });
    }

    window.on_apply_requested(|| {
        tracing::warn!(
            "Extra Controls Apply ignored: no production advanced-mutation owner is connected"
        );
    });
}

pub(crate) fn refresh(window: &ExtraWindow) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        reset_readiness(window);
        window.set_status("Advanced controls backend unavailable".into());
        return;
    };

    if context.refreshing.swap(true, Ordering::AcqRel) {
        return;
    }

    window.set_backend_ready(false);
    window.set_keyboard_control_ready(false);
    window.set_aura_control_ready(false);
    window.set_panel_overdrive_control_ready(false);
    window.set_status("Refreshing read-only advanced state…".into());

    let weak = window.as_weak();
    let completion = context.refreshing.clone();
    context.runtime.spawn(async move {
        let keyboard_provider = AsusKeyboardBacklightProvider::default();
        let panel_provider = AsusArmouryPanelOverdriveProvider::default();

        // Keyboard and panel reads are independent of D-Bus. Aura obtains its
        // own bounded system-bus connection so this bridge can be bootstrapped
        // from the existing UI runtime hook without changing the large entrypoint.
        let (keyboard_result, aura_result, panel_result) = tokio::join!(
            bounded_keyboard_read(&keyboard_provider),
            bounded_aura_read(),
            bounded_panel_read(&panel_provider),
        );

        let keyboard = keyboard_observed(keyboard_result);
        let aura = aura_observed(aura_result);
        let panel = panel_observed(panel_result);
        completion.store(false, Ordering::Release);

        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            // These are observed values only. The corresponding mutation flags
            // remain false until a production write owner and capability
            // evidence are connected.
            window.set_backend_ready(false);

            window.set_keyboard_state_ready(keyboard.ready);
            window.set_keyboard_control_ready(false);
            window.set_keyboard_brightness(keyboard.brightness);

            window.set_aura_state_ready(aura.ready);
            window.set_aura_control_ready(false);
            window.set_keyboard_effect(aura.effect);
            window.set_keyboard_speed(aura.speed);

            window.set_panel_overdrive_state_ready(panel.ready);
            window.set_panel_overdrive_control_ready(false);
            window.set_panel_overdrive(panel.enabled);

            let any_ready = keyboard.ready || aura.ready || panel.ready;
            window.set_status(
                if any_ready {
                    format!(
                        "Observed · {} · {} · {} · writes disabled",
                        keyboard.status, aura.status, panel.status
                    )
                } else {
                    format!(
                        "Advanced observations unavailable · {} · {} · {}",
                        keyboard.status, aura.status, panel.status
                    )
                }
                .into(),
            );
        }) {
            tracing::warn!(error = ?error, "failed to publish Extra observed state to UI");
        }
    });
}

async fn bounded_keyboard_read(
    provider: &AsusKeyboardBacklightProvider,
) -> Result<KeyboardBacklightState, ProviderError> {
    tokio::time::timeout(READ_TIMEOUT, provider.keyboard_backlight_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra keyboard read timed out".into()))?
}

async fn bounded_aura_read() -> Result<orbis_core::aura::AuraState, ProviderError> {
    let connection = tokio::time::timeout(READ_TIMEOUT, zbus::Connection::system())
        .await
        .map_err(|_| ProviderError::Timeout("Extra Aura bus connect timed out".into()))?
        .map_err(|error| ProviderError::BackendUnavailable(format!(
            "Extra Aura system bus unavailable: {error}"
        )))?;
    let provider = AsusAuraProvider::new(connection);
    tokio::time::timeout(READ_TIMEOUT, provider.aura_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra Aura read timed out".into()))?
}

async fn bounded_panel_read(
    provider: &AsusArmouryPanelOverdriveProvider,
) -> Result<PanelOverdriveState, ProviderError> {
    tokio::time::timeout(READ_TIMEOUT, provider.panel_overdrive_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra panel-overdrive read timed out".into()))?
}

fn keyboard_observed(result: Result<KeyboardBacklightState, ProviderError>) -> KeyboardObserved {
    match result {
        Ok(state) => KeyboardObserved {
            ready: true,
            brightness: i32::from(state.current.get()),
            status: format!("keyboard {}/{}", state.current.get(), state.max.get()),
        },
        Err(error) => KeyboardObserved {
            ready: false,
            brightness: -1,
            status: error_status("keyboard", &error),
        },
    }
}

fn aura_observed(result: Result<orbis_core::aura::AuraState, ProviderError>) -> AuraObserved {
    match result {
        Ok(state) => AuraObserved {
            ready: true,
            effect: aura_effect_index(state.current_mode),
            speed: aura_speed_index(&state.current_effect.speed),
            status: format!(
                "Aura mode={} speed={}",
                aura_mode_label(state.current_mode),
                state.current_effect.speed.as_str()
            ),
        },
        Err(error) => AuraObserved {
            ready: false,
            effect: -1,
            speed: -1,
            status: error_status("Aura", &error),
        },
    }
}

fn panel_observed(result: Result<PanelOverdriveState, ProviderError>) -> PanelObserved {
    match result {
        Ok(state) => PanelObserved {
            ready: true,
            enabled: matches!(state, PanelOverdriveState::Enabled),
            status: if matches!(state, PanelOverdriveState::Enabled) {
                "panel OD on".into()
            } else {
                "panel OD off".into()
            },
        },
        Err(error) => PanelObserved {
            ready: false,
            enabled: false,
            status: error_status("panel OD", &error),
        },
    }
}

fn aura_effect_index(mode: AuraMode) -> i32 {
    match mode {
        AuraMode::Static => 0,
        AuraMode::Breathe => 1,
        AuraMode::RainbowCycle => 2,
        // The current Extra UI's fourth legacy label is "Strobing" while the
        // exact domain value is Flash. Do not claim that they are equivalent.
        AuraMode::Flash => -1,
        _ => -1,
    }
}

fn aura_speed_index(speed: &AuraSpeed) -> i32 {
    match speed {
        AuraSpeed::Low => 0,
        AuraSpeed::Med => 1,
        AuraSpeed::High => 2,
        AuraSpeed::Unknown(_) => -1,
    }
}

fn aura_mode_label(mode: AuraMode) -> String {
    match mode {
        AuraMode::Unknown(raw) => format!("unknown({raw})"),
        other => format!("{other:?}"),
    }
}

fn error_status(prefix: &str, error: &ProviderError) -> String {
    let reason = match error {
        ProviderError::Unsupported(_) => "unsupported",
        ProviderError::BackendUnavailable(_) => "backend missing",
        ProviderError::PermissionDenied(_) => "read denied",
        ProviderError::Timeout(_) => "timed out",
        _ => "unavailable",
    };
    format!("{prefix} {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::aura::{
        AuraBrightness, AuraDirection, AuraEffect, AuraRgb, AuraState, AuraZone,
    };
    use orbis_core::keyboard_backlight::KeyboardBrightnessLevel;

    #[test]
    fn aura_mapping_never_coerces_unsupported_modes() {
        assert_eq!(aura_effect_index(AuraMode::Static), 0);
        assert_eq!(aura_effect_index(AuraMode::Breathe), 1);
        assert_eq!(aura_effect_index(AuraMode::RainbowCycle), 2);
        assert_eq!(aura_effect_index(AuraMode::Flash), -1);
        assert_eq!(aura_effect_index(AuraMode::RainbowWave), -1);
        assert_eq!(aura_effect_index(AuraMode::Unknown(99)), -1);
    }

    #[test]
    fn observed_keyboard_preserves_dynamic_hardware_max() {
        let state = keyboard_observed(Ok(KeyboardBacklightState {
            current: KeyboardBrightnessLevel::new(2),
            max: KeyboardBrightnessLevel::new(4),
        }));
        assert!(state.ready);
        assert_eq!(state.brightness, 2);
        assert!(state.status.contains("2/4"));
    }

    #[test]
    fn unknown_aura_speed_has_no_selected_ui_preset() {
        let state = aura_observed(Ok(AuraState {
            current_mode: AuraMode::Static,
            current_effect: AuraEffect {
                mode: AuraMode::Static,
                zone: AuraZone::None,
                colour1: AuraRgb { r: 1, g: 2, b: 3 },
                colour2: AuraRgb { r: 0, g: 0, b: 0 },
                speed: AuraSpeed::Unknown("Turbo".into()),
                direction: AuraDirection::Right,
            },
            brightness: AuraBrightness::Med,
            supported_modes: vec![AuraMode::Static],
            supported_zones: Vec::new(),
            supported_brightness: vec![AuraBrightness::Off, AuraBrightness::Med],
        }));
        assert!(state.ready);
        assert_eq!(state.effect, 0);
        assert_eq!(state.speed, -1);
    }

    #[test]
    fn bridge_contains_no_advanced_mutation_calls() {
        let source = include_str!("extra_backend.rs");
        for needle in ["set_panel", "set_aura", "set_led_mode", "set_keyboard_backlight"] {
            assert!(!source.contains(needle), "unexpected mutation token: {needle}");
        }
        assert!(source.contains("set_backend_ready(false)"));
        assert!(source.contains("set_keyboard_control_ready(false)"));
        assert!(source.contains("set_aura_control_ready(false)"));
        assert!(source.contains("set_panel_overdrive_control_ready(false)"));
    }
}
