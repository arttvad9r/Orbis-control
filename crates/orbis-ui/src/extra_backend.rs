use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbis_core::aura::{AuraMode, AuraPowerState, AuraRgb, AuraSpeed};
use orbis_core::display::PanelOverdriveState;
use orbis_core::firmware::BootSoundState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{AuraProvider, PanelOverdriveProvider};
use orbis_providers::{AsusArmouryPanelOverdriveProvider, AsusAuraProvider, AsusBootSoundProvider};
use orbis_session_client::{SessionClamshellSource, ZbusSessionClamshellSource};
use orbis_session_protocol::clamshell;
use slint::ComponentHandle;

use crate::quick_controls_backend::hardware_controls_backend::{
    AuraEffectSnapshot, HardwareProductControlClient, ProductWriteStatus, kernel_effect_overlay,
    require_supported,
};
use crate::{AppWindow, ClamshellState};

const READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct ExtraContext {
    runtime: tokio::runtime::Handle,
    refreshing: Arc<AtomicBool>,
    mutating: Arc<AtomicBool>,
    advanced_dirty: Arc<AtomicBool>,
    advanced_unresolved: Arc<AtomicBool>,
    advanced_draft: Arc<Mutex<Option<AdvancedApplyDraft>>>,
    clamshell: Arc<dyn SessionClamshellSource>,
    refresh_reads: Arc<dyn Fn() -> RefreshReadsFuture + Send + Sync>,
    write_statuses: Arc<dyn Fn() -> WriteStatusFuture + Send + Sync>,
}

type RefreshReads = (
    Result<orbis_core::aura::AuraState, ProviderError>,
    Result<PanelOverdriveState, ProviderError>,
    Result<BootSoundState, ProviderError>,
    Result<u8, ProviderError>,
    Result<(bool, ProductWriteStatus), ProviderError>,
);
type RefreshReadsFuture = std::pin::Pin<Box<dyn std::future::Future<Output = RefreshReads> + Send>>;

type WriteStatuses = Result<
    (
        ProductWriteStatus,
        ProductWriteStatus,
        ProductWriteStatus,
        ProductWriteStatus,
    ),
    ProviderError,
>;
type WriteStatusFuture = std::pin::Pin<Box<dyn std::future::Future<Output = WriteStatuses> + Send>>;

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
    /// Keyboard zone of `LedPower`, when asusd reports it.
    power: Option<AuraPowerState>,
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

fn advanced_apply_requires_reconciliation(error: &AdvancedApplyError) -> bool {
    matches!(error.error, ProviderError::Timeout(_))
}

fn advanced_control_writable(unresolved: bool, ready: bool, supported: bool) -> bool {
    !unresolved && ready && supported
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdvancedReadiness {
    boot_sound: bool,
    igpu_memory: bool,
    aspm: bool,
    apply: bool,
}

fn advanced_readiness(
    unresolved: bool,
    dirty: bool,
    boot_observed: bool,
    boot_write: ProductWriteStatus,
    apu_observed: bool,
    apu_write: ProductWriteStatus,
    aspm_write: ProductWriteStatus,
) -> AdvancedReadiness {
    let boot_sound =
        advanced_control_writable(unresolved, boot_observed, boot_write.is_supported());
    let igpu_memory = advanced_control_writable(unresolved, apu_observed, apu_write.is_supported());
    let aspm = advanced_control_writable(
        unresolved,
        aspm_write != ProductWriteStatus::Unknown,
        aspm_write.is_supported(),
    );
    AdvancedReadiness {
        boot_sound,
        igpu_memory,
        aspm,
        apply: advanced_apply_ready(dirty, boot_sound, igpu_memory, aspm),
    }
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

fn advanced_refresh_reconciliation(
    unresolved: bool,
    dirty: bool,
    draft: Option<AdvancedApplyDraft>,
    boot_sound: Option<bool>,
    igpu_memory: Option<u8>,
    aspm_disabled: Option<bool>,
) -> (bool, bool) {
    if !unresolved {
        return (false, false);
    }
    let Some(draft) = draft else {
        return (true, dirty);
    };
    let reconciled = (!draft.boot_sound_ready || boot_sound == Some(draft.boot_sound))
        && (!draft.igpu_memory_ready || igpu_memory == Some(draft.igpu_memory))
        && (!draft.aspm_ready || aspm_disabled == Some(draft.aspm_disabled));
    let observations_complete = (!draft.boot_sound_ready || boot_sound.is_some())
        && (!draft.igpu_memory_ready || igpu_memory.is_some())
        && (!draft.aspm_ready || aspm_disabled.is_some());
    if observations_complete {
        (false, if reconciled { false } else { dirty })
    } else {
        (true, dirty)
    }
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle, session_connection: zbus::Connection) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ExtraContext {
            runtime,
            refreshing: Arc::new(AtomicBool::new(false)),
            mutating: Arc::new(AtomicBool::new(false)),
            advanced_dirty: Arc::new(AtomicBool::new(false)),
            advanced_unresolved: Arc::new(AtomicBool::new(false)),
            advanced_draft: Arc::new(Mutex::new(None)),
            clamshell: Arc::new(ZbusSessionClamshellSource::new(session_connection)),
            refresh_reads: Arc::new(|| Box::pin(bounded_refresh_reads())),
            write_statuses: Arc::new(|| Box::pin(bounded_write_statuses())),
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
    window.set_advanced_apply_ready(false);
    window.set_igpu_memory_pending(false);
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
    window.set_aura_power_ready(false);
    window.set_boot_sound(false);
    window.set_igpu_memory(0);
    window.set_status(
        if CONTEXT.with(|slot| slot.borrow().is_some()) {
            "Чтение состояния…"
        } else {
            "Служба расширенных параметров недоступна"
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
        window.on_aura_power_requested(move |which, on| {
            if let Some(window) = weak.upgrade() {
                request_aura_power(&window, which, on);
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
        window.set_status("Нет изменений для применения".into());
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
    window.set_status("Применение параметров…".into());
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    let unresolved_state = context.advanced_unresolved.clone();
    let pending_draft = context.advanced_draft.clone();
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
            let unresolved = result
                .as_ref()
                .err()
                .is_some_and(advanced_apply_requires_reconciliation);
            unresolved_state.store(unresolved, Ordering::Release);
            if unresolved {
                *pending_draft.lock().unwrap() = Some(draft);
            } else {
                *pending_draft.lock().unwrap() = None;
                if let Some(context) = CONTEXT.with(|slot| slot.borrow().clone()) {
                    context.advanced_dirty.store(
                        result
                            .as_ref()
                            .is_err_and(|error| !error.completed.is_empty()),
                        Ordering::Release,
                    );
                }
            }
            let (status, pending) = match result {
                Ok(observation) => (
                    if observation.igpu_memory.is_some_and(|(_, pending)| pending) {
                        "Параметры применены · память iGPU изменится после перезагрузки".to_string()
                    } else {
                        "Параметры применены и подтверждены".to_string()
                    },
                    observation.igpu_memory.map(|(_, pending)| pending),
                ),
                Err(error) => (advanced_apply_error_status(&error), None),
            };
            refresh_with_status(&window, Some(status), pending, !unresolved);
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

fn apply_power_state(window: &AppWindow, power: Option<AuraPowerState>) {
    window.set_aura_power_ready(power.is_some());
    if let Some(power) = power {
        window.set_aura_power_boot(power.boot);
        window.set_aura_power_awake(power.awake);
        window.set_aura_power_sleep(power.sleep);
        window.set_aura_power_shutdown(power.shutdown);
    }
}

/// Toggle one power state (0 boot, 1 awake, 2 sleep, 3 shutdown) of the
/// keyboard zone, keeping the other three as currently shown.
fn with_power_toggle(current: AuraPowerState, which: i32, on: bool) -> Option<AuraPowerState> {
    let mut next = current;
    match which {
        0 => next.boot = on,
        1 => next.awake = on,
        2 => next.sleep = on,
        3 => next.shutdown = on,
        _ => return None,
    }
    Some(next)
}

fn request_aura_power(window: &AppWindow, which: i32, on: bool) {
    if !window.get_aura_power_ready() || !window.get_aura_control_ready() {
        tracing::warn!(which, "Aura power request rejected by UI gate");
        return;
    }
    let current = AuraPowerState {
        zone: AuraPowerState::KEYBOARD_ZONE,
        boot: window.get_aura_power_boot(),
        awake: window.get_aura_power_awake(),
        sleep: window.get_aura_power_sleep(),
        shutdown: window.get_aura_power_shutdown(),
    };
    let Some(requested) = with_power_toggle(current, which, on) else {
        return;
    };
    let Some(context) = begin_mutation(window) else {
        return;
    };
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    context.runtime.spawn(async move {
        let result = async {
            let client = HardwareProductControlClient::connect_system().await?;
            client.set_aura_power(requested).await
        }
        .await;
        completion.store(false, Ordering::Release);
        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_applying(false);
            match result {
                Ok(observed) => {
                    apply_power_state(&window, Some(observed));
                    window.set_status("Подсветка по состояниям сохранена".into());
                }
                Err(error) => {
                    apply_power_state(&window, Some(current));
                    window.set_status(
                        format!(
                            "Не удалось изменить подсветку по состояниям · {}",
                            write_error_label(&error)
                        )
                        .into(),
                    );
                }
            }
            refresh(&window);
        }) {
            tracing::warn!(error = ?error, "failed to publish Aura power result");
        }
    });
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
                    window.set_status(
                        if observed.confirmed {
                            "Эффект Aura применён"
                        } else {
                            "Эффект Aura отправлен в прошивку · подтверждение недоступно"
                        }
                        .into(),
                    );
                }
                Err(error) => window.set_status(
                    format!(
                        "Не удалось применить эффект Aura · {}",
                        write_error_label(&error)
                    )
                    .into(),
                ),
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

    window.set_status("Применение Panel Overdrive…".into());
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
                            "Panel Overdrive {} · подтверждено",
                            if observed {
                                "включён"
                            } else {
                                "выключен"
                            }
                        )
                        .into(),
                    );
                }
                Err(error) => {
                    tracing::warn!(error = ?error, "Extra Panel Overdrive mutation failed");
                    window.set_status(
                        format!(
                            "Не удалось изменить Panel Overdrive · {}",
                            write_error_label(&error)
                        )
                        .into(),
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
    window.set_status("Изменение режима закрытой крышки…".into());
    let weak = window.as_weak();
    let completion = context.mutating.clone();
    let clamshell = context.clamshell.clone();
    context.runtime.spawn(async move {
        let result = clamshell.set_clamshell(enabled).await;
        let lid_handler = lid_handler_name(clamshell.as_ref()).await;
        completion.store(false, Ordering::Release);
        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_applying(false);
            let state = clamshell_ui_state(result);
            window.set_auto_clamshell_state(state);
            window.set_lid_other_handler(lid_handler.into());
            window.set_status(clamshell_status(state).into());
        }) {
            tracing::warn!(error = ?error, "failed to publish clamshell result");
        }
    });
}

async fn lid_handler_name(source: &dyn SessionClamshellSource) -> String {
    match tokio::time::timeout(READ_TIMEOUT, source.other_lid_handler()).await {
        Ok(Ok(name)) => name.unwrap_or_default(),
        Ok(Err(error)) => {
            tracing::debug!(error = ?error, "lid handler lookup unavailable");
            String::new()
        }
        Err(_) => String::new(),
    }
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
        ClamshellState::Loading => "Режим закрытой крышки определяется",
        ClamshellState::Inactive => "Режим закрытой крышки выключен",
        ClamshellState::Active => "Режим закрытой крышки включён",
        ClamshellState::Unavailable => "Режим закрытой крышки недоступен",
        ClamshellState::PermissionDenied => "Режим закрытой крышки недоступен · нет разрешения",
        ClamshellState::StartFailed => "Не удалось включить режим закрытой крышки",
        ClamshellState::ExitFailed => "Не удалось выключить режим закрытой крышки",
        ClamshellState::Unknown => "Состояние режима закрытой крышки неизвестно",
    }
}

pub(crate) fn refresh(window: &AppWindow) {
    refresh_with_status(window, None, None, true);
}

fn refresh_with_status(
    window: &AppWindow,
    final_status: Option<String>,
    pending: Option<bool>,
    reconcile_advanced: bool,
) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        reset_readiness(window);
        window.set_status("Служба расширенных параметров недоступна".into());
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
    window.set_status("Обновление состояния…".into());

    let weak = window.as_weak();
    let completion = context.refreshing.clone();
    context.runtime.spawn(async move {
        let (refresh_results, write_statuses, clamshell_result, lid_handler) = tokio::join!(
            (context.refresh_reads)(),
            (context.write_statuses)(),
            context.clamshell.read_clamshell(),
            lid_handler_name(context.clamshell.as_ref()),
        );
        let (aura_result, panel_result, boot_sound_result, apu_result, aspm_result) =
            refresh_results;

        let aura = aura_observed(aura_result);
        let panel = panel_observed(panel_result);
        let boot_sound = boot_sound_observed(boot_sound_result);
        let apu = apu_observed(apu_result);
        let aspm_observed = aspm_result.as_ref().ok().map(|(value, _)| *value);
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
            window.set_lid_other_handler(lid_handler.into());

            if let Some(context) = CONTEXT.with(|slot| slot.borrow().clone()) {
                let boot_observed = boot_sound.ready.then_some(boot_sound.enabled);
                let igpu_observed = apu.ready.then_some(apu.value.clamp(0, 8) as u8);
                let aspm_observed = if aspm_write != ProductWriteStatus::Unknown {
                    aspm_observed
                } else {
                    None
                };
                let unresolved = context.advanced_unresolved.load(Ordering::Acquire);
                let dirty = context.advanced_dirty.load(Ordering::Acquire);
                let (unresolved, dirty) = if reconcile_advanced {
                    advanced_refresh_reconciliation(
                        unresolved,
                        dirty,
                        *context.advanced_draft.lock().unwrap(),
                        boot_observed,
                        igpu_observed,
                        aspm_observed,
                    )
                } else {
                    (unresolved, dirty)
                };
                context
                    .advanced_unresolved
                    .store(unresolved, Ordering::Release);
                context.advanced_dirty.store(dirty, Ordering::Release);
                if !unresolved {
                    *context.advanced_draft.lock().unwrap() = None;
                }
                let readiness = advanced_readiness(
                    unresolved,
                    dirty,
                    boot_sound.ready,
                    boot_sound_write,
                    apu.ready,
                    apu_write,
                    aspm_write,
                );
                window.set_boot_sound_control_ready(readiness.boot_sound);
                window.set_igpu_memory_control_ready(readiness.igpu_memory);
                window.set_aspm_control_ready(readiness.aspm);
                window.set_advanced_apply_ready(readiness.apply);
            } else {
                window.set_advanced_apply_ready(false);
            }

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
            apply_power_state(&window, aura.power);

            window.set_panel_overdrive_state_ready(panel.ready);
            window.set_panel_overdrive_control_ready(panel.ready && panel_write.is_supported());
            window.set_panel_overdrive(panel.enabled);

            window.set_boot_sound_state_ready(boot_sound.ready);
            window.set_boot_sound(boot_sound.enabled);
            window.set_igpu_memory_state_ready(apu.ready);
            window.set_igpu_memory(apu.value);
            window.set_igpu_memory_pending(pending.unwrap_or(false));
            window.set_aspm_state_ready(aspm_write != ProductWriteStatus::Unknown);
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
            tracing::debug!(
                status = %observed_status,
                clamshell = clamshell_status(clamshell_state),
                "advanced hardware state refreshed"
            );
            window.set_status(final_status.unwrap_or_default().into());
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
    let mut state = tokio::time::timeout(READ_TIMEOUT, provider.aura_state())
        .await
        .map_err(|_| ProviderError::Timeout("Extra Aura read timed out".into()))??;
    let kernel_modes = async {
        let client = HardwareProductControlClient::connect_system().await?;
        client.aura_kernel_effect_modes().await
    }
    .await
    .unwrap_or_default();
    for mode in kernel_modes.into_iter().map(AuraMode::from_u32) {
        if !matches!(mode, AuraMode::Unknown(_)) && !state.supported_modes.contains(&mode) {
            state.supported_modes.push(mode);
        }
    }
    Ok(state)
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

async fn bounded_refresh_reads() -> RefreshReads {
    let panel_provider = AsusArmouryPanelOverdriveProvider::default();
    let boot_sound_provider = AsusBootSoundProvider::default();
    tokio::join!(
        bounded_aura_read(),
        bounded_panel_read(&panel_provider),
        bounded_boot_sound_read(&boot_sound_provider),
        bounded_apu_memory_read(),
        bounded_aspm_read(),
    )
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
    bounded_write_statuses_with(&client).await
}

async fn bounded_write_statuses_with(
    client: &HardwareProductControlClient,
) -> Result<
    (
        ProductWriteStatus,
        ProductWriteStatus,
        ProductWriteStatus,
        ProductWriteStatus,
    ),
    ProviderError,
> {
    let (keyboard, panel, boot_sound, apu) = tokio::join!(
        client.keyboard_status(),
        client.panel_status(),
        client.boot_sound_status(),
        client.apu_memory_status(),
    );
    Ok((keyboard?, panel?, boot_sound?, apu?))
}

fn aura_observed(result: Result<orbis_core::aura::AuraState, ProviderError>) -> AuraObserved {
    let result = result.map(|state| {
        let asusd_now = AuraEffectSnapshot {
            mode: state.current_effect.mode.to_u32(),
            speed: state.current_effect.speed.as_str().to_string(),
            colour1: state.current_effect.colour1,
            colour2: state.current_effect.colour2,
        };
        let overlay = kernel_effect_overlay(&asusd_now);
        (state, overlay)
    });
    match result {
        Ok((state, Some(sent))) => {
            let mode = AuraMode::from_u32(sent.mode);
            AuraObserved {
                ready: true,
                effect: aura_effect_index(mode),
                speed: aura_speed_index(&sent.speed.parse().unwrap()),
                supported_modes: aura_supported_modes(&state),
                colour1: sent.colour1,
                colour2: sent.colour2,
                power: keyboard_power_state(&state),
                status: format!(
                    "Aura mode={} speed={} (отправлено в прошивку, не подтверждено)",
                    aura_mode_label(mode),
                    sent.speed
                ),
            }
        }
        Ok((state, None)) => AuraObserved {
            ready: true,
            effect: aura_effect_index(state.current_mode),
            speed: aura_speed_index(&state.current_effect.speed),
            supported_modes: aura_supported_modes(&state),
            colour1: state.current_effect.colour1,
            colour2: state.current_effect.colour2,
            power: keyboard_power_state(&state),
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
            power: None,
            status: error_status("Aura", &error),
        },
    }
}

fn keyboard_power_state(state: &orbis_core::aura::AuraState) -> Option<AuraPowerState> {
    state
        .power_states
        .iter()
        .copied()
        .find(|s| s.zone == AuraPowerState::KEYBOARD_ZONE)
}

fn aura_supported_modes(state: &orbis_core::aura::AuraState) -> [bool; 13] {
    let mut modes = [false; 13];
    for mode in &state.supported_modes {
        if let Some(slot) = modes.get_mut(aura_effect_index(*mode).max(0) as usize) {
            *slot = !matches!(mode, AuraMode::Unknown(_));
        }
    }
    modes
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
        AuraMode::Unknown(_) => -1,
        known => known.to_u32() as i32,
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
        ProviderError::Unsupported(_) => "запись не поддерживается",
        ProviderError::BackendUnavailable(_) => "служба недоступна",
        ProviderError::PermissionDenied(_) => "доступ запрещён",
        ProviderError::Timeout(_) => "истекло время ожидания",
        _ => "ошибка записи",
    }
}

fn advanced_apply_error_status(error: &AdvancedApplyError) -> String {
    let reason = write_error_label(&error.error);
    let Some(failed) = error.failed.map(advanced_apply_control_label) else {
        return format!("Не удалось применить · {reason} · состояние обновлено");
    };
    if matches!(error.error, ProviderError::Timeout(_)) {
        let completed = if error.completed.is_empty() {
            String::new()
        } else {
            format!(
                "{}; ",
                error
                    .completed
                    .iter()
                    .map(|control| format!("{}: применено", advanced_apply_control_label(*control)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        return format!(
            "Исход неизвестен · {completed}{failed}: возможно изменено · {reason} · состояние обновлено"
        );
    }
    if error.completed.is_empty() {
        return format!("Не удалось применить · {failed}: {reason} · состояние обновлено");
    }
    let completed = error
        .completed
        .iter()
        .map(|control| format!("{}: применено", advanced_apply_control_label(*control)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Применено частично · {completed}; {failed}: {reason} · состояние обновлено")
}

fn advanced_apply_control_label(control: AdvancedApplyControl) -> &'static str {
    match control {
        AdvancedApplyControl::BootSound => "звук при включении",
        AdvancedApplyControl::IgpuMemory => "память iGPU",
        AdvancedApplyControl::Aspm => "ASPM",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn power_toggle_changes_only_the_chosen_state() {
        let current = AuraPowerState {
            zone: AuraPowerState::KEYBOARD_ZONE,
            boot: true,
            awake: true,
            sleep: false,
            shutdown: false,
        };
        let next = with_power_toggle(current, 2, true).unwrap();
        assert_eq!(
            next,
            AuraPowerState {
                sleep: true,
                ..current
            }
        );
        assert!(!with_power_toggle(current, 0, false).unwrap().boot);
        assert!(with_power_toggle(current, 9, true).is_none());
    }
    use crate::ClamshellState;
    use orbis_core::aura::{
        AuraBrightness, AuraDirection, AuraEffect, AuraRgb, AuraState, AuraZone,
    };
    use orbis_session_client::{HardwareProductGpuSource, ProductGpuMutationResult};

    const OBJECT_PATH: &str = "/io/github/orbiscontrol/Hardware";

    struct FakeClamshell;

    #[async_trait::async_trait]
    impl SessionClamshellSource for FakeClamshell {
        async fn read_clamshell(&self) -> Result<u8, ProviderError> {
            Ok(clamshell::ACTIVE)
        }

        async fn set_clamshell(&self, _enabled: bool) -> Result<u8, ProviderError> {
            Ok(clamshell::ACTIVE)
        }
    }

    #[test]
    fn production_refresh_publishes_injected_reads_to_app_window() {
        i_slint_backend_testing::init_integration_test_with_mock_time();
        let app = AppWindow::new().expect("construct headless AppWindow");
        app.set_status("before callback".into());

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("build fixture runtime");
        let (refresh_started, refresh_started_rx) = std::sync::mpsc::channel();
        let injected = Arc::new(AtomicUsize::new(0));
        let calls = injected.clone();
        let hardware = FakeAdvancedHardware {
            state: Arc::new(Mutex::new((false, 2, false))),
            boot_status: 0,
            fail_on: None,
        };
        let (initial_server, initial_connection) =
            runtime.block_on(private_advanced_peer(hardware));
        let peer = Arc::new(Mutex::new(HardwareProductControlClient::new(
            initial_connection,
        )));
        let peer_for_status = peer.clone();
        let server = Arc::new(Mutex::new(Some(initial_server)));
        let fixture_runtime = runtime.handle().clone();
        let context = ExtraContext {
            runtime: runtime.handle().clone(),
            refreshing: Arc::new(AtomicBool::new(false)),
            mutating: Arc::new(AtomicBool::new(false)),
            advanced_dirty: Arc::new(AtomicBool::new(false)),
            advanced_unresolved: Arc::new(AtomicBool::new(false)),
            advanced_draft: Arc::new(Mutex::new(None)),
            clamshell: Arc::new(FakeClamshell),
            refresh_reads: Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                refresh_started.send(()).expect("signal refresh read start");
                Box::pin(async {
                    (
                        Ok(AuraState {
                            current_mode: AuraMode::Static,
                            current_effect: AuraEffect {
                                mode: AuraMode::Static,
                                zone: AuraZone::None,
                                colour1: AuraRgb {
                                    r: 12,
                                    g: 34,
                                    b: 56,
                                },
                                colour2: AuraRgb { r: 0, g: 0, b: 0 },
                                speed: AuraSpeed::Med,
                                direction: AuraDirection::Right,
                            },
                            brightness: AuraBrightness::Med,
                            supported_modes: vec![AuraMode::Static],
                            supported_zones: Vec::new(),
                            supported_brightness: vec![AuraBrightness::Med],
                            power_states: Vec::new(),
                        }),
                        Ok(PanelOverdriveState::Enabled),
                        Ok(BootSoundState::Enabled),
                        Ok(6),
                        Ok((true, ProductWriteStatus::Supported)),
                    )
                })
            }),
            write_statuses: Arc::new(move || {
                let client = peer_for_status.lock().expect("peer client lock").clone();
                Box::pin(async move { bounded_write_statuses_with(&client).await })
            }),
        };
        CONTEXT.with(|slot| *slot.borrow_mut() = Some(context));
        wire_window(&app);

        refresh_with_status(&app, None, None, true);
        refresh_started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("production refresh reaches injected read source");

        let (cancel_watchdog, watchdog_cancelled) = std::sync::mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            if watchdog_cancelled
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_err()
            {
                let _ = slint::quit_event_loop();
            }
        });
        let (observed, observed_rx) = std::sync::mpsc::channel();
        let observed = Arc::new(Mutex::new(Some(observed)));
        let poll_weak = app.as_weak();
        let apply_phase = Arc::new(AtomicUsize::new(0));
        let poll_apply_phase = apply_phase.clone();
        let poll_deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let poll = Arc::new(Mutex::new(None::<Box<dyn FnMut() + Send>>));
        let poll_again = poll.clone();
        let phase = Arc::new(AtomicUsize::new(0));
        let poll_phase = phase.clone();
        let poll_peer = peer.clone();
        let poll_server = server.clone();
        let poll_runtime = fixture_runtime.clone();
        let poll_once: Box<dyn FnMut() + Send> = Box::new(move || {
            let current_phase = poll_phase.load(Ordering::SeqCst);
            let ready = poll_weak.upgrade().is_some_and(|window| {
                window.get_aura_state_ready()
                    && window.get_rgb_red() == 12
                    && window.get_boot_sound_control_ready() == (current_phase != 1)
                    && window.get_igpu_memory_control_ready() == (current_phase != 1)
                    && window.get_aspm_control_ready()
            });
            if ready && current_phase == 0 && poll_apply_phase.load(Ordering::SeqCst) == 0 {
                let window = poll_weak.upgrade().expect("AppWindow remains alive");
                window.invoke_boot_sound_requested(!window.get_boot_sound());
                assert!(window.get_advanced_apply_ready());
                poll_apply_phase.store(1, Ordering::SeqCst);
            }
            let ready = ready
                && poll_weak.upgrade().is_some_and(|window| {
                    window.get_advanced_apply_ready() == (current_phase == 0)
                });
            if ready && current_phase < 2 {
                let next_phase = current_phase + 1;
                poll_phase.store(next_phase, Ordering::SeqCst);
                if next_phase == 1 {
                    drop(poll_server.lock().expect("peer server lock").take());
                    let client = poll_peer.lock().expect("peer client lock").clone();
                    assert!(
                        poll_runtime
                            .block_on(bounded_write_statuses_with(&client))
                            .is_err()
                    );
                } else {
                    let hardware = FakeAdvancedHardware {
                        state: Arc::new(Mutex::new((false, 2, false))),
                        boot_status: 0,
                        fail_on: None,
                    };
                    let (new_server, new_connection) =
                        poll_runtime.block_on(private_advanced_peer(hardware));
                    *poll_peer.lock().expect("peer client lock") =
                        HardwareProductControlClient::new(new_connection);
                    *poll_server.lock().expect("peer server lock") = Some(new_server);
                }
                let weak = poll_weak.clone();
                slint::invoke_from_event_loop(move || {
                    let window = weak.upgrade().expect("AppWindow remains alive");
                    refresh_with_status(&window, None, None, true);
                })
                .expect("schedule next production refresh after publication");
                let next = poll_again.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(poll) = next.lock().expect("poll callback lock").as_mut() {
                        poll();
                    }
                })
                .expect("requeue owner-thread getter check");
            } else if ready {
                observed
                    .lock()
                    .expect("observation sender lock")
                    .take()
                    .expect("observation sender available")
                    .send(())
                    .expect("signal final publication");
                let callback_weak = poll_weak.clone();
                slint::invoke_from_event_loop(move || {
                    let window = callback_weak.upgrade().expect("AppWindow stays alive");
                    window.set_status("callback delivered".into());
                    slint::quit_event_loop().expect("quit after final callback status update");
                })
                .expect("schedule H1 callback after recovery publication");
            } else if std::time::Instant::now() >= poll_deadline {
                observed.lock().expect("observation sender lock").take();
                slint::quit_event_loop().expect("quit after getter watchdog deadline");
            } else {
                let next = poll_again.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(poll) = next.lock().expect("poll callback lock").as_mut() {
                        poll();
                    }
                })
                .expect("requeue owner-thread getter check");
            }
        });
        *poll.lock().expect("poll callback lock") = Some(poll_once);
        let first_poll = poll.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(poll) = first_poll.lock().expect("poll callback lock").as_mut() {
                poll();
            }
        })
        .expect("queue first owner-thread getter check");
        slint::run_event_loop().expect("pump bounded Slint event loop");
        observed_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("event loop quit after bounded getter check");
        assert_eq!(app.get_status(), "callback delivered");
        cancel_watchdog
            .send(())
            .expect("cancel event-loop watchdog");
        watchdog.join().expect("event-loop watchdog completes");
        assert_eq!(injected.load(Ordering::SeqCst), 3);
        assert!(app.get_aura_state_ready());
        assert_eq!(app.get_rgb_red(), 12);
        assert_eq!(app.get_rgb_green(), 34);
        assert_eq!(app.get_rgb_blue(), 56);
        assert!(app.get_panel_overdrive());
        assert!(app.get_boot_sound());
        assert_eq!(app.get_igpu_memory(), 6);
        assert!(app.get_disable_aspm());
        assert_eq!(app.get_auto_clamshell_state(), ClamshellState::Active);
        assert!(!app.get_advanced_apply_ready());
        CONTEXT.with(|slot| slot.borrow_mut().take());
        drop(runtime);
    }

    #[derive(Clone)]
    struct FakeAdvancedHardware {
        state: Arc<Mutex<(bool, u8, bool)>>,
        boot_status: u8,
        fail_on: Option<AdvancedApplyControl>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl FakeAdvancedHardware {
        async fn keyboard_backlight_mutation_status(&self) -> u8 {
            0
        }

        async fn panel_mutation_status(&self) -> u8 {
            0
        }

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

    #[derive(Clone)]
    struct FakeProductGpuHardware {
        state: Arc<Mutex<ProductGpuMutationResult>>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl FakeProductGpuHardware {
        async fn set_product_gpu_mode(
            &self,
            requested_mode: u32,
        ) -> zbus::fdo::Result<ProductGpuMutationResult> {
            let mut state = self.state.lock().unwrap();
            *state = ProductGpuMutationResult {
                requested_mode,
                current_mode: state.current_mode,
                queued_mode: requested_mode,
                outcome: 1,
                reboot_required: true,
            };
            Ok(*state)
        }

        async fn product_gpu_status(&self) -> zbus::fdo::Result<ProductGpuMutationResult> {
            Ok(*self.state.lock().unwrap())
        }
    }

    async fn private_product_gpu_peer(
        hardware: FakeProductGpuHardware,
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

    #[tokio::test]
    async fn private_product_gpu_peer_mutates_reads_back_and_recovers_owner() {
        let initial = ProductGpuMutationResult {
            requested_mode: 0,
            current_mode: 0,
            queued_mode: u32::MAX,
            outcome: 0,
            reboot_required: false,
        };
        let first_hardware = FakeProductGpuHardware {
            state: Arc::new(Mutex::new(initial)),
        };
        let first_state = first_hardware.state.clone();
        let (first_server, first_connection) = private_product_gpu_peer(first_hardware).await;
        let first_client =
            orbis_session_client::ZbusHardwareProductGpuSource::new(first_connection);

        let mutation = first_client.set_product_gpu_mode(2).await.unwrap();
        assert_eq!(mutation.queued_mode, 2);
        assert!(mutation.reboot_required);
        let read_back = first_client.product_gpu_status().await.unwrap();
        assert_eq!(read_back.current_mode, 0);
        assert_eq!(read_back.queued_mode, 2);
        assert_eq!(*first_state.lock().unwrap(), read_back);

        drop(first_server);
        assert!(first_client.product_gpu_status().await.is_err());

        let recovered_hardware = FakeProductGpuHardware {
            state: Arc::new(Mutex::new(initial)),
        };
        let recovered_state = recovered_hardware.state.clone();
        let (_recovered_server, recovered_connection) =
            private_product_gpu_peer(recovered_hardware).await;
        let recovered_client =
            orbis_session_client::ZbusHardwareProductGpuSource::new(recovered_connection);
        let recovered = recovered_client.product_gpu_status().await.unwrap();
        assert_eq!(recovered.queued_mode, u32::MAX);
        assert_eq!(*recovered_state.lock().unwrap(), initial);
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
        assert_eq!(aura_effect_index(AuraMode::Flash), 12);
        assert_eq!(aura_effect_index(AuraMode::RainbowWave), 3);
        assert_eq!(aura_effect_index(AuraMode::Pulse), 10);
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
    async fn hardware_owner_loss_produces_disabled_advanced_readiness() {
        let hardware = FakeAdvancedHardware {
            state: Arc::new(Mutex::new((false, 2, false))),
            boot_status: 0,
            fail_on: None,
        };
        let (server, connection) = private_advanced_peer(hardware).await;
        let client = HardwareProductControlClient::new(connection);
        let statuses = bounded_write_statuses_with(&client).await.unwrap();
        let ready = advanced_readiness(
            false,
            true,
            true,
            statuses.2,
            true,
            statuses.3,
            ProductWriteStatus::Supported,
        );
        assert!(ready.boot_sound && ready.igpu_memory && ready.aspm && ready.apply);

        drop(server);
        let unavailable = bounded_write_statuses_with(&client).await;
        assert!(unavailable.is_err());
        let readiness = advanced_readiness(
            false,
            true,
            true,
            ProductWriteStatus::Unknown,
            true,
            ProductWriteStatus::Unknown,
            ProductWriteStatus::Unknown,
        );
        assert!(!readiness.boot_sound);
        assert!(!readiness.igpu_memory);
        assert!(!readiness.aspm);
        assert!(!readiness.apply);
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
            "Применено частично · звук при включении: применено; память iGPU: ошибка записи · состояние обновлено"
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
            "Не удалось применить · звук при включении: запись не поддерживается · состояние обновлено"
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
            "Применено частично · звук при включении: применено; память iGPU: служба недоступна · состояние обновлено"
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
            "Исход неизвестен · ASPM: возможно изменено · истекло время ожидания · состояние обновлено"
        );
    }

    #[test]
    fn ambiguous_apply_keeps_partial_completion_evidence_and_blocks_writes() {
        let error = AdvancedApplyError {
            completed: vec![AdvancedApplyControl::BootSound],
            failed: Some(AdvancedApplyControl::IgpuMemory),
            error: ProviderError::Timeout("dispatch ambiguous".into()),
        };

        assert_eq!(
            advanced_apply_error_status(&error),
            "Исход неизвестен · звук при включении: применено; память iGPU: возможно изменено · истекло время ожидания · состояние обновлено"
        );
        assert!(advanced_apply_requires_reconciliation(&error));
        assert!(!advanced_control_writable(true, true, true));
    }

    #[test]
    fn ambiguous_refresh_only_clears_dirty_after_authoritative_reconciliation() {
        let draft = advanced_apply_draft(true, 6, true, true, true, true);

        assert_eq!(
            advanced_refresh_reconciliation(
                true,
                true,
                Some(draft),
                Some(true),
                Some(6),
                Some(true)
            ),
            (false, false)
        );
        assert_eq!(
            advanced_refresh_reconciliation(true, true, Some(draft), Some(true), None, Some(true)),
            (true, true)
        );
        assert_eq!(
            advanced_refresh_reconciliation(
                true,
                true,
                Some(draft),
                Some(true),
                Some(4),
                Some(true)
            ),
            (false, true)
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
            power_states: Vec::new(),
        }));
        assert!(state.ready);
        assert_eq!(state.effect, 0);
        assert_eq!(state.speed, -1);
    }

    #[test]
    fn advanced_controls_have_no_gpu_fan_or_legacy_rgb_write() {
        let source = include_str!("extra_backend.rs");
        assert!(!source.contains(&["set_aura", "_static_rgb"].concat()));
        assert!(!source.contains(&["set_gpu", "_mode"].concat()));
        assert!(!source.contains(&["set_fan", "_curve"].concat()));
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
    }

    #[test]
    fn clamshell_ui_status_covers_every_explicit_state() {
        for (state, marker) in [
            (ClamshellState::Loading, "определяется"),
            (ClamshellState::Inactive, "выключен"),
            (ClamshellState::Active, "включён"),
            (ClamshellState::Unavailable, "недоступен"),
            (ClamshellState::PermissionDenied, "нет разрешения"),
            (ClamshellState::StartFailed, "не удалось включить"),
            (ClamshellState::ExitFailed, "не удалось выключить"),
            (ClamshellState::Unknown, "неизвестно"),
        ] {
            assert!(
                clamshell_status(state).to_lowercase().contains(marker),
                "{state:?}"
            );
        }
    }
}
