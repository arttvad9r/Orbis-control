use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbis_core::aura::{AuraMode, AuraRgb, AuraSpeed};
use orbis_core::display::PanelOverdriveState;
use orbis_core::firmware::BootSoundState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{AuraProvider, PanelOverdriveProvider};
use orbis_providers::{AsusArmouryPanelOverdriveProvider, AsusAuraProvider, AsusBootSoundProvider};
use orbis_session_client::{SessionClamshellSource, ZbusSessionClamshellSource};
use orbis_session_protocol::clamshell;
use slint::ComponentHandle;

use crate::quick_controls_backend::hardware_controls_backend::{
    HardwareProductControlClient, ProductWriteStatus, require_supported,
};
use crate::{AppWindow, ClamshellState};

const READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct ExtraContext {
    runtime: tokio::runtime::Handle,
    refreshing: Arc<AtomicBool>,
    mutating: Arc<AtomicBool>,
    advanced_dirty: Arc<AtomicBool>,
    clamshell: Arc<dyn SessionClamshellSource>,
}

thread_local! {
    static CONTEXT: RefCell<Option<ExtraContext>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuraObserved {
    ready: bool,
    effect: i32,
    speed: i32,
    supported_modes: [bool; 13],
    colour1: AuraRgb,
    colour2: AuraRgb,
    status: String,
}

struct AuraEffectRequest {
    mode: i32,
    speed: i32,
    colour1: [i32; 3],
    colour2: [i32; 3],
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApuMemoryObserved {
    ready: bool,
    value: i32,
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdvancedApplyDraft {
    boot_sound: bool,
    boot_sound_ready: bool,
    igpu_memory: u8,
    igpu_memory_ready: bool,
    aspm_disabled: bool,
    aspm_ready: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AdvancedApplyObservation {
    igpu_memory: Option<(u8, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdvancedApplyControl {
    BootSound,
    IgpuMemory,
    Aspm,
}

#[derive(Debug)]
struct AdvancedApplyError {
    completed: Vec<AdvancedApplyControl>,
    failed: Option<AdvancedApplyControl>,
    error: ProviderError,
}

impl AdvancedApplyError {
    fn new(failed: AdvancedApplyControl, error: ProviderError) -> Self {
        Self {
            completed: Vec::new(),
            failed: Some(failed),
            error,
        }
    }
}

fn advanced_apply_ready(dirty: bool, boot_sound: bool, igpu_memory: bool, aspm: bool) -> bool {
    dirty && (boot_sound || igpu_memory || aspm)
}

fn advanced_apply_draft(
    boot_sound: bool,
    igpu_memory: i32,
    aspm_disabled: bool,
    boot_sound_ready: bool,
    igpu_memory_ready: bool,
    aspm_ready: bool,
) -> AdvancedApplyDraft {
    AdvancedApplyDraft {
        boot_sound,
        boot_sound_ready,
        igpu_memory: igpu_memory.clamp(0, 8) as u8,
        igpu_memory_ready,
        aspm_disabled,
        aspm_ready,
    }
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle, session_connection: zbus::Connection) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ExtraContext {
            runtime,
            refreshing: Arc::new(AtomicBool::new(false)),
            mutating: Arc::new(AtomicBool::new(false)),
            advanced_dirty: Arc::new(AtomicBool::new(false)),
            clamshell: Arc::new(ZbusSessionClamshellSource::new(session_connection)),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| {
        slot.borrow_mut().take();
    });
}

fn reset_readiness(window: &AppWindow) {
    window.set_backend_ready(false);
    window.set_applying(false);
    window.set_aura_state_ready(false);
    window.set_panel_overdrive_state_ready(false);
    window.set_panel_overdrive_control_ready(false);
    window.set_boot_sound_state_ready(false);
    window.set_boot_sound_control_ready(false);
    window.set_igpu_memory_state_ready(false);
    window.set_igpu_memory_control_ready(false);
    window.set_aspm_control_ready(false);
    window.set_standby_networking_control_ready(false);
    window.set_hibernate_control_ready(false);
    window.set_core_count_control_ready(false);
    window.set_binding_control_ready(false);
    window.set_advanced_apply_ready(false);
    window.set_igpu_memory_pending(false);
    window.set_status_led_state_ready(false);
    window.set_status_led_control_ready(false);
    window.set_aspm_state_ready(false);
    window.set_aspm_control_ready(false);
}

pub(crate) fn wire_window(window: &AppWindow) {
    reset_readiness(window);
    window.set_keyboard_effect(-1);
    window.set_keyboard_speed(-1);
    window.set_aura_static_supported(false);
    window.set_aura_breathe_supported(false);
    window.set_aura_rainbow_supported(false);
    window.set_aura_rainbow_wave_supported(false);
    window.set_aura_star_supported(false);
    window.set_aura_rain_supported(false);
    window.set_aura_highlight_supported(false);
    window.set_aura_laser_supported(false);
    window.set_aura_ripple_supported(false);
    window.set_aura_pulse_supported(false);
    window.set_aura_comet_supported(false);
    window.set_aura_flash_supported(false);
    window.set_boot_sound(false);
    window.set_igpu_memory(0);
    window.set_status(
        if CONTEXT.with(|slot| slot.borrow().is_some()) {
            "Reading advanced hardware observations…"
        } else {
            "Advanced controls backend unavailable"
        }
        .into(),
    );
    window.set_auto_clamshell_state(ClamshellState::Loading);

    {
        let weak = window.as_weak();
        window.on_aura_effect_requested(move |mode, speed, r1, g1, b1, r2, g2, b2| {
            if let Some(window) = weak.upgrade() {
                request_aura_effect(
                    &window,
                    AuraEffectRequest {
                        mode,
                        speed,
                        colour1: [r1, g1, b1],
                        colour2: [r2, g2, b2],
                    },
                );
            }
        });
    }

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
        window.on_igpu_memory_requested(move |value| {
            if let Some(window) = weak.upgrade() {
                stage_apu_memory(&window, value);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_boot_sound_requested(move |enabled| {
            if let Some(window) = weak.upgrade() {
                stage_boot_sound(&window, enabled);
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

    {
        let weak = window.as_weak();
        window.on_aspm_requested(move |disabled| {
            if let Some(window) = weak.upgrade() {
                stage_aspm(&window, disabled);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_auto_clamshell_requested(move |enabled| {
            if let Some(window) = weak.upgrade() {
                request_clamshell(&window, enabled);
            }
        });
    }

    let weak = window.as_weak();
    window.on_apply_requested(move || {
        if let Some(window) = weak.upgrade() {
            apply_advanced(&window);
        }
    });
}

fn stage_advanced(window: &AppWindow) {
    let Some(context) = CONTEXT.with(|slot| slot.borrow().clone()) else {
        return;
    };
    context.advanced_dirty.store(true, Ordering::Release);
    window.set_advanced_apply_ready(advanced_apply_ready(
        true,
        window.get_boot_sound_control_ready(),
        window.get_igpu_memory_control_ready(),
        window.get_aspm_control_ready(),
    ));
}

fn stage_boot_sound(window: &AppWindow, enabled: bool) {
    if !window.get_boot_sound_control_ready() {
        return;
    }
    window.set_boot_sound(enabled);
    stage_advanced(window);
}

fn stage_apu_memory(window: &AppWindow, value: i32) {
    if !window.get_igpu_memory_control_ready() || !(0..=8).contains(&value) {
        return;
    }
    window.set_igpu_memory(value);
    stage_advanced(window);
}

fn stage_aspm(window: &AppWindow, disabled: bool) {
    if !window.get_aspm_control_ready() {
        return;
    }
    window.set_disable_aspm(disabled);
    stage_advanced(window);
}

fn begin_mutation(window: &AppWindow) -> Option<ExtraContext> {
    let context = CONTEXT.with(|slot| slot.borrow().clone())?;
    if context.mutating.swap(true, Ordering::AcqRel) {
        return None;
    }
    window.set_applying(true);
    window.set_aura_control_ready(false);
    window.set_panel_overdrive_control_ready(false);
    window.set_boot_sound_control_ready(false);
    window.set_igpu_memory_control_ready(false);
    window.set_aspm_control_ready(false);
    Some(context)
}

fn apply_advanced(window: &AppWindow) {
    let Some(context) = CONTEXT.with(|slot| slot.borrow().clone()) else {
        return;
    };
    if !context.advanced_dirty.load(Ordering::Acquire) {
        window.set_status("Apply skipped · no staged changes".into());
        return;
    }
    let draft = advanced_apply_draft(
        window.get_boot_sound(),
        window.get_igpu_memory(),
        window.get_disable_aspm(),
        window.get_boot_sound_control_ready(),
        window.get_igpu_memory_control_ready(),
        window.get_aspm_control_ready(),
    );
    let Some(context) = begin_mutation(window) else {
        return;
    };
    window.set_advanced_apply_ready(false);
    window.set_status("Applying staged ASUS parameters through Hardware1…".into());
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    context.runtime.spawn(async move {
        let result = async {
            let client = HardwareProductControlClient::connect_system()
                .await
                .map_err(|error| AdvancedApplyError {
                    completed: Vec::new(),
                    failed: None,
                    error,
                })?;
            apply_advanced_client(&client, draft).await
        }
        .await;
        completion.store(false, Ordering::Release);
        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_applying(false);
            if let Some(context) = CONTEXT.with(|slot| slot.borrow().clone()) {
                context.advanced_dirty.store(false, Ordering::Release);
            }
            let (status, pending) = match result {
                Ok(observation) => (
                    if observation.igpu_memory.is_some_and(|(_, pending)| pending) {
                        "Staged ASUS parameters applied · reboot required".to_string()
                    } else {
                        "Staged ASUS parameters applied · read-back confirmed".to_string()
                    },
                    observation.igpu_memory.map(|(_, pending)| pending),
                ),
                Err(error) => (advanced_apply_error_status(&error), None),
            };
            refresh_with_status(&window, Some(status), pending);
        }) {
            tracing::warn!(error = ?error, "failed to publish staged ASUS Apply result");
        }
    });
}

async fn apply_advanced_client(
    client: &HardwareProductControlClient,
    draft: AdvancedApplyDraft,
) -> Result<AdvancedApplyObservation, AdvancedApplyError> {
    let mut observation = AdvancedApplyObservation::default();
    let mut completed = Vec::new();
    if draft.boot_sound_ready {
        let status = client
            .boot_sound_status()
            .await
            .map_err(|error| AdvancedApplyError::new(AdvancedApplyControl::BootSound, error))?;
        require_supported(status, "boot sound")
            .map_err(|error| AdvancedApplyError::new(AdvancedApplyControl::BootSound, error))?;
        client
            .set_boot_sound(draft.boot_sound)
            .await
            .map_err(|error| AdvancedApplyError {
                completed: completed.clone(),
                failed: Some(AdvancedApplyControl::BootSound),
                error,
            })?;
        completed.push(AdvancedApplyControl::BootSound);
    }
    if draft.igpu_memory_ready {
        let status = client
            .apu_memory_status()
            .await
            .map_err(|error| AdvancedApplyError {
                completed: completed.clone(),
                failed: Some(AdvancedApplyControl::IgpuMemory),
                error,
            })?;
        require_supported(status, "iGPU memory").map_err(|error| AdvancedApplyError {
            completed: completed.clone(),
            failed: Some(AdvancedApplyControl::IgpuMemory),
            error,
        })?;
        observation.igpu_memory = Some(client.set_apu_memory(draft.igpu_memory).await.map_err(
            |error| AdvancedApplyError {
                completed: completed.clone(),
                failed: Some(AdvancedApplyControl::IgpuMemory),
                error,
            },
        )?);
        completed.push(AdvancedApplyControl::IgpuMemory);
    }
    if draft.aspm_ready {
        let (_, status) = client
            .aspm_state()
            .await
            .map_err(|error| AdvancedApplyError {
                completed: completed.clone(),
                failed: Some(AdvancedApplyControl::Aspm),
                error,
            })?;
        require_supported(status, "ASPM").map_err(|error| AdvancedApplyError {
            completed: completed.clone(),
            failed: Some(AdvancedApplyControl::Aspm),
            error,
        })?;
        let observed = client
            .set_aspm_disabled(draft.aspm_disabled)
            .await
            .map_err(|error| AdvancedApplyError {
                completed: completed.clone(),
                failed: Some(AdvancedApplyControl::Aspm),
                error,
            })?;
        if observed != draft.aspm_disabled {
            return Err(AdvancedApplyError {
                completed,
                failed: Some(AdvancedApplyControl::Aspm),
                error: ProviderError::BackendUnavailable(
                    "ASPM read-back did not match staged value".into(),
                ),
            });
        }
        completed.push(AdvancedApplyControl::Aspm);
    }
    Ok(observation)
}

fn request_aura_effect(window: &AppWindow, request: AuraEffectRequest) {
    let AuraEffectRequest {
        mode,
        speed,
        colour1,
        colour2,
    } = request;
    if !window.get_aura_control_ready()
        || !(0..=12).contains(&mode)
        || !(0..=2).contains(&speed)
        || colour1
            .iter()
            .chain(colour2.iter())
            .any(|v| !(0..=255).contains(v))
    {
        tracing::warn!(
            mode,
            speed,
            "Aura effect request rejected by UI gate or range"
        );
        return;
    }
    let Some(context) = begin_mutation(window) else {
        return;
    };
    let speed = match speed {
        0 => "Low",
        1 => "Med",
        _ => "High",
    }
    .to_string();
    let colour1 = AuraRgb {
        r: colour1[0] as u8,
        g: colour1[1] as u8,
        b: colour1[2] as u8,
    };
    let colour2 = AuraRgb {
        r: colour2[0] as u8,
        g: colour2[1] as u8,
        b: colour2[2] as u8,
    };
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    context.runtime.spawn(async move {
        let result = async {
            let client = HardwareProductControlClient::connect_system().await?;
            client
                .set_aura_effect(mode as u32, speed, colour1, colour2)
                .await
        }
        .await;
        completion.store(false, Ordering::Release);
        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_applying(false);
            match result {
                Ok(observed) => {
                    window.set_keyboard_effect(observed.mode as i32);
                    window.set_keyboard_speed(match observed.speed.as_str() {
                        "Low" => 0,
                        "Med" => 1,
                        "High" => 2,
                        _ => -1,
                    });
                    window.set_rgb_red(observed.colour1.r as i32);
                    window.set_rgb_green(observed.colour1.g as i32);
                    window.set_rgb_blue(observed.colour1.b as i32);
                    window.set_aura_secondary_red(observed.colour2.r as i32);
                    window.set_aura_secondary_green(observed.colour2.g as i32);
                    window.set_aura_secondary_blue(observed.colour2.b as i32);
                    window.set_status("Aura effect accepted · config read-back confirmed".into());
                }
                Err(error) => window.set_status(format!("Aura effect failed · {error}").into()),
            }
            refresh(&window);
        }) {
            tracing::warn!(error = ?error, "failed to publish Aura effect mutation result");
        }
    });
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

fn request_clamshell(window: &AppWindow, enabled: bool) {
    if !matches!(
        window.get_auto_clamshell_state(),
        ClamshellState::Inactive | ClamshellState::Active
    ) {
        return;
    }
    let Some(context) = begin_mutation(window) else {
        return;
    };
    window.set_status("Updating closed-lid mode…".into());
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    let clamshell = context.clamshell.clone();
    context.runtime.spawn(async move {
        let result = clamshell.set_clamshell(enabled).await;
        completion.store(false, Ordering::Release);
        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_applying(false);
            let state = clamshell_ui_state(result);
            window.set_auto_clamshell_state(state);
            window.set_status(clamshell_status(state).into());
        }) {
            tracing::warn!(error = ?error, "failed to publish clamshell result");
        }
    });
}

fn clamshell_ui_state(result: Result<u8, ProviderError>) -> ClamshellState {
    match result {
        Ok(clamshell::INACTIVE) => ClamshellState::Inactive,
        Ok(clamshell::ACTIVE) => ClamshellState::Active,
        Ok(clamshell::UNAVAILABLE) => ClamshellState::Unavailable,
        Ok(clamshell::PERMISSION_DENIED) => ClamshellState::PermissionDenied,
        Ok(clamshell::START_FAILED) => ClamshellState::StartFailed,
        Ok(clamshell::EXIT_FAILED) => ClamshellState::ExitFailed,
        Ok(_) | Err(_) => ClamshellState::Unknown,
    }
}

fn clamshell_status(state: ClamshellState) -> &'static str {
    match state {
        ClamshellState::Loading => "Closed-lid mode is loading",
        ClamshellState::Inactive => "Closed-lid mode off · session inhibitor inactive",
        ClamshellState::Active => "Closed-lid mode on · session inhibitor active",
        ClamshellState::Unavailable => "Closed-lid mode unavailable",
        ClamshellState::PermissionDenied => "Closed-lid mode unavailable · permission denied",
        ClamshellState::StartFailed => "Closed-lid mode unavailable · inhibitor start failed",
        ClamshellState::ExitFailed => "Closed-lid mode unavailable · inhibitor exit failed",
        ClamshellState::Unknown => "Closed-lid mode unknown",
    }
}

pub(crate) fn refresh(window: &AppWindow) {
    refresh_with_status(window, None, None);
}

fn refresh_with_status(window: &AppWindow, final_status: Option<String>, pending: Option<bool>) {
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
    window.set_panel_overdrive_control_ready(false);
    window.set_auto_clamshell_state(ClamshellState::Loading);
    window.set_status("Refreshing advanced hardware state…".into());

    let weak = window.as_weak();
    let completion = context.refreshing.clone();
    context.runtime.spawn(async move {
        let panel_provider = AsusArmouryPanelOverdriveProvider::default();
        let boot_sound_provider = AsusBootSoundProvider::default();

        let (
            aura_result,
            panel_result,
            boot_sound_result,
            apu_result,
            aspm_result,
            write_statuses,
            clamshell_result,
        ) = tokio::join!(
            bounded_aura_read(),
            bounded_panel_read(&panel_provider),
            bounded_boot_sound_read(&boot_sound_provider),
            bounded_apu_memory_read(),
            bounded_aspm_read(),
            bounded_write_statuses(),
            context.clamshell.read_clamshell(),
        );

        let aura = aura_observed(aura_result);
        let panel = panel_observed(panel_result);
        let boot_sound = boot_sound_observed(boot_sound_result);
        let apu = apu_observed(apu_result);
        let (aspm_disabled, aspm_write) = match aspm_result {
            Ok(value) => value,
            Err(error) => {
                tracing::debug!(error = ?error, "ASPM observation unavailable");
                (false, ProductWriteStatus::Unknown)
            }
        };
        let (_keyboard_write, panel_write, boot_sound_write, apu_write) = match write_statuses {
            Ok(statuses) => statuses,
            Err(error) => {
                tracing::debug!(error = ?error, "Extra Hardware1 mutation-status read unavailable");
                (
                    ProductWriteStatus::Unknown,
                    ProductWriteStatus::Unknown,
                    ProductWriteStatus::Unknown,
                    ProductWriteStatus::Unknown,
                )
            }
        };
        completion.store(false, Ordering::Release);

        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_backend_ready(false);
            window.set_applying(false);

            let clamshell_state = clamshell_ui_state(clamshell_result);
            window.set_auto_clamshell_state(clamshell_state);

            if let Some(context) = CONTEXT.with(|slot| slot.borrow().clone()) {
                context.advanced_dirty.store(false, Ordering::Release);
            }
            window.set_advanced_apply_ready(false);

            window.set_aura_state_ready(aura.ready);
            window.set_keyboard_effect(aura.effect);
            window.set_keyboard_speed(aura.speed);
            window.set_aura_static_supported(aura.supported_modes[0]);
            window.set_aura_breathe_supported(aura.supported_modes[1]);
            window.set_aura_rainbow_supported(aura.supported_modes[2]);
            window.set_aura_rainbow_wave_supported(aura.supported_modes[3]);
            window.set_aura_star_supported(aura.supported_modes[4]);
            window.set_aura_rain_supported(aura.supported_modes[5]);
            window.set_aura_highlight_supported(aura.supported_modes[6]);
            window.set_aura_laser_supported(aura.supported_modes[7]);
            window.set_aura_ripple_supported(aura.supported_modes[8]);
            window.set_aura_pulse_supported(aura.supported_modes[10]);
            window.set_aura_comet_supported(aura.supported_modes[11]);
            window.set_aura_flash_supported(aura.supported_modes[12]);
            window.set_rgb_red(aura.colour1.r as i32);
            window.set_rgb_green(aura.colour1.g as i32);
            window.set_rgb_blue(aura.colour1.b as i32);
            window.set_aura_secondary_red(aura.colour2.r as i32);
            window.set_aura_secondary_green(aura.colour2.g as i32);
            window.set_aura_secondary_blue(aura.colour2.b as i32);

            window.set_panel_overdrive_state_ready(panel.ready);
            window.set_panel_overdrive_control_ready(panel.ready && panel_write.is_supported());
            window.set_panel_overdrive(panel.enabled);

            window.set_boot_sound_state_ready(boot_sound.ready);
            window
                .set_boot_sound_control_ready(boot_sound.ready && boot_sound_write.is_supported());
            window.set_boot_sound(boot_sound.enabled);
            window.set_igpu_memory_state_ready(apu.ready);
            window.set_igpu_memory_control_ready(apu.ready && apu_write.is_supported());
            window.set_igpu_memory(apu.value);
            window.set_igpu_memory_pending(pending.unwrap_or(false));
            window.set_aspm_state_ready(aspm_write != ProductWriteStatus::Unknown);
            window.set_aspm_control_ready(aspm_write.is_supported());
            window.set_disable_aspm(aspm_disabled);

            let any_ready = aura.ready
                || panel.ready
                || boot_sound.ready
                || apu.ready
                || aspm_write != ProductWriteStatus::Unknown;
            window.set_backend_ready(any_ready);
            let observed_status = if any_ready {
                format!(
                    "Observed · {} · panel {} · {} · iGPU {}",
                    aura.status,
                    panel_write.short_label(),
                    boot_sound.status,
                    apu.status,
                )
            } else {
                format!(
                    "Advanced observations unavailable · {} · {} · iGPU {}",
                    aura.status, boot_sound.status, apu.status,
                )
            };
            window.set_status(
                final_status
                    .unwrap_or_else(|| {
                        format!("{observed_status} · {}", clamshell_status(clamshell_state))
                    })
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

async fn bounded_apu_memory_read() -> Result<u8, ProviderError> {
    let client = HardwareProductControlClient::connect_system().await?;
    tokio::time::timeout(READ_TIMEOUT, client.apu_memory_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra iGPU memory read timed out".into()))?
}

async fn bounded_aspm_read() -> Result<(bool, ProductWriteStatus), ProviderError> {
    let client = HardwareProductControlClient::connect_system().await?;
    tokio::time::timeout(READ_TIMEOUT, client.aspm_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra ASPM read timed out".into()))?
}

async fn bounded_write_statuses() -> Result<
    (
        ProductWriteStatus,
        ProductWriteStatus,
        ProductWriteStatus,
        ProductWriteStatus,
    ),
    ProviderError,
> {
    let client = HardwareProductControlClient::connect_system().await?;
    let (keyboard, panel, boot_sound, apu) = tokio::join!(
        client.keyboard_status(),
        client.panel_status(),
        client.boot_sound_status(),
        client.apu_memory_status(),
    );
    Ok((keyboard?, panel?, boot_sound?, apu?))
}

fn aura_observed(result: Result<orbis_core::aura::AuraState, ProviderError>) -> AuraObserved {
    match result {
        Ok(state) => AuraObserved {
            ready: true,
            effect: aura_effect_index(state.current_mode),
            speed: aura_speed_index(&state.current_effect.speed),
            supported_modes: [
                state.supported_modes.contains(&AuraMode::Static),
                state.supported_modes.contains(&AuraMode::Breathe),
                state.supported_modes.contains(&AuraMode::RainbowCycle),
                state.supported_modes.contains(&AuraMode::RainbowWave),
                state.supported_modes.contains(&AuraMode::Star),
                state.supported_modes.contains(&AuraMode::Rain),
                state.supported_modes.contains(&AuraMode::Highlight),
                state.supported_modes.contains(&AuraMode::Laser),
                state.supported_modes.contains(&AuraMode::Ripple),
                false,
                state.supported_modes.contains(&AuraMode::Pulse),
                state.supported_modes.contains(&AuraMode::Comet),
                state.supported_modes.contains(&AuraMode::Flash),
            ],
            colour1: state.current_effect.colour1,
            colour2: state.current_effect.colour2,
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
            supported_modes: [false; 13],
            colour1: AuraRgb { r: 0, g: 0, b: 0 },
            colour2: AuraRgb { r: 0, g: 0, b: 0 },
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

fn apu_observed(result: Result<u8, ProviderError>) -> ApuMemoryObserved {
    match result {
        Ok(value) if value <= 8 => ApuMemoryObserved {
            ready: true,
            value: value as i32,
            status: format!("{value} selected"),
        },
        Ok(value) => ApuMemoryObserved {
            ready: false,
            value: 0,
            status: format!("invalid value {value}"),
        },
        Err(error) => ApuMemoryObserved {
            ready: false,
            value: 0,
            status: error_status("iGPU memory", &error),
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

fn advanced_apply_error_status(error: &AdvancedApplyError) -> String {
    let reason = write_error_label(&error.error);
    let Some(failed) = error.failed.map(advanced_apply_control_label) else {
        return format!("Apply failed · {reason} · hardware state refreshed");
    };
    if matches!(error.error, ProviderError::Timeout(_)) {
        return format!(
            "Apply outcome unknown · {failed} may have changed · {reason} · hardware state refreshed"
        );
    }
    if error.completed.is_empty() {
        return format!("Apply failed · {failed} {reason} · hardware state refreshed");
    }
    let completed = error
        .completed
        .iter()
        .map(|control| format!("{} applied", advanced_apply_control_label(*control)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Apply partially completed · {completed}; {failed} failed · {reason} · hardware state refreshed"
    )
}

fn advanced_apply_control_label(control: AdvancedApplyControl) -> &'static str {
    match control {
        AdvancedApplyControl::BootSound => "boot sound",
        AdvancedApplyControl::IgpuMemory => "iGPU memory",
        AdvancedApplyControl::Aspm => "ASPM",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::ClamshellState;
    use orbis_core::aura::{
        AuraBrightness, AuraDirection, AuraEffect, AuraRgb, AuraState, AuraZone,
    };

    const OBJECT_PATH: &str = "/io/github/orbiscontrol/Hardware";

    #[derive(Clone)]
    struct FakeAdvancedHardware {
        state: Arc<Mutex<(bool, u8, bool)>>,
        boot_status: u8,
        fail_on: Option<AdvancedApplyControl>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl FakeAdvancedHardware {
        async fn boot_sound_mutation_status(&self) -> u8 {
            self.boot_status
        }

        async fn set_boot_sound(&self, enabled: bool) -> zbus::fdo::Result<u8> {
            if self.fail_on == Some(AdvancedApplyControl::BootSound) {
                return Err(zbus::fdo::Error::Failed("boot sound failure".into()));
            }
            self.state.lock().unwrap().0 = enabled;
            Ok(enabled as u8)
        }

        async fn apu_memory_mutation_status(&self) -> u8 {
            0
        }

        async fn set_apu_memory(&self, value: u8) -> zbus::fdo::Result<(u8, u8, u8)> {
            if self.fail_on == Some(AdvancedApplyControl::IgpuMemory) {
                return Err(zbus::fdo::Error::Failed("iGPU memory failure".into()));
            }
            self.state.lock().unwrap().1 = value;
            Ok((value, value, 1))
        }

        async fn aspm_mutation_status(&self) -> u8 {
            0
        }

        async fn aspm_disabled(&self) -> bool {
            self.state.lock().unwrap().2
        }

        async fn set_aspm_disabled(&self, disabled: bool) -> zbus::fdo::Result<bool> {
            if self.fail_on == Some(AdvancedApplyControl::Aspm) {
                return Err(zbus::fdo::Error::Failed("ASPM failure".into()));
            }
            self.state.lock().unwrap().2 = disabled;
            Ok(disabled)
        }
    }

    async fn private_advanced_peer(
        hardware: FakeAdvancedHardware,
    ) -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at(OBJECT_PATH, hardware)
            .unwrap();
        let client = zbus::connection::Builder::unix_stream(client_stream).p2p();
        tokio::try_join!(server.build(), client.build()).unwrap()
    }

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
    fn advanced_apply_requires_staged_change_and_typed_write_evidence() {
        assert!(!advanced_apply_ready(false, true, true, true));
        assert!(advanced_apply_ready(true, true, false, false));
        assert!(advanced_apply_ready(true, false, false, true));
        assert!(!advanced_apply_ready(true, false, false, false));
    }

    #[test]
    fn advanced_apply_captures_all_ready_controls_before_mutation_gate() {
        let draft = advanced_apply_draft(true, 6, true, true, true, true);

        assert_eq!(
            draft,
            AdvancedApplyDraft {
                boot_sound: true,
                boot_sound_ready: true,
                igpu_memory: 6,
                igpu_memory_ready: true,
                aspm_disabled: true,
                aspm_ready: true,
            }
        );
    }

    #[tokio::test]
    async fn staged_apply_uses_only_supported_typed_controls() {
        let hardware = FakeAdvancedHardware {
            state: Arc::new(Mutex::new((false, 2, false))),
            boot_status: 0,
            fail_on: None,
        };
        let state = hardware.state.clone();
        let (_server, connection) = private_advanced_peer(hardware).await;
        let client = HardwareProductControlClient::new(connection);

        apply_advanced_client(
            &client,
            AdvancedApplyDraft {
                boot_sound: true,
                boot_sound_ready: true,
                igpu_memory: 6,
                igpu_memory_ready: true,
                aspm_disabled: true,
                aspm_ready: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(*state.lock().unwrap(), (true, 6, true));
    }

    #[tokio::test]
    async fn staged_apply_reports_controls_that_lost_write_support() {
        let hardware = FakeAdvancedHardware {
            state: Arc::new(Mutex::new((false, 2, false))),
            boot_status: 1,
            fail_on: None,
        };
        let (_server, connection) = private_advanced_peer(hardware).await;
        let client = HardwareProductControlClient::new(connection);

        let result = apply_advanced_client(
            &client,
            AdvancedApplyDraft {
                boot_sound: true,
                boot_sound_ready: true,
                igpu_memory: 2,
                igpu_memory_ready: false,
                aspm_disabled: false,
                aspm_ready: false,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "lost write support must not be reported as success"
        );
        assert!(matches!(
            result.unwrap_err().failed,
            Some(AdvancedApplyControl::BootSound)
        ));
    }

    #[tokio::test]
    async fn staged_apply_later_operation_failure_reports_prior_success() {
        let hardware = FakeAdvancedHardware {
            state: Arc::new(Mutex::new((false, 2, false))),
            boot_status: 0,
            fail_on: Some(AdvancedApplyControl::IgpuMemory),
        };
        let state = hardware.state.clone();
        let (_server, connection) = private_advanced_peer(hardware).await;
        let client = HardwareProductControlClient::new(connection);

        let error = apply_advanced_client(
            &client,
            AdvancedApplyDraft {
                boot_sound: true,
                boot_sound_ready: true,
                igpu_memory: 6,
                igpu_memory_ready: true,
                aspm_disabled: false,
                aspm_ready: false,
            },
        )
        .await
        .unwrap_err();

        assert_eq!(*state.lock().unwrap(), (true, 2, false));
        assert_eq!(
            advanced_apply_error_status(&error),
            "Apply partially completed · boot sound applied; iGPU memory failed · write failed · hardware state refreshed"
        );
    }

    #[test]
    fn staged_apply_first_failure_is_not_presented_as_partial_success() {
        let error = AdvancedApplyError::new(
            AdvancedApplyControl::BootSound,
            ProviderError::Unsupported("boot sound".into()),
        );

        assert_eq!(
            advanced_apply_error_status(&error),
            "Apply failed · boot sound write disabled · hardware state refreshed"
        );
    }

    #[test]
    fn staged_apply_later_failure_preserves_completed_control_and_no_rollback_claim() {
        let error = AdvancedApplyError {
            completed: vec![AdvancedApplyControl::BootSound],
            failed: Some(AdvancedApplyControl::IgpuMemory),
            error: ProviderError::BackendUnavailable("lost connection".into()),
        };

        assert_eq!(
            advanced_apply_error_status(&error),
            "Apply partially completed · boot sound applied; iGPU memory failed · write unavailable · hardware state refreshed"
        );
        assert!(!advanced_apply_error_status(&error).contains("rolled back"));
    }

    #[test]
    fn staged_apply_timeout_is_reported_as_unknown_for_failed_control() {
        let error = AdvancedApplyError::new(
            AdvancedApplyControl::Aspm,
            ProviderError::Timeout("ASPM timed out".into()),
        );

        assert_eq!(
            advanced_apply_error_status(&error),
            "Apply outcome unknown · ASPM may have changed · write timed out · hardware state refreshed"
        );
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
    fn boot_sound_uses_typed_hardware1_and_unrelated_draft_stays_disabled() {
        let source = include_str!("extra_backend.rs");
        assert!(source.contains("AsusBootSoundProvider"));
        assert!(source.contains("set_boot_sound_state_ready"));
        assert!(source.contains("set_backend_ready(false)"));
        assert!(source.contains("set_aura_control_ready(false)"));
        assert!(!source.contains(&["set_aura", "_static_rgb"].concat()));
        assert!(source.contains("set_boot_sound(enabled)"));
        assert!(source.contains("boot_sound_status()"));
        assert!(!source.contains(&["set_gpu", "_mode"].concat()));
        assert!(!source.contains(&["set_fan", "_curve"].concat()));
    }

    #[test]
    fn unsupported_advanced_controls_are_explicitly_unavailable() {
        let source = include_str!("../../../ui/audited/sections/system.slint");
        assert!(source.contains("Недоступно: поддержка функции не обнаружена"));
        assert!(source.contains("disabled: true; // no typed owner yet"));
        assert!(source.contains("status-led-control-ready"));
        assert!(source.contains("standby-networking-control-ready"));
        assert!(source.contains("hibernate-control-ready"));
        assert!(source.contains("core-count-control-ready"));
        assert!(source.contains("binding-control-ready"));
        assert!(source.contains("advanced-apply-ready"));
        assert!(
            source.contains(
                "Параметры станут редактируемыми после подтверждения поддержки устройства."
            )
        );
    }

    #[test]
    fn clamshell_wire_values_map_to_explicit_ui_states() {
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::UNAVAILABLE)),
            ClamshellState::Unavailable
        );
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::INACTIVE)),
            ClamshellState::Inactive
        );
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::ACTIVE)),
            ClamshellState::Active
        );
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::PERMISSION_DENIED)),
            ClamshellState::PermissionDenied
        );
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::START_FAILED)),
            ClamshellState::StartFailed
        );
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::EXIT_FAILED)),
            ClamshellState::ExitFailed
        );
        assert_eq!(
            clamshell_ui_state(Ok(clamshell::UNKNOWN)),
            ClamshellState::Unknown
        );
        assert_eq!(clamshell_ui_state(Ok(255)), ClamshellState::Unknown);
    }

    #[test]
    fn clamshell_provider_errors_are_unknown_and_fail_closed() {
        assert_eq!(
            clamshell_ui_state(Err(ProviderError::Internal("permission denied".into()))),
            ClamshellState::Unknown
        );
        let source = include_str!("../../../ui/audited/sections/system.slint");
        assert!(source.contains("ClamshellState"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.Loading"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.Inactive"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.Active"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.Unavailable"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.PermissionDenied"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.StartFailed"));
        assert!(source.contains("auto-clamshell-state == ClamshellState.ExitFailed"));
        assert!(source.contains("disabled: (root.auto-clamshell-state != ClamshellState.Inactive"));
    }

    #[test]
    fn clamshell_ui_status_covers_every_explicit_state() {
        for (state, marker) in [
            (ClamshellState::Loading, "loading"),
            (ClamshellState::Inactive, "off"),
            (ClamshellState::Active, "on"),
            (ClamshellState::Unavailable, "unavailable"),
            (ClamshellState::PermissionDenied, "permission denied"),
            (ClamshellState::StartFailed, "start failed"),
            (ClamshellState::ExitFailed, "exit failed"),
            (ClamshellState::Unknown, "unknown"),
        ] {
            assert!(clamshell_status(state).contains(marker), "{state:?}");
        }
    }
}
