//! Orbis Control — GUI entry point.
//!
//! Hardware-backed controls keep their worker/provider paths. Secondary UI
//! surfaces are enabled only when their corresponding production backend or
//! lifecycle contract is wired and can publish authoritative state.

#[allow(dead_code)]
mod controller;
mod diagnostics_backend;
mod launch_context;
mod preferences_backend;
mod quick_controls_backend;

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use orbis_application::{
    ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome, PerformanceCommandOutcome,
    PerformanceState, SetChargeLimitError, SetGpuModeError,
};
use orbis_config::{
    PreferencesConfig, PreferencesError, PreferencesLoad, PreferencesWarning, ThemePreference,
    load_preferences, save_preferences,
};
use orbis_core::action::{ActionRequirement, ApplyResult};
use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_session_client::{
    HardwareProductGpuSource, ProductGpuMutationResult, ZbusHardwareProductGpuSource,
};
use orbis_ui::composition::build_production_runtime;
use orbis_ui::worker::{WorkerCommand, WorkerEvent, run_worker_with_product_gpu};
use slint::platform::{Platform, PlatformError, Renderer, WindowAdapter, WindowEvent};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, LogicalSize, PhysicalSize, Rgb8Pixel, WindowSize};
use tokio::sync::mpsc::UnboundedSender;

slint::include_modules!();

thread_local! {
    static PREVIEW_DIALOG_WINDOW: RefCell<Option<PreviewDialogWindow>> = const { RefCell::new(None) };
    static THEME_LIGHT: Cell<bool> = const { Cell::new(false) };
}

/// Разобранные аргументы командной строки.
struct Args {
    ui_state: String,
    ui_section: Option<String>,
    screenshot: Option<String>,
}

fn parse_args() -> Args {
    let mut ui_state = "default".to_string();
    let mut ui_section = None;
    let mut screenshot = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ui-state" => {
                if let Some(v) = it.next() {
                    ui_state = v;
                }
            }
            "--screenshot" => screenshot = it.next(),
            "--ui-section" => {
                if let Some(v) = it.next() {
                    ui_section = Some(v);
                }
            }
            other => eprintln!("orbis-control: игнорирую неизвестный аргумент '{other}'"),
        }
    }
    Args {
        ui_state,
        ui_section,
        screenshot,
    }
}

/// Scenario state for UI-review screenshot builds (`ui-review` feature).
#[cfg(any(test, feature = "ui-review"))]
fn scenario_state(name: &str) -> anyhow::Result<controller::UiState> {
    let mut s = controller::UiState::from_mock_profile("zephyrus-full");
    match name {
        "default" => {}
        "pending" => {
            s.gpu_queued = 2;
            s.gpu_reboot_required = true;
            s.gpu_selected = 2;
            s.gpu_ultimate_pending = true;
        }
        "disabled" => s.gpu_ultimate_disabled = true,
        "error" => s.gpu_section_error = true,
        "dirty" => {
            s.fan_selected = 1;
            s.fan_curve_state = controller::FanCurveHwState::Ready;
            s.fan_curve_writable = true;
            s.fan_curve_temps = [35, 45, 55, 65, 72, 80, 88, 96];
            s.fan_curve_pwms = [0, 20, 45, 70, 100, 135, 190, 255];
            s.fan_curve_dirty = true;
        }
        "fan-error" => {
            s.fan_selected = 1;
            s.fan_curve_state = controller::FanCurveHwState::Ready;
            s.fan_curve_writable = true;
            s.fan_curve_error = true;
        }
        other => eprintln!("orbis-control: неизвестное состояние '{other}', использую default"),
    }
    Ok(s)
}

/// Release builds keep no fixture-derived scenario states (#115).
#[cfg(not(any(test, feature = "ui-review")))]
fn scenario_state(_name: &str) -> anyhow::Result<controller::UiState> {
    anyhow::bail!("--screenshot/--ui-state scenarios require a build with --features ui-review")
}

fn to_slint(state: &controller::UiState) -> UiState {
    UiState {
        capability_generation: state.capability_generation as i32,
        perf_selected: state.perf_selected,
        available_perf_mask: state.available_perf_mask,
        perf_state: match state.perf_state {
            controller::PerformanceHwState::Loading => PerformanceHwState::Loading,
            controller::PerformanceHwState::Ready => PerformanceHwState::Ready,
            controller::PerformanceHwState::Unavailable => PerformanceHwState::Unavailable,
        },
        perf_writable: state.perf_writable,
        perf_unavailable_reason: state
            .perf_unavailable_reason
            .clone()
            .unwrap_or_default()
            .into(),
        gpu_selected: state.gpu_selected,
        available_gpu_mask: state.available_gpu_mask,
        gpu_ultimate_pending: state.gpu_ultimate_pending,
        gpu_ultimate_disabled: state.gpu_ultimate_disabled,
        gpu_section_error: state.gpu_section_error,
        gpu_mode_state: match state.gpu_mode_state {
            controller::GpuModeHwState::Loading => GpuModeHwState::Loading,
            controller::GpuModeHwState::Ready => GpuModeHwState::Ready,
            controller::GpuModeHwState::Unavailable => GpuModeHwState::Unavailable,
        },
        gpu_mode_writable: state.gpu_mode_writable,
        gpu_queued: state.gpu_queued,
        gpu_reboot_required: state.gpu_reboot_required,
        charge_limit: state.charge_limit,
        charge_limit_enabled: state.charge_limit_enabled,
        charge_limit_writable: state.charge_limit_writable,
        charge_limit_unavailable_reason: state
            .charge_limit_unavailable_reason
            .clone()
            .unwrap_or_default()
            .into(),
        charge_limit_state: match state.charge_limit_state {
            controller::ChargeLimitState::Loading => ChargeLimitState::Loading,
            controller::ChargeLimitState::Ready => ChargeLimitState::Ready,
            controller::ChargeLimitState::Unavailable => ChargeLimitState::Unavailable,
        },
        gpu_power_state: match state.gpu_power {
            controller::GpuHwState::Loading => GpuHwState::Loading,
            controller::GpuHwState::Ready => GpuHwState::Ready,
            controller::GpuHwState::Unavailable => GpuHwState::Unavailable,
        },
        gpu_mux_state: match state.gpu_mux {
            controller::GpuHwState::Loading => GpuHwState::Loading,
            controller::GpuHwState::Ready => GpuHwState::Ready,
            controller::GpuHwState::Unavailable => GpuHwState::Unavailable,
        },
        gpu_access_state: match state.gpu_access {
            controller::GpuHwState::Loading => GpuHwState::Loading,
            controller::GpuHwState::Ready => GpuHwState::Ready,
            controller::GpuHwState::Unavailable => GpuHwState::Unavailable,
        },
        gpu_power_value: state.gpu_power_value,
        gpu_mux_value: state.gpu_mux_value,
        gpu_access_value: state.gpu_access_value,
        cpu_temp: state.cpu_temp.clone().into(),
        gpu_temp: state.gpu_temp.clone().into(),
        cpu_fan_rpm: state.cpu_fan_rpm.clone().into(),
        gpu_fan_rpm: state.gpu_fan_rpm.clone().into(),
        battery_percent: state.battery_percent.clone().into(),
        battery_percent_value: state.battery_percent_value,
        power_ac_mw: state.power_ac.clone().into(),
        battery_health: state.battery_health.clone().into(),
        battery_cycles: state.battery_cycles.clone().into(),
        battery_status: state.battery_status.clone().into(),
        ac_online: state.ac_online.clone().into(),
        gpu_power: state.gpu_power_display.clone().into(),
        telemetry_fresh: state.telemetry_fresh,
        version: state.version.clone().into(),
        mock_profile: state.mock_profile.clone().into(),
        fan_curve_state: match state.fan_curve_state {
            controller::FanCurveHwState::Loading => FanCurveHwState::Loading,
            controller::FanCurveHwState::Ready => FanCurveHwState::Ready,
            controller::FanCurveHwState::Unavailable => FanCurveHwState::Unavailable,
        },
        fan_curve_writable: state.fan_curve_writable,
        fan_curve_unavailable_reason: state
            .fan_curve_unavailable_reason
            .clone()
            .unwrap_or_default()
            .into(),
        fan_curve_error: state.fan_curve_error,
        fan_curve_dirty: state.fan_curve_dirty,
        fan_curve_enabled_known: state.fan_curve_enabled.is_some(),
        fan_curve_enabled: state.fan_curve_enabled.unwrap_or(false),
        fan_selected: state.fan_selected,
        fan_profile_selected: state.fan_profile_selected,
        fan_temp_0: state.fan_curve_temps[0],
        fan_temp_1: state.fan_curve_temps[1],
        fan_temp_2: state.fan_curve_temps[2],
        fan_temp_3: state.fan_curve_temps[3],
        fan_temp_4: state.fan_curve_temps[4],
        fan_temp_5: state.fan_curve_temps[5],
        fan_temp_6: state.fan_curve_temps[6],
        fan_temp_7: state.fan_curve_temps[7],
        fan_pwm_0: state.fan_curve_pwms[0],
        fan_pwm_1: state.fan_curve_pwms[1],
        fan_pwm_2: state.fan_curve_pwms[2],
        fan_pwm_3: state.fan_curve_pwms[3],
        fan_pwm_4: state.fan_curve_pwms[4],
        fan_pwm_5: state.fan_curve_pwms[5],
        fan_pwm_6: state.fan_curve_pwms[6],
        fan_pwm_7: state.fan_curve_pwms[7],
    }
}

fn from_slint(state: &UiState) -> controller::UiState {
    controller::UiState {
        capability_generation: state.capability_generation.max(0) as u64,
        perf_selected: state.perf_selected,
        available_perf_mask: state.available_perf_mask,
        perf_state: match state.perf_state {
            PerformanceHwState::Loading => controller::PerformanceHwState::Loading,
            PerformanceHwState::Ready => controller::PerformanceHwState::Ready,
            PerformanceHwState::Unavailable => controller::PerformanceHwState::Unavailable,
        },
        perf_writable: state.perf_writable,
        gpu_selected: state.gpu_selected,
        available_gpu_mask: state.available_gpu_mask,
        gpu_ultimate_pending: state.gpu_ultimate_pending,
        gpu_ultimate_disabled: state.gpu_ultimate_disabled,
        gpu_section_error: state.gpu_section_error,
        gpu_mode_state: match state.gpu_mode_state {
            GpuModeHwState::Loading => controller::GpuModeHwState::Loading,
            GpuModeHwState::Ready => controller::GpuModeHwState::Ready,
            GpuModeHwState::Unavailable => controller::GpuModeHwState::Unavailable,
        },
        gpu_mode_writable: state.gpu_mode_writable,
        gpu_queued: state.gpu_queued,
        gpu_reboot_required: state.gpu_reboot_required,
        charge_limit: state.charge_limit,
        charge_limit_enabled: state.charge_limit_enabled,
        charge_limit_writable: state.charge_limit_writable,
        charge_limit_state: match state.charge_limit_state {
            ChargeLimitState::Loading => controller::ChargeLimitState::Loading,
            ChargeLimitState::Ready => controller::ChargeLimitState::Ready,
            ChargeLimitState::Unavailable => controller::ChargeLimitState::Unavailable,
        },
        gpu_power: match state.gpu_power_state {
            GpuHwState::Loading => controller::GpuHwState::Loading,
            GpuHwState::Ready => controller::GpuHwState::Ready,
            GpuHwState::Unavailable => controller::GpuHwState::Unavailable,
        },
        gpu_mux: match state.gpu_mux_state {
            GpuHwState::Loading => controller::GpuHwState::Loading,
            GpuHwState::Ready => controller::GpuHwState::Ready,
            GpuHwState::Unavailable => controller::GpuHwState::Unavailable,
        },
        gpu_access: match state.gpu_access_state {
            GpuHwState::Loading => controller::GpuHwState::Loading,
            GpuHwState::Ready => controller::GpuHwState::Ready,
            GpuHwState::Unavailable => controller::GpuHwState::Unavailable,
        },
        gpu_power_value: state.gpu_power_value,
        gpu_mux_value: state.gpu_mux_value,
        gpu_access_value: state.gpu_access_value,
        cpu_temp: state.cpu_temp.to_string(),
        gpu_temp: state.gpu_temp.to_string(),
        cpu_fan_rpm: state.cpu_fan_rpm.to_string(),
        gpu_fan_rpm: state.gpu_fan_rpm.to_string(),
        battery_percent: state.battery_percent.to_string(),
        battery_percent_value: state.battery_percent_value,
        power_ac: state.power_ac_mw.to_string(),
        version: state.version.to_string(),
        mock_profile: state.mock_profile.to_string(),
        perf_capability: controller::CapabilityAvailability::Unknown,
        perf_unavailable_reason: None,
        charge_limit_capability: controller::CapabilityAvailability::Unknown,
        charge_limit_unavailable_reason: None,
        gpu_power_capability: controller::CapabilityAvailability::Unknown,
        gpu_mux_capability: controller::CapabilityAvailability::Unknown,
        gpu_access_capability: controller::CapabilityAvailability::Unknown,
        battery_health: state.battery_health.to_string(),
        battery_cycles: state.battery_cycles.to_string(),
        battery_status: state.battery_status.to_string(),
        ac_online: state.ac_online.to_string(),
        gpu_power_display: state.gpu_power.to_string(),
        telemetry_fresh: state.telemetry_fresh,
        // Fan Curve Editor
        fan_curve_state: match state.fan_curve_state {
            FanCurveHwState::Loading => controller::FanCurveHwState::Loading,
            FanCurveHwState::Ready => controller::FanCurveHwState::Ready,
            FanCurveHwState::Unavailable => controller::FanCurveHwState::Unavailable,
        },
        fan_curve_writable: state.fan_curve_writable,
        fan_curve_unavailable_reason: None,
        fan_curve_error: state.fan_curve_error,
        fan_curve_dirty: state.fan_curve_dirty,
        fan_curve_enabled: if state.fan_curve_enabled_known {
            Some(state.fan_curve_enabled)
        } else {
            None
        },
        fan_selected: state.fan_selected,
        fan_profile_selected: state.fan_profile_selected,
        fan_curve_temps: [
            state.fan_temp_0,
            state.fan_temp_1,
            state.fan_temp_2,
            state.fan_temp_3,
            state.fan_temp_4,
            state.fan_temp_5,
            state.fan_temp_6,
            state.fan_temp_7,
        ],
        fan_curve_pwms: [
            state.fan_pwm_0,
            state.fan_pwm_1,
            state.fan_pwm_2,
            state.fan_pwm_3,
            state.fan_pwm_4,
            state.fan_pwm_5,
            state.fan_pwm_6,
            state.fan_pwm_7,
        ],
        fan_curve_capability: controller::CapabilityAvailability::Unknown,
    }
}

fn build_app(
    state: &controller::UiState,
    worker_tx: Option<UnboundedSender<WorkerCommand>>,
) -> Result<AppWindow, slint::PlatformError> {
    let app = AppWindow::new()?;
    app.global::<ThemeState>().set_mode(current_theme_mode());
    app.set_ui_state(to_slint(state));
    // Reset requires separate runtime capability evidence; fan write access alone is insufficient.
    app.set_factory_reset_available(false);
    apply_device_identity(&app);
    wire_callbacks(&app, worker_tx);
    quick_controls_backend::wire_window(&app);
    Ok(app)
}

/// Privacy-safe DMI identity for the device header and the System page.
///
/// Reuses the production read-only `HardwareIdentityProvider` allowlist
/// (vendor/product/board/BIOS only; no serial/UUID). Missing fields stay
/// empty and the UI renders honest placeholders.
fn apply_device_identity(app: &AppWindow) {
    let identity = orbis_providers::HardwareIdentityProvider::new().snapshot();
    if let Some(identity) = identity.identity {
        app.set_device_name(identity.product.into());
        app.set_device_board(identity.board.into());
        app.set_bios_version(identity.bios_version.into());
        app.set_bios_date(identity.bios_date.into());
    }
}

fn theme_mode(light: bool) -> ThemeMode {
    if light {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    }
}

fn current_theme_light() -> bool {
    THEME_LIGHT.with(Cell::get)
}

fn current_theme_mode() -> ThemeMode {
    theme_mode(current_theme_light())
}

fn set_current_theme_light(light: bool) {
    THEME_LIGHT.with(|state| state.set(light));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupPreferences {
    theme_light: bool,
    start_minimized: bool,
}

fn initial_startup_preferences_with(
    load: impl FnOnce() -> Result<PreferencesLoad, PreferencesError>,
) -> StartupPreferences {
    match load() {
        Ok(load) => {
            if let Some(warning) = &load.warning {
                tracing::warn!(
                    path = ?warning.path,
                    kind = ?warning.kind,
                    "preferences load warning; using safe runtime preferences"
                );
            }
            StartupPreferences {
                theme_light: matches!(load.preferences.appearance.theme, ThemePreference::Light),
                start_minimized: load.preferences.window.start_minimized,
            }
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "preferences load failed; using safe runtime preferences"
            );
            StartupPreferences {
                theme_light: false,
                start_minimized: false,
            }
        }
    }
}

fn initialize_runtime_preferences_with(
    load: impl FnOnce() -> Result<PreferencesLoad, PreferencesError>,
) -> StartupPreferences {
    let preferences = initial_startup_preferences_with(load);
    set_current_theme_light(preferences.theme_light);
    preferences
}

fn initialize_runtime_preferences() -> StartupPreferences {
    initialize_runtime_preferences_with(load_preferences)
}

#[cfg(test)]
fn initialize_runtime_theme_with(
    load: impl FnOnce() -> Result<PreferencesLoad, PreferencesError>,
) -> bool {
    initialize_runtime_preferences_with(load).theme_light
}

fn apply_start_minimized(start_minimized: bool, mut set_minimized: impl FnMut(bool)) {
    if start_minimized {
        set_minimized(true);
    }
}

#[derive(Debug)]
enum ThemePersistenceFailure {
    Load(PreferencesError),
    Preserve(PreferencesWarning),
    Save(PreferencesError),
}

impl std::fmt::Display for ThemePersistenceFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(error) => {
                write!(f, "failed to load preferences before saving theme: {error}")
            }
            Self::Preserve(warning) => write!(
                f,
                "refusing to overwrite preserved preferences source {:?}: {:?}",
                warning.path, warning.kind
            ),
            Self::Save(error) => write!(f, "failed to save theme preference: {error}"),
        }
    }
}

impl std::error::Error for ThemePersistenceFailure {}

fn persist_theme_with<L, S>(light: bool, load: L, save: S) -> Result<(), ThemePersistenceFailure>
where
    L: FnOnce() -> Result<PreferencesLoad, PreferencesError>,
    S: FnOnce(&PreferencesConfig) -> Result<std::path::PathBuf, PreferencesError>,
{
    let load = load().map_err(ThemePersistenceFailure::Load)?;
    if let Some(warning) = load.warning {
        return Err(ThemePersistenceFailure::Preserve(warning));
    }

    let mut preferences = load.preferences;
    preferences.appearance.theme = if light {
        ThemePreference::Light
    } else {
        ThemePreference::Dark
    };
    save(&preferences).map_err(ThemePersistenceFailure::Save)?;
    Ok(())
}

fn persist_theme(light: bool) -> Result<(), ThemePersistenceFailure> {
    persist_theme_with(light, load_preferences, save_preferences)
}

fn apply_theme_if_open<T>(window: Option<&T>, light: bool, apply: impl FnOnce(&T, ThemeMode)) {
    if let Some(window) = window {
        apply(window, theme_mode(light));
    }
}

fn apply_theme_to_all(app: &AppWindow, light: bool) {
    set_current_theme_light(light);
    app.global::<ThemeState>().set_mode(theme_mode(light));
    PREVIEW_DIALOG_WINDOW.with(|slot| {
        let slot = slot.borrow();
        apply_theme_if_open(slot.as_ref(), light, |window, mode| {
            window.global::<ThemeState>().set_mode(mode);
        });
    });
}

fn apply_autostart_state_to_window(
    window: &AppWindow,
    state: &preferences_backend::AutostartUiState,
) {
    window.set_startup(state.enabled);
    window.set_startup_enabled(state.writable);
    window.set_startup_status(state.status.into());
}

fn apply_window_preferences_state_to_window(
    window: &AppWindow,
    state: &preferences_backend::WindowPreferencesUiState,
) {
    window.set_start_minimized(state.start_minimized);
    window.set_start_minimized_enabled(state.start_minimized_writable);
    window.set_remember_position(state.remember_position);
    window.set_remember_position_enabled(state.remember_position_writable);
    window.set_close_action(state.close_action);
    window.set_close_action_enabled(state.close_action_writable);
    window.set_settings_local_status(state.status.into());
}

/// Apply the persisted preferences state onto the settings section surface.
fn apply_preferences_state(window: &AppWindow) {
    match preferences_backend::read_autostart_state() {
        Ok(state) => apply_autostart_state_to_window(window, &state),
        Err(error) => {
            tracing::warn!(error = %error, "failed to read user autostart state");
            window.set_startup(false);
            window.set_startup_enabled(false);
            window.set_startup_status("Autostart unavailable".into());
        }
    }

    match preferences_backend::read_window_preferences_state() {
        Ok(state) => apply_window_preferences_state_to_window(window, &state),
        Err(error) => {
            tracing::warn!(error = %error, "failed to read window preferences state");
            window.set_start_minimized(false);
            window.set_start_minimized_enabled(false);
            window.set_remember_position(false);
            window.set_remember_position_enabled(false);
            window.set_close_action(0);
            window.set_close_action_enabled(false);
            window.set_settings_local_status("Preferences backend unavailable".into());
        }
    }
}

fn show_preview_dialog(kind: i32) -> Result<(), slint::PlatformError> {
    PREVIEW_DIALOG_WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let window = PreviewDialogWindow::new()?;
            let weak = window.as_weak();
            window.on_dismiss_clicked(move || {
                if let Some(window) = weak.upgrade() {
                    let _ = window.hide();
                }
            });
            *slot = Some(window);
        }
        let window = slot.as_ref().expect("PreviewDialogWindow initialized");
        window.set_kind(kind.clamp(0, 3));
        window.global::<ThemeState>().set_mode(current_theme_mode());
        window.show()
    })
}

fn performance_profile_from_index(index: i32) -> Option<PerformanceProfile> {
    match index {
        0 => Some(PerformanceProfile::Silent),
        1 => Some(PerformanceProfile::Balanced),
        2 => Some(PerformanceProfile::Turbo),
        _ => None,
    }
}

fn perf_selected_index(profile: PerformanceProfile) -> i32 {
    match profile {
        PerformanceProfile::Silent => 0,
        PerformanceProfile::Balanced => 1,
        PerformanceProfile::Turbo => 2,
    }
}

fn performance_available_mask(available: &[PerformanceProfile]) -> i32 {
    let mut mask = 0;
    for p in available {
        mask |= 1 << perf_selected_index(*p);
    }
    mask
}

#[cfg(test)]
fn performance_write_available(name_has_owner: Option<bool>) -> bool {
    name_has_owner == Some(true)
}

fn battery_write_available(
    hardware_owner: bool,
    state: controller::ChargeLimitState,
    limit: &ChargeLimit,
) -> bool {
    hardware_owner
        && state == controller::ChargeLimitState::Ready
        && limit.configured_percent.is_some()
        && limit.effective_percent.is_some()
}

fn performance_click_allowed(state: &controller::UiState, index: i32) -> bool {
    // Capability status (including backend/permission evidence) is reduced to
    // `perf_writable`; Ready requires a successful current-profile read. No
    // request reaches the worker while any precondition is unknown/blocked.
    (0..=2).contains(&index)
        && state.perf_state == controller::PerformanceHwState::Ready
        && state.perf_writable
        && state.available_perf_mask & (1 << index) != 0
}

fn performance_command_for_click(state: &controller::UiState, index: i32) -> Option<WorkerCommand> {
    if !performance_click_allowed(state, index) {
        return None;
    }
    performance_profile_from_index(index).map(WorkerCommand::SetPerformance)
}

fn charge_mutation_allowed(state: &controller::UiState) -> bool {
    state.charge_limit_writable && state.charge_limit_state == controller::ChargeLimitState::Ready
}

fn gpu_mode_click_allowed(state: &controller::UiState) -> bool {
    state.gpu_mode_writable && state.gpu_mode_state == controller::GpuModeHwState::Ready
}

#[cfg(test)]
fn gpu_mode_card_selected(state: &controller::UiState, index: i32) -> bool {
    state.gpu_mode_state == controller::GpuModeHwState::Ready && state.gpu_selected == index
}

#[cfg(test)]
fn gpu_mode_card_disabled(state: &controller::UiState, _index: i32, mask_bit: i32) -> bool {
    state.gpu_mode_state != controller::GpuModeHwState::Ready
        || !state.gpu_mode_writable
        || state.available_gpu_mask & mask_bit == 0
}

fn gpu_mode_from_index(index: i32) -> Option<u32> {
    // Exact product wire targets for Hardware1.SetProductGpuMode.
    //
    // The ASUS Armoury product API has no `Optimized` mode, so card 3 never
    // issues a product request.
    match index {
        0 => Some(0), // Hybrid
        1 => Some(1), // Integrated
        2 => Some(2), // Ultimate
        _ => None,
    }
}

fn gpu_selected_index(mode: GpuMode) -> i32 {
    match mode {
        GpuMode::Eco => 0,
        GpuMode::Standard => 1,
        GpuMode::Ultimate => 2,
        GpuMode::Optimized => 3,
    }
}

fn charge_limit_from_ui(value: f32) -> Option<u8> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if !(20.0..=100.0).contains(&value) {
        return None;
    }
    u8::try_from(value as i64).ok()
}

fn apply_performance_outcome(state: &mut controller::UiState, outcome: &PerformanceCommandOutcome) {
    state.perf_selected = perf_selected_index(outcome.state.current);
    state.available_perf_mask = performance_available_mask(&outcome.state.available);
}

fn apply_gpu_outcome(state: &mut controller::UiState, outcome: &GpuCommandOutcome) {
    state.gpu_selected = gpu_selected_index(outcome.state.requested);
    state.gpu_ultimate_pending = match &outcome.result {
        ApplyResult::Pending { requirement } => {
            outcome.state.requested == GpuMode::Ultimate
                && *requirement == ActionRequirement::Reboot
        }
        _ => false,
    };

    match &outcome.result {
        ApplyResult::Applied => {
            state.gpu_section_error = false;
            tracing::debug!(
                "gpu: режим применён: requested={:?}, mux={:?}, access={:?}, power={:?}",
                outcome.state.requested,
                outcome.state.mux,
                outcome.state.access_policy,
                outcome.state.power_state,
            );
        }
        ApplyResult::Pending { requirement } => {
            state.gpu_section_error = false;
            tracing::debug!("gpu: результат Pending: requirement={requirement:?}");
        }
        ApplyResult::Failed { .. } | ApplyResult::RolledBack { .. } => {
            state.gpu_section_error = true;
            tracing::warn!("gpu: результат не применился: {:?}", outcome.result);
        }
        ApplyResult::Accepted => {
            // `Accepted` — не hardware-confirmed success; в GPU path он сейчас
            // не ожидается, поэтому трактуем fail-closed: не показываем режим
            // как успешно применённый.
            state.gpu_section_error = true;
            tracing::warn!(
                "gpu: результат Accepted не подтверждает применение режима: {:?}",
                outcome.result
            );
        }
    }
}

fn apply_gpu_result(
    state: &mut controller::UiState,
    result: Result<GpuCommandOutcome, SetGpuModeError>,
) {
    match result {
        Ok(outcome) => apply_gpu_outcome(state, &outcome),
        Err(CommandError::Command(e)) => {
            state.gpu_section_error = true;
            tracing::warn!("gpu: команда не выполнена: {e:?}");
        }
        Err(CommandError::ReadBack { result, source }) => {
            state.gpu_section_error = true;
            tracing::warn!(
                "gpu: команда выполнена ({result:?}), но read-back не удался: {source:?}"
            );
        }
        Err(CommandError::Unconfirmed {
            intent,
            command,
            observation,
        }) => {
            // Итог неизвестен: не показываем ни успех, ни определённый failure.
            // UI сохраняет прежнее состояние до последующего authoritative read-back.
            // Не устанавливаем gpu_section_error — это не definitive failure.
            tracing::warn!(
                "gpu: исход мутации неизвестен (ожидалось: {intent}; команда: {command:?}; read-back: {observation:?})"
            );
        }
    }
}

fn apply_product_gpu_result(
    state: &mut controller::UiState,
    result: Result<ProductGpuMutationResult, ProviderError>,
) {
    // Wire outcome encoding of `Hardware1.SetProductGpuMode` (orbis-hardwared).
    const OUTCOME_ALREADY_ACTIVE: u32 = 0;
    const OUTCOME_REBOOT_REQUIRED: u32 = 1;
    const OUTCOME_INCONSISTENT: u32 = 3;

    match result {
        Ok(reply) => {
            // Apply only authoritative current/queued read-back values; a
            // successful queued result never claims an applied mode and
            // unknown wire sentinels keep previous UI evidence.
            if let Some(index) = controller::asus_product_gpu_index(reply.current_mode) {
                state.gpu_selected = index;
            }
            if let Some(index) = controller::asus_product_gpu_index(reply.queued_mode) {
                state.gpu_queued = index;
            }
            state.gpu_reboot_required = reply.reboot_required;
            match reply.outcome {
                OUTCOME_ALREADY_ACTIVE | OUTCOME_REBOOT_REQUIRED => {
                    state.gpu_mode_writable = true;
                    state.gpu_section_error = false;
                    tracing::debug!(
                        "product gpu: queued state confirmed: requested={}, current={}, queued={}, reboot_required={}",
                        reply.requested_mode,
                        reply.current_mode,
                        reply.queued_mode,
                        reply.reboot_required
                    );
                }
                OUTCOME_INCONSISTENT => {
                    state.gpu_mode_writable = false;
                    state.gpu_section_error = true;
                    tracing::warn!("product gpu: read-back inconsistent: {reply:?}");
                }
                _ => {
                    // Unknown outcome is not a definitive failure and not a
                    // success: UI keeps previous section state.
                    tracing::warn!(
                        "product gpu: outcome unknown; UI keeps previous evidence: {reply:?}"
                    );
                }
            }
        }
        Err(e) => {
            state.gpu_mode_writable = false;
            state.gpu_section_error = true;
            tracing::warn!("product gpu: команда не выполнена: {e:?}");
        }
    }
}

fn apply_charge_limit_outcome(
    state: &mut controller::UiState,
    outcome: &ChargeLimitCommandOutcome,
) {
    match outcome.state.configured_percent {
        Some(percent) => {
            state.charge_limit = i32::from(percent.get());
            tracing::debug!("battery: лимит применён: percent={}", percent.get());
        }
        None => tracing::warn!(
            "battery: authoritative percent отсутствует (None); UI сохраняет прежнее значение"
        ),
    }
    if !matches!(outcome.result, ApplyResult::Applied) {
        tracing::warn!("battery: результат не Applied: {:?}", outcome.result);
    }
}

fn apply_charge_limit_result(
    state: &mut controller::UiState,
    result: Result<ChargeLimitCommandOutcome, SetChargeLimitError>,
) {
    match result {
        Ok(outcome) => apply_charge_limit_outcome(state, &outcome),
        Err(CommandError::Command(e)) => tracing::warn!("battery: команда не выполнена: {e:?}"),
        Err(CommandError::ReadBack { result, source }) => {
            tracing::warn!(
                "battery: команда выполнена ({result:?}), но read-back не удался: {source:?}"
            );
        }
        Err(CommandError::Unconfirmed {
            intent,
            command,
            observation,
        }) => {
            // Итог неизвестен: UI сохраняет прежнее состояние и помечает
            // отсутствие подтверждения; Авторитетный read-back позже обновит.
            tracing::warn!(
                "battery: исход мутации неизвестен (ожидалось: {intent}; команда: {command:?}; read-back: {observation:?})"
            );
        }
    }
}

fn apply_charge_limit_refresh(
    state: &mut controller::UiState,
    result: Result<ChargeLimit, ProviderError>,
) {
    match result {
        Ok(limit) => match limit.configured_percent {
            Some(percent) => {
                state.charge_limit = i32::from(percent.get());
                state.charge_limit_enabled = limit.enabled;
                state.charge_limit_state = controller::ChargeLimitState::Ready;
                state.charge_limit_writable = battery_write_available(
                    state.charge_limit_writable,
                    controller::ChargeLimitState::Ready,
                    &limit,
                );
                tracing::debug!(
                    "battery: refresh OK, percent={}, enabled={}",
                    percent.get(),
                    limit.enabled
                );
            }
            None => {
                state.charge_limit_state = controller::ChargeLimitState::Unavailable;
                state.charge_limit_writable = false;
                tracing::warn!(
                    "battery: refresh OK, но authoritative percent отсутствует (None); не подставляю fixture/default"
                );
            }
        },
        Err(e) => {
            state.charge_limit_state = controller::ChargeLimitState::Unavailable;
            state.charge_limit_writable = false;
            tracing::warn!("battery: refresh недоступен: {e:?}");
        }
    }
}

fn apply_performance_refresh(
    state: &mut controller::UiState,
    result: Result<PerformanceState, ProviderError>,
) {
    match result {
        Ok(s) => {
            state.perf_state = controller::PerformanceHwState::Ready;
            state.perf_selected = perf_selected_index(s.current);
            state.available_perf_mask = performance_available_mask(&s.available);
            tracing::debug!(
                "performance: refresh OK, current={:?}, available={:?}",
                s.current,
                s.available
            );
        }
        Err(e) => {
            state.perf_state = controller::PerformanceHwState::Unavailable;
            tracing::warn!("performance: refresh недоступен: {e:?}");
        }
    }
}

fn apply_gpu_power_refresh(
    state: &mut controller::UiState,
    result: Result<GpuPowerState, ProviderError>,
) {
    match result {
        Ok(v) => {
            state.gpu_power = controller::GpuHwState::Ready;
            state.gpu_power_value = gpu_power_value_to_int(v);
        }
        Err(e) => {
            state.gpu_power = controller::GpuHwState::Unavailable;
            tracing::warn!("gpu-power: refresh недоступен: {e:?}");
        }
    }
}

fn apply_gpu_mux_refresh(
    state: &mut controller::UiState,
    result: Result<GpuMuxState, ProviderError>,
) {
    match result {
        Ok(v) => {
            state.gpu_mux = controller::GpuHwState::Ready;
            state.gpu_mux_value = gpu_mux_value_to_int(v);
        }
        Err(e) => {
            state.gpu_mux = controller::GpuHwState::Unavailable;
            tracing::warn!("gpu-mux: refresh недоступен: {e:?}");
        }
    }
}

fn apply_gpu_access_refresh(
    state: &mut controller::UiState,
    result: Result<GpuAccessPolicy, ProviderError>,
) {
    match result {
        Ok(v) => {
            state.gpu_access = controller::GpuHwState::Ready;
            state.gpu_access_value = gpu_access_value_to_int(v);
        }
        Err(e) => {
            state.gpu_access = controller::GpuHwState::Unavailable;
            tracing::warn!("gpu-access: refresh недоступен: {e:?}");
        }
    }
}

fn gpu_power_value_to_int(v: GpuPowerState) -> i32 {
    match v {
        GpuPowerState::Active => 0,
        GpuPowerState::Suspended => 1,
        GpuPowerState::Off => 2,
        GpuPowerState::Stale => 3,
        GpuPowerState::Unknown => 4,
    }
}

fn gpu_mux_value_to_int(v: GpuMuxState) -> i32 {
    match v {
        GpuMuxState::Integrated => 0,
        GpuMuxState::Discrete => 1,
        GpuMuxState::Unknown => 2,
    }
}

fn gpu_access_value_to_int(v: GpuAccessPolicy) -> i32 {
    match v {
        GpuAccessPolicy::Unblocked => 0,
        GpuAccessPolicy::Blocked => 1,
        GpuAccessPolicy::Pending => 2,
        GpuAccessPolicy::Unknown => 3,
    }
}

fn apply_performance_event(state: &mut controller::UiState, event: WorkerEvent) {
    match event {
        WorkerEvent::Performance(Ok(outcome)) => {
            apply_performance_outcome(state, &outcome);
            match &outcome.result {
                ApplyResult::Applied => tracing::debug!("performance: профиль применён"),
                r => tracing::warn!("performance: результат не Applied: {r:?}"),
            }
        }
        WorkerEvent::Performance(Err(CommandError::Command(e))) => {
            tracing::warn!("performance: команда не выполнена: {e:?}")
        }
        WorkerEvent::Performance(Err(CommandError::ReadBack { result, source })) => {
            tracing::warn!(
                "performance: команда выполнена ({result:?}), но read-back не удался: {source:?}"
            );
        }
        WorkerEvent::Performance(Err(CommandError::Unconfirmed {
            intent,
            command,
            observation,
        })) => {
            // Итог неизвестен: не заявляем ни успеха, ни определённой ошибки;
            // UI сохраняет прежнее состояние до последующего read-back.
            tracing::warn!(
                "performance: исход мутации неизвестен (ожидалось: {intent}; команда: {command:?}; read-back: {observation:?})"
            );
        }
        WorkerEvent::Gpu(result) => apply_gpu_result(state, result),
        WorkerEvent::ProductGpu(result) => apply_product_gpu_result(state, result),
        WorkerEvent::ChargeLimit(result) => apply_charge_limit_result(state, result),
        WorkerEvent::ChargeLimitRefresh(result) => apply_charge_limit_refresh(state, result),
        WorkerEvent::GpuPowerRefresh(result) => apply_gpu_power_refresh(state, result),
        WorkerEvent::GpuMuxRefresh(result) => apply_gpu_mux_refresh(state, result),
        WorkerEvent::GpuAccessRefresh(result) => apply_gpu_access_refresh(state, result),
        WorkerEvent::PerformanceRefresh(result) => apply_performance_refresh(state, result),
        WorkerEvent::RegistryChange(Ok((generation, snapshot))) => {
            tracing::debug!("capability registry refreshed: generation={}", generation);
            state.update_capabilities_at(generation, &snapshot);
        }
        WorkerEvent::RegistryChange(Err(e)) => {
            tracing::warn!("capability registry refresh failed: {e:?}");
        }
        WorkerEvent::TelemetryRefresh(Ok(telemetry)) => state.update_telemetry(&telemetry),
        WorkerEvent::TelemetryRefresh(Err(e)) => {
            // Ошибка read: НЕ затираем последний успешный telemetry state, но
            // помечаем его stale, чтобы UI не показывал старые значения как
            // актуальные.
            tracing::warn!("telemetry refresh failed: {e:?}");
            state.mark_telemetry_stale();
        }
        WorkerEvent::FanCurve(Ok(apply_result)) => match &apply_result {
            ApplyResult::Applied => {
                tracing::debug!("fan curve: mutation applied (read-back confirmed)");
                state.fan_curve_error = false;
                state.fan_curve_dirty = false;
            }
            other => {
                tracing::warn!("fan curve: mutation result not Applied: {other:?}");
                state.fan_curve_error = true;
            }
        },
        WorkerEvent::FanCurve(Err(e)) => {
            tracing::warn!("fan curve mutation failed: {e:?}");
            state.fan_curve_error = true;
        }
        WorkerEvent::FanCurveRefresh {
            profile,
            result,
            writable,
        } => match result {
            Ok(curve) => {
                state.load_fan_curve(&curve, profile);
                state.fan_curve_writable = writable;
            }
            Err(e) => {
                state.fan_curve_writable = false;
                tracing::warn!("fan curve refresh failed: {e:?}");
                state.fan_curve_state = controller::FanCurveHwState::Unavailable;
                state.fan_curve_error = true;
            }
        },
        WorkerEvent::FanCurveDefaults { profile, result } => match result {
            Ok(ApplyResult::Accepted) => {
                tracing::debug!(
                    "fan factory reset accepted for profile={profile:?}; confirmation of platform defaults unavailable"
                );
                // Command accepted but observed state not independently verified as platform defaults.
                // Do not set fan_curve_error (not a definitive failure).
                // Do not clear fan_curve_dirty (user may want to apply custom curve after reset).
                // Refresh is enqueued in handle_worker_event.
            }
            Ok(other) => {
                tracing::warn!("fan factory reset returned unexpected result: {other:?}");
                state.fan_curve_error = true;
            }
            Err(CommandError::Unconfirmed {
                intent,
                command,
                observation,
            }) => {
                // Unconfirmed: mutation may have dispatched but outcome unknown.
                // Do NOT set fan_curve_error (not a definitive failure).
                // Preserve UI state, log context for diagnostics.
                tracing::warn!(
                    "fan factory reset unconfirmed (intent={intent}; command={command:?}; observation={observation:?})"
                );
            }
            Err(e) => {
                // Definitive error (Unsupported, PermissionDenied, etc.)
                // Do NOT set fan_curve_error - preserve UI state like other mutations.
                tracing::warn!("fan factory reset failed definitively: {e:?}");
            }
        },
    }
}

fn fan_reset_available(snapshot: &orbis_capabilities::CapabilityRegistrySnapshot) -> bool {
    use orbis_core::capability::{CapabilityStatus, FeatureId};
    snapshot
        .capability(FeatureId::FanCurves)
        .map(|cap| {
            matches!(
                cap.operations.write.status,
                CapabilityStatus::Supported | CapabilityStatus::SupportedWithRequirement
            )
        })
        .unwrap_or(false)
}

fn handle_worker_event(
    app: &AppWindow,
    event: WorkerEvent,
    worker_tx: &UnboundedSender<WorkerCommand>,
) {
    // Check if this is a factory reset accepted event and extract profile for refresh.
    let refresh_fan_curve = match &event {
        WorkerEvent::FanCurveDefaults {
            profile,
            result: Ok(ApplyResult::Accepted),
        } => Some(*profile),
        _ => None,
    };

    let refresh_quick_controls = matches!(&event, WorkerEvent::TelemetryRefresh(_));
    if let WorkerEvent::RegistryChange(Ok((_generation, snapshot))) = &event {
        diagnostics_backend::replace_capabilities(snapshot.clone());
    }
    let mut s = from_slint(&app.get_ui_state());
    if let WorkerEvent::RegistryChange(Ok((_generation, snapshot))) = &event {
        app.set_factory_reset_available(fan_reset_available(snapshot));
    }
    apply_performance_event(&mut s, event);
    app.set_ui_state(to_slint(&s));
    if refresh_quick_controls {
        quick_controls_backend::refresh_if_due(app, Duration::from_secs(10));
    }
    // After factory reset, enqueue a refresh for the selected fan to load observed state.
    if let Some(profile) = refresh_fan_curve {
        if let Some(fan_id) = controller::UiState::fan_id_from_index(s.fan_selected) {
            if let Err(e) = worker_tx.send(WorkerCommand::RefreshFanCurve {
                profile,
                fan: fan_id,
            }) {
                tracing::warn!("fan factory reset refresh enqueue failed: {e:?}");
            }
        }
    }
}

fn wire_callbacks(app: &AppWindow, worker_tx: Option<UnboundedSender<WorkerCommand>>) {
    let app_weak = app.as_weak();
    {
        let worker_tx = worker_tx.clone();
        let app_weak = app.as_weak();
        app.on_perf_clicked(move |i| {
            let Some(app) = app_weak.upgrade() else {
                tracing::warn!("perf-clicked после уничтожения окна: {i}");
                return;
            };
            let state = from_slint(&app.get_ui_state());
            let Some(command) = performance_command_for_click(&state, i) else {
                tracing::warn!("perf-clicked игнорирован: Performance недоступен/read-only: {i}");
                return;
            };
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(command) {
                        tracing::warn!("worker закрыт, команда не отправлена: {e:?}");
                    }
                }
                None => {
                    tracing::warn!("perf-clicked вне интерактивного режима (worker отсутствует)")
                }
            }
        });
    }
    {
        let worker_tx = worker_tx.clone();
        app.on_gpu_clicked(move |i| {
            let Some(raw) = gpu_mode_from_index(i) else {
                tracing::warn!("gpu-clicked с неизвестным/непродуктовым индексом: {i}");
                return;
            };
            if let Some(app) = app_weak.upgrade() {
                let s = from_slint(&app.get_ui_state());
                if !gpu_mode_click_allowed(&s) {
                    tracing::warn!(
                        "gpu-clicked игнорирован: product GPU mode недоступен/read-only"
                    );
                    return;
                }
            }
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(WorkerCommand::SetProductGpuMode { raw }) {
                        tracing::warn!("worker закрыт, команда не отправлена: {e:?}");
                    }
                }
                None => {
                    tracing::warn!("gpu-clicked вне интерактивного режима (worker отсутствует)")
                }
            }
        });
    }
    {
        let worker_tx = worker_tx.clone();
        let app_weak = app.as_weak();
        app.on_charge_changed(move |v| {
            let Some(percent) = charge_limit_from_ui(v) else {
                tracing::warn!("charge-changed с недопустимым значением: {v}");
                return;
            };
            if let Some(app) = app_weak.upgrade() {
                let s = from_slint(&app.get_ui_state());
                if !charge_mutation_allowed(&s) {
                    tracing::warn!("charge-changed игнорирован: ChargeLimit недоступен/read-only");
                    return;
                }
            }
            match &worker_tx {
                Some(tx) => {
                    tracing::debug!(requested_percent = percent, "battery GUI commit");
                    if let Err(e) = tx.send(WorkerCommand::SetChargeLimit { percent }) {
                        tracing::warn!("worker закрыт, команда не отправлена: {e:?}");
                    }
                }
                None => {
                    tracing::warn!("charge-changed вне интерактивного режима (worker отсутствует)")
                }
            }
        });
    }
    {
        let worker_tx = worker_tx.clone();
        let app_weak = app.as_weak();
        app.on_fan_changed(move |i| {
            let Some(app) = app_weak.upgrade() else {
                tracing::warn!("fan-changed после уничтожения окна: {i}");
                return;
            };
            let mut s = from_slint(&app.get_ui_state());
            let Some(fan_id) = controller::UiState::fan_id_from_index(i) else {
                tracing::warn!("fan-changed с неизвестным индексом: {i}");
                return;
            };
            let Some(profile) =
                controller::UiState::asusd_profile_from_index(s.fan_profile_selected)
            else {
                tracing::warn!(
                    "fan-changed: invalid profile index {}",
                    s.fan_profile_selected
                );
                return;
            };
            s.fan_selected = i;
            s.fan_curve_state = controller::FanCurveHwState::Loading;
            s.fan_curve_writable = false;
            s.fan_curve_error = false;
            app.set_ui_state(to_slint(&s));
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(WorkerCommand::RefreshFanCurve {
                        profile,
                        fan: fan_id,
                    }) {
                        tracing::warn!("worker закрыт, fan refresh не отправлен: {e:?}");
                    }
                }
                None => tracing::warn!("fan-changed вне интерактивного режима"),
            }
        });
    }
    {
        let worker_tx = worker_tx.clone();
        let app_weak = app.as_weak();
        app.on_fan_profile_changed(move |i| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            let mut s = from_slint(&app.get_ui_state());
            let Some(profile) = controller::UiState::asusd_profile_from_index(i) else {
                tracing::warn!("fan-profile-changed: invalid profile index {i}");
                return;
            };
            let Some(fan_id) = controller::UiState::fan_id_from_index(s.fan_selected) else {
                tracing::warn!("fan-profile-changed: invalid fan index {}", s.fan_selected);
                return;
            };
            s.fan_profile_selected = i;
            s.fan_curve_state = controller::FanCurveHwState::Loading;
            s.fan_curve_writable = false;
            s.fan_curve_error = false;
            app.set_ui_state(to_slint(&s));
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(WorkerCommand::RefreshFanCurve {
                        profile,
                        fan: fan_id,
                    }) {
                        tracing::warn!("worker закрыт, fan profile refresh не отправлен: {e:?}");
                    }
                }
                None => tracing::warn!("fan-profile-changed вне интерактивного режима"),
            }
        });
    }
    {
        let app_weak = app.as_weak();
        app.on_fan_temp_point_changed(move |index, value| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            let mut s = from_slint(&app.get_ui_state());
            if (0..8).contains(&index) {
                s.fan_curve_temps[index as usize] = value;
                s.fan_curve_dirty = true;
            }
            app.set_ui_state(to_slint(&s));
        });
    }
    {
        let app_weak = app.as_weak();
        app.on_fan_pwm_point_changed(move |index, value| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            let mut s = from_slint(&app.get_ui_state());
            if (0..8).contains(&index) {
                s.fan_curve_pwms[index as usize] = value;
                s.fan_curve_dirty = true;
            }
            app.set_ui_state(to_slint(&s));
        });
    }
    {
        let worker_tx = worker_tx.clone();
        let app_weak = app.as_weak();
        app.on_fan_apply_clicked(move |reset_defaults| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            let s = from_slint(&app.get_ui_state());

            if reset_defaults {
                if s.fan_curve_state != controller::FanCurveHwState::Ready
                    || !s.fan_curve_writable
                    || s.fan_curve_error
                {
                    tracing::warn!("fan factory reset rejected: unavailable/read-only/error");
                    return;
                }
                let Some(profile) =
                    controller::UiState::asusd_profile_from_index(s.fan_profile_selected)
                else {
                    tracing::warn!(
                        "fan factory reset: invalid profile index {}",
                        s.fan_profile_selected
                    );
                    return;
                };
                // Requested profile is captured at command creation time and does not
                // depend on subsequent UI selection.
                if let Some(tx) = &worker_tx {
                    if let Err(e) = tx.send(WorkerCommand::ResetFanCurvesToDefaults { profile }) {
                        tracing::warn!("worker closed, fan factory reset not sent: {e:?}");
                    }
                } else {
                    tracing::warn!("fan factory reset unavailable outside interactive mode");
                }
                return;
            }

            if !s.fan_curve_can_mutate() {
                tracing::warn!(
                    "fan-apply rejected: not writable, not dirty, error, or invalid curve"
                );
                return;
            }
            let Some(profile) =
                controller::UiState::asusd_profile_from_index(s.fan_profile_selected)
            else {
                tracing::warn!(
                    "fan-apply: invalid profile index {}",
                    s.fan_profile_selected
                );
                return;
            };
            let Some(fan_id) = controller::UiState::fan_id_from_index(s.fan_selected) else {
                tracing::warn!("fan-apply: invalid fan index {}", s.fan_selected);
                return;
            };
            let Some(curve) = s.build_fan_curve_points() else {
                tracing::warn!("fan-apply: failed to build FanCurvePoints from editor state");
                return;
            };
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(WorkerCommand::SetFanCurve {
                        profile,
                        fan: fan_id,
                        curve,
                    }) {
                        tracing::warn!("worker закрыт, fan mutation не отправлена: {e:?}");
                    }
                }
                None => tracing::warn!("fan-apply вне интерактивного режима"),
            }
        });
    }

    // Frameless title bar (spec §3): drag via the winit accessor, minimize via
    // the window handle, close via the shared close-action decision path.
    {
        let app_weak = app.as_weak();
        app.on_titlebar_minimize_requested(move || {
            if let Some(app) = app_weak.upgrade() {
                app.window().set_minimized(true);
            }
        });
    }
    {
        let app_weak = app.as_weak();
        app.on_titlebar_maximize_requested(move || {
            if let Some(app) = app_weak.upgrade() {
                let window = app.window();
                let next = !window.is_maximized();
                window.set_maximized(next);
                app.set_maximized(next);
            }
        });
    }
    {
        let app_weak = app.as_weak();
        app.on_titlebar_close_requested(move || {
            if let Some(app) = app_weak.upgrade() {
                quick_controls_backend::handle_close_request(&app);
            }
        });
    }
    {
        let app_weak = app.as_weak();
        app.on_titlebar_drag_started(move || {
            if let Some(app) = app_weak.upgrade() {
                let _ = app.window().with_winit_window(|winit_window| {
                    let _ = winit_window.drag_window();
                });
            }
        });
    }
}

/// Wire the settings section: preferences persistence handlers plus the
/// diagnostics actions (same runtime paths as the former windows, spec §2.4).
fn wire_settings_section(app: &AppWindow) {
    {
        let app_weak = app.as_weak();
        app.on_theme_changed(move |light| {
            if let Some(app) = app_weak.upgrade() {
                apply_theme_to_all(&app, light);
            }
            if let Err(error) = persist_theme(light) {
                tracing::warn!(
                    error = %error,
                    "theme preference save failed; runtime theme remains active"
                );
            }
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_startup_changed(move |enabled| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            app.set_startup_enabled(false);
            app.set_startup_status("Applying…".into());
            match preferences_backend::set_autostart(enabled) {
                Ok(state) => {
                    apply_autostart_state_to_window(&app, &state);
                    app.set_settings_local_status(
                        if state.enabled {
                            "Autostart enabled"
                        } else {
                            "Autostart disabled"
                        }
                        .into(),
                    );
                }
                Err(error) => {
                    tracing::warn!(error = %error, "autostart change failed");
                    apply_preferences_state(&app);
                    app.set_settings_local_status("Autostart change failed".into());
                }
            }
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_start_minimized_changed(move |enabled| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            app.set_start_minimized_enabled(false);
            match preferences_backend::persist_start_minimized(enabled) {
                Ok(preferences) => {
                    app.set_start_minimized(preferences.window.start_minimized);
                    app.set_start_minimized_enabled(true);
                    app.set_settings_local_status("Saved · applies on next launch".into());
                }
                Err(error) => {
                    tracing::warn!(error = %error, "start-minimized preference save failed");
                    apply_preferences_state(&app);
                    app.set_settings_local_status("Could not save Start Minimized".into());
                }
            }
        });
    }

    quick_controls_backend::wire_position_preferences_bridge(app);
    diagnostics_backend::wire_window(app);
    apply_preferences_state(app);
    diagnostics_backend::refresh(app);
}

// ---------------------------------------------------------------------------
// Детерминированный оффскрин-рендер (SoftwareRenderer)

// ---------------------------------------------------------------------------
// Детерминированный оффскрин-рендер (SoftwareRenderer)
// ---------------------------------------------------------------------------

struct SoftwareWindowAdapter {
    renderer: Rc<slint::platform::software_renderer::SoftwareRenderer>,
    window: OnceCell<slint::Window>,
    size: Cell<PhysicalSize>,
}

impl WindowAdapter for SoftwareWindowAdapter {
    fn window(&self) -> &slint::Window {
        self.window.get().expect("window set")
    }

    fn renderer(&self) -> &dyn Renderer {
        self.renderer.as_ref()
    }

    fn size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn set_size(&self, size: WindowSize) {
        let (logical, phys) = match size {
            WindowSize::Physical(p) => (p.to_logical(1.0), p),
            WindowSize::Logical(l) => (l, l.to_physical(1.0)),
        };
        self.size.set(phys);
        self.window()
            .dispatch_event(WindowEvent::Resized { size: logical });
    }

    fn set_visible(&self, _visible: bool) -> Result<(), PlatformError> {
        Ok(())
    }
}

struct SoftwarePlatform {
    adapter: Rc<SoftwareWindowAdapter>,
}

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.adapter.clone())
    }
}

fn render_screenshot(
    state: &controller::UiState,
    path: &str,
    ui_section: Option<&str>,
) -> anyhow::Result<()> {
    let (width, height) = (1200u32, 800u32);
    let renderer = Rc::new(slint::platform::software_renderer::SoftwareRenderer::new());
    let adapter = Rc::new(SoftwareWindowAdapter {
        renderer: renderer.clone(),
        window: OnceCell::new(),
        size: Cell::new(PhysicalSize::new(width, height)),
    });
    {
        let dyn_adapter: Rc<dyn WindowAdapter> = adapter.clone();
        let weak: std::rc::Weak<dyn WindowAdapter> = Rc::downgrade(&dyn_adapter);
        let window = slint::Window::new(weak);
        adapter.window.set(window).ok();
    }
    slint::platform::set_platform(Box::new(SoftwarePlatform { adapter })).expect("platform once");

    let app = build_app(state, None)?;
    match ui_section {
        Some("performance") => app.set_active_section(Section::Performance),
        Some("power") => app.set_active_section(Section::Power),
        Some("fans") | Some("cooling") => app.set_active_section(Section::Cooling),
        Some("graphics") => app.set_active_section(Section::Graphics),
        Some("backlight") => app.set_active_section(Section::Backlight),
        Some("display") => app.set_active_section(Section::Display),
        Some("extra") | Some("system") => app.set_active_section(Section::System),
        Some("settings") => app.set_active_section(Section::Settings),
        Some("about") => app.set_active_section(Section::About),
        _ => app.set_active_section(Section::Dashboard),
    }
    app.window()
        .set_size(LogicalSize::new(width as f32, height as f32));
    app.show()?;

    let size = app.window().size();
    let (w, h) = (size.width as usize, size.height as usize);
    let mut buf: Vec<Rgb8Pixel> = vec![Rgb8Pixel::new(0, 0, 0); w * h];
    renderer.render(&mut buf, w);

    let mut raw = Vec::with_capacity(w * h * 3);
    for p in &buf {
        raw.extend_from_slice(&[p.r, p.g, p.b]);
    }
    image::save_buffer(path, &raw, w as u32, h as u32, image::ColorType::Rgb8)?;
    eprintln!("orbis-control: сохранён скриншот {path} ({w}x{h})");
    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let launch_mode = if args.screenshot.is_some() {
        launch_context::LaunchMode::Screenshot
    } else {
        launch_context::LaunchMode::Interactive
    };
    launch_context::LaunchContext::new(launch_mode, effective_uid()).validate()?;
    let mut state = if args.screenshot.is_some() {
        scenario_state(&args.ui_state)?
    } else {
        controller::UiState::production_initial()
    };

    if let Some(path) = args.screenshot {
        return render_screenshot(&state, &path, args.ui_section.as_deref());
    }

    init_tracing();
    let startup_preferences = initialize_runtime_preferences();
    let runtime = tokio::runtime::Runtime::new()?;
    quick_controls_backend::initialize(runtime.handle().clone());

    let session_connection = runtime
        .block_on(zbus::Connection::session())
        .map_err(|e| anyhow::anyhow!("не удалось подключиться к session bus: {e}"))?;
    let system_connection = runtime
        .block_on(zbus::Connection::system())
        .map_err(|e| anyhow::anyhow!("не удалось подключиться к system bus: {e}"))?;
    let diagnostics_session_connection = session_connection.clone();
    let diagnostics_system_connection = system_connection.clone();
    // Original application caller identity for the ASUS product GPU Hardware1
    // operation: the GUI owns this connection and passes it through the worker
    // FIFO. The operation stays fail-closed until polkit/backend promotion.
    let product_gpu_source: std::sync::Arc<dyn HardwareProductGpuSource> =
        std::sync::Arc::new(ZbusHardwareProductGpuSource::new(system_connection.clone()));
    let (application_runtime, _hardware_owner) = runtime.block_on(build_production_runtime(
        session_connection,
        system_connection,
    ))?;
    state.perf_writable = false;
    state.charge_limit_writable = false;

    let poll_interval = application_runtime.telemetry.poll_interval();
    diagnostics_backend::initialize(
        runtime.handle().clone(),
        diagnostics_session_connection,
        diagnostics_system_connection,
        application_runtime.capabilities_arc(),
        poll_interval.saturating_mul(3),
    );

    let (worker_tx, worker_rx) = orbis_ui::worker::command_channel();

    let app = build_app(&state, Some(worker_tx.clone()))?;
    quick_controls_backend::force_refresh(&app);
    wire_settings_section(&app);
    app.window().set_size(LogicalSize::new(760.0, 600.0));
    apply_start_minimized(startup_preferences.start_minimized, |minimized| {
        app.window().set_minimized(minimized);
    });

    let weak = app.as_weak();
    let worker_tx_for_event = worker_tx.clone();
    let event_sink = move |event: WorkerEvent| {
        let weak = weak.clone();
        let worker_tx_clone = worker_tx_for_event.clone();
        if let Err(e) = weak.upgrade_in_event_loop(move |app| {
            handle_worker_event(&app, event, &worker_tx_clone);
        }) {
            tracing::warn!("не удалось вернуть событие worker-а в event loop: {e:?}");
        }
    };
    runtime.spawn(run_worker_with_product_gpu(
        application_runtime,
        worker_rx,
        event_sink,
        Some(poll_interval),
        Some(product_gpu_source),
    ));

    if let Err(e) = worker_tx.send(WorkerCommand::RefreshChargeLimit) {
        tracing::warn!("worker закрыт, initial battery refresh не отправлен: {e:?}");
    }
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshGpuCapabilities) {
        tracing::warn!("worker закрыт, initial gpu capabilities refresh не отправлен: {e:?}");
    }
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshPerformance) {
        tracing::warn!("worker закрыт, initial performance refresh не отправлен: {e:?}");
    }
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshCapabilities) {
        tracing::warn!("worker закрыт, initial capability refresh не отправлен: {e:?}");
    }
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshTelemetry) {
        tracing::warn!("worker закрыт, initial telemetry refresh не отправлен: {e:?}");
    }
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshFanCurve {
        profile: orbis_core::profile::AsusdFanProfile::Balanced,
        fan: orbis_core::fan::FanId::Cpu,
    }) {
        tracing::warn!("worker закрыт, initial fan curve refresh не отправлен: {e:?}");
    }

    app.show()?;
    slint::run_event_loop()?;

    PREVIEW_DIALOG_WINDOW.with(|slot| *slot.borrow_mut() = None);
    diagnostics_backend::clear();
    quick_controls_backend::clear();
    drop(app);
    drop(worker_tx);
    drop(runtime);
    Ok(())
}

#[cfg(test)]
mod main_tests;
