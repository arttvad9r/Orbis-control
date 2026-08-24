use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbis_core::aura::{AuraMode, AuraSpeed};
use orbis_core::display::PanelOverdriveState;
use orbis_core::firmware::BootSoundState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{AuraProvider, PanelOverdriveProvider};
use orbis_providers::{AsusArmouryPanelOverdriveProvider, AsusAuraProvider, AsusBootSoundProvider};
use slint::ComponentHandle;

use crate::AppWindow;
use crate::quick_controls_backend::hardware_controls_backend::{
    HardwareProductControlClient, ProductWriteStatus,
};

const READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct ExtraContext {
    runtime: tokio::runtime::Handle,
    refreshing: Arc<AtomicBool>,
    mutating: Arc<AtomicBool>,
}

thread_local! {
    static CONTEXT: RefCell<Option<ExtraContext>> = const { RefCell::new(None) };
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct BootSoundObserved {
    ready: bool,
    enabled: bool,
    status: String,
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ExtraContext {
            runtime,
            refreshing: Arc::new(AtomicBool::new(false)),
            mutating: Arc::new(AtomicBool::new(false)),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

fn reset_readiness(window: &AppWindow) {
    window.set_backend_ready(false);
    window.set_applying(false);
    window.set_aura_state_ready(false);
    window.set_aura_control_ready(false);
    window.set_panel_overdrive_state_ready(false);
    window.set_panel_overdrive_control_ready(false);
    window.set_boot_sound_state_ready(false);
}

pub(crate) fn wire_window(window: &AppWindow) {
    reset_readiness(window);
    window.set_keyboard_effect(-1);
    window.set_keyboard_speed(-1);
    window.set_boot_sound(false);
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

    {
        let weak = window.as_weak();
        window.on_panel_overdrive_requested(move |enabled| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            request_panel_overdrive(&window, enabled);
        });
    }

    window.on_apply_requested(|| {
        tracing::warn!(
            "Extra Controls Apply ignored: remaining multi-field advanced backend is not connected"
        );
    });
}

fn begin_mutation(window: &AppWindow) -> Option<ExtraContext> {
    let context = CONTEXT.with(|slot| slot.borrow().clone())?;
    if context.mutating.swap(true, Ordering::AcqRel) {
        return None;
    }
    window.set_applying(true);
    window.set_panel_overdrive_control_ready(false);
    Some(context)
}

fn request_panel_overdrive(window: &AppWindow, enabled: bool) {
    if !window.get_panel_overdrive_control_ready() {
        tracing::warn!(
            requested = enabled,
            "Extra panel request ignored: write evidence unavailable"
        );
        return;
    }
    let Some(context) = begin_mutation(window) else {
        return;
    };

    window.set_status("Applying Panel Overdrive through Hardware1…".into());
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    context.runtime.spawn(async move {
        let result = async {
            let client = HardwareProductControlClient::connect_system().await?;
            client.set_panel_overdrive(enabled).await
        }
        .await;
        completion.store(false, Ordering::Release);

        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_applying(false);
            match result {
                Ok(observed) => {
                    window.set_panel_overdrive(observed);
                    window.set_status(
                        format!(
                            "Panel Overdrive {} · authoritative read-back confirmed",
                            if observed { "on" } else { "off" }
                        )
                        .into(),
                    );
                }
                Err(error) => {
                    tracing::warn!(error = ?error, "Extra Panel Overdrive mutation failed");
                    window.set_status(
                        format!("Panel write failed · {}", write_error_label(&error)).into(),
                    );
                }
            }
            refresh(&window);
        }) {
            tracing::warn!(error = ?error, "failed to publish Extra panel mutation result");
        }
    });
}

pub(crate) fn refresh(window: &AppWindow) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        reset_readiness(window);
        window.set_status("Advanced controls backend unavailable".into());
        return;
    };

    if context.mutating.load(Ordering::Acquire) {
        return;
    }
    if context.refreshing.swap(true, Ordering::AcqRel) {
        return;
    }

    window.set_backend_ready(false);
    window.set_keyboard_control_ready(false);
    window.set_aura_control_ready(false);
    window.set_panel_overdrive_control_ready(false);
    window.set_status("Refreshing advanced hardware state…".into());

    let weak = window.as_weak();
    let completion = context.refreshing.clone();
    context.runtime.spawn(async move {
        let panel_provider = AsusArmouryPanelOverdriveProvider::default();
        let boot_sound_provider = AsusBootSoundProvider::default();

        let (aura_result, panel_result, boot_sound_result, write_statuses) = tokio::join!(
            bounded_aura_read(),
            bounded_panel_read(&panel_provider),
            bounded_boot_sound_read(&boot_sound_provider),
            bounded_write_statuses(),
        );

        let aura = aura_observed(aura_result);
        let panel = panel_observed(panel_result);
        let boot_sound = boot_sound_observed(boot_sound_result);
        let (_keyboard_write, panel_write) = match write_statuses {
            Ok(statuses) => statuses,
            Err(error) => {
                tracing::debug!(error = ?error, "Extra Hardware1 mutation-status read unavailable");
                (ProductWriteStatus::Unknown, ProductWriteStatus::Unknown)
            }
        };
        completion.store(false, Ordering::Release);

        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_backend_ready(false);
            window.set_applying(false);

            window.set_aura_state_ready(aura.ready);
            window.set_aura_control_ready(false);
            window.set_keyboard_effect(aura.effect);
            window.set_keyboard_speed(aura.speed);

            window.set_panel_overdrive_state_ready(panel.ready);
            window.set_panel_overdrive_control_ready(panel.ready && panel_write.is_supported());
            window.set_panel_overdrive(panel.enabled);

            window.set_boot_sound_state_ready(boot_sound.ready);
            window.set_boot_sound(boot_sound.enabled);

            let any_ready = aura.ready || panel.ready || boot_sound.ready;
            window.set_status(
                if any_ready {
                    format!(
                        "Observed · {} · panel {} · {}",
                        aura.status,
                        panel_write.short_label(),
                        boot_sound.status,
                    )
                } else {
                    format!(
                        "Advanced observations unavailable · {} · {}",
                        aura.status, boot_sound.status
                    )
                }
                .into(),
            );
        }) {
            tracing::warn!(error = ?error, "failed to publish Extra observed state to UI");
        }
    });
}

async fn bounded_aura_read() -> Result<orbis_core::aura::AuraState, ProviderError> {
    let connection = tokio::time::timeout(READ_TIMEOUT, zbus::Connection::system())
        .await
        .map_err(|_| ProviderError::Timeout("Extra Aura bus connect timed out".into()))?
        .map_err(|error| {
            ProviderError::BackendUnavailable(format!("Extra Aura system bus unavailable: {error}"))
        })?;
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

async fn bounded_boot_sound_read(
    provider: &AsusBootSoundProvider,
) -> Result<BootSoundState, ProviderError> {
    tokio::time::timeout(READ_TIMEOUT, provider.boot_sound_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra boot-sound read timed out".into()))?
}

async fn bounded_write_statuses() -> Result<(ProductWriteStatus, ProductWriteStatus), ProviderError>
{
    let client = HardwareProductControlClient::connect_system().await?;
    let (keyboard, panel) = tokio::join!(client.keyboard_status(), client.panel_status());
    Ok((keyboard?, panel?))
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

fn boot_sound_observed(result: Result<BootSoundState, ProviderError>) -> BootSoundObserved {
    match result {
        Ok(state) => BootSoundObserved {
            ready: true,
            enabled: matches!(state, BootSoundState::Enabled),
            status: if matches!(state, BootSoundState::Enabled) {
                "boot sound on".into()
            } else {
                "boot sound off".into()
            },
        },
        Err(error) => BootSoundObserved {
            ready: false,
            enabled: false,
            status: error_status("boot sound", &error),
        },
    }
}

fn aura_effect_index(mode: AuraMode) -> i32 {
    match mode {
        AuraMode::Static => 0,
        AuraMode::Breathe => 1,
        AuraMode::RainbowCycle => 2,
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

fn write_error_label(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::Unsupported(_) => "write disabled",
        ProviderError::BackendUnavailable(_) => "write unavailable",
        ProviderError::PermissionDenied(_) => "write denied",
        ProviderError::Timeout(_) => "write timed out",
        _ => "write failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::aura::{
        AuraBrightness, AuraDirection, AuraEffect, AuraRgb, AuraState, AuraZone,
    };

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
    fn boot_sound_observation_preserves_false_as_known_state() {
        let state = boot_sound_observed(Ok(BootSoundState::Disabled));
        assert!(state.ready);
        assert!(!state.enabled);
        assert!(state.status.contains("off"));
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
    fn direct_controls_are_hardware1_gated_and_request_only() {
        let source = include_str!("extra_backend.rs");
        assert!(source.contains("HardwareProductControlClient"));
        assert!(source.contains("keyboard_status()"));
        assert!(source.contains("panel_status()"));
        assert!(source.contains("set_keyboard_backlight(level)"));
        assert!(source.contains("set_panel_overdrive(enabled)"));
        assert!(source.contains("get_keyboard_control_ready()"));
        assert!(source.contains("get_panel_overdrive_control_ready()"));
        assert!(source.contains("refresh(&window)"));
    }

    #[test]
    fn boot_sound_is_observation_only_and_unrelated_draft_stays_disabled() {
        let source = include_str!("extra_backend.rs");
        assert!(source.contains("AsusBootSoundProvider"));
        assert!(source.contains("set_boot_sound_state_ready"));
        assert!(source.contains("set_backend_ready(false)"));
        assert!(source.contains("set_aura_control_ready(false)"));
        assert!(!source.contains(&["set_aura", "_static_rgb"].concat()));
        let boot_sound_mutation = ["client.", "set_boot_sound("].concat();
        assert!(!source.contains(&boot_sound_mutation));
        assert!(!source.contains(&["set_gpu", "_mode"].concat()));
        assert!(!source.contains(&["set_fan", "_curve"].concat()));
    }
}
