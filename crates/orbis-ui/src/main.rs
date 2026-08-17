//! Orbis Control — минимальный визуальный прототип главного окна.
//!
//! Режимы запуска:
//!   orbis-control [--ui-state default|pending|disabled|error]
//!   orbis-control --ui-state default --screenshot <path.png>
//!
//! `--ui-state` меняет только локальное состояние интерфейса; без `--screenshot`
//! окно запускается через штатный winit-бэкенд. Скриншоты рендерятся
//! детерминированно через `slint::platform` + SoftwareRenderer (масштаб 100%,
//! без окна, без новых зависимостей).

// UiAction::Perf, UiAction::Gpu и UiAction::Charge больше не конструируются в
// production: Performance, GPU Mode и Battery Charge Limit идут через worker.
// Контроллер сохраняется как boundary/model helper для legacy unit tests.
#[allow(dead_code)]
mod controller;

use std::cell::{Cell, OnceCell};
use std::rc::Rc;

use orbis_application::{
    ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome, PerformanceCommandOutcome,
    PerformanceState, SetChargeLimitError, SetGpuModeError,
};
use orbis_core::action::{ActionRequirement, ApplyResult};
use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_ui::composition::build_production_runtime;
use orbis_ui::worker::{WorkerCommand, WorkerEvent, run_worker_with_polling};
use slint::platform::{Platform, PlatformError, Renderer, WindowAdapter, WindowEvent};
use slint::{LogicalSize, PhysicalSize, Rgb8Pixel, WindowSize};
use tokio::sync::mpsc::UnboundedSender;

slint::include_modules!();

/// Разобранные аргументы командной строки.
struct Args {
    ui_state: String,
    screenshot: Option<String>,
}

/// Парсинг аргументов через `std::env::args` (без clap).
fn parse_args() -> Args {
    let mut ui_state = "default".to_string();
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
            other => {
                eprintln!("orbis-control: игнорирую неизвестный аргумент '{other}'");
            }
        }
    }
    Args {
        ui_state,
        screenshot,
    }
}

/// Начальное состояние для сценария `--ui-state`.
fn state_for_scenario(name: &str) -> controller::UiState {
    let mut s = controller::UiState::from_mock_profile("zephyrus-full");
    match name {
        "default" => {}
        "pending" => {
            s.gpu_selected = 2; // Ultimate
            s.gpu_ultimate_pending = true;
        }
        "disabled" => {
            s.gpu_ultimate_disabled = true; // MUX unavailable
        }
        "error" => {
            s.gpu_section_error = true;
        }
        other => {
            eprintln!("orbis-control: неизвестное состояние '{other}', использую default");
        }
    }
    s
}

/// Высота окна: в состоянии error добавляется баннер GPU-ошибки; в
/// Performance- и GPU-секциях статус-строки Loading/Unavailable (+30px каждая).
fn window_height(state: &controller::UiState) -> f32 {
    // высота клиентской области без внутреннего titlebar (34px удалены)
    if state.gpu_section_error {
        466.0
    } else {
        441.0
    }
}

// ---------------------------------------------------------------------------
// Маппинг controller::UiState <-> сгенерированный Slint UiState
// ---------------------------------------------------------------------------

fn to_slint(state: &controller::UiState) -> UiState {
    UiState {
        perf_selected: state.perf_selected,
        available_perf_mask: state.available_perf_mask,
        perf_state: match state.perf_state {
            controller::PerformanceHwState::Loading => PerformanceHwState::Loading,
            controller::PerformanceHwState::Ready => PerformanceHwState::Ready,
            controller::PerformanceHwState::Unavailable => PerformanceHwState::Unavailable,
        },
        perf_writable: state.perf_writable,
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
        charge_limit: state.charge_limit,
        charge_limit_enabled: state.charge_limit_enabled,
        charge_limit_writable: state.charge_limit_writable,
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
        power_ac_mw: state.power_ac.clone().into(),
        battery_health: state.battery_health.clone().into(),
        battery_cycles: state.battery_cycles.clone().into(),
        battery_status: state.battery_status.clone().into(),
        ac_online: state.ac_online.clone().into(),
        gpu_power: state.gpu_power_display.clone().into(),
        version: state.version.clone().into(),
        mock_profile: state.mock_profile.clone().into(),
    }
}

fn from_slint(state: &UiState) -> controller::UiState {
    controller::UiState {
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
        power_ac: state.power_ac_mw.to_string(),
        version: state.version.to_string(),
        mock_profile: state.mock_profile.to_string(),
        perf_capability: controller::CapabilityAvailability::Unknown,
        charge_limit_capability: controller::CapabilityAvailability::Unknown,
        gpu_power_capability: controller::CapabilityAvailability::Unknown,
        gpu_mux_capability: controller::CapabilityAvailability::Unknown,
        gpu_access_capability: controller::CapabilityAvailability::Unknown,
        battery_health: state.battery_health.to_string(),
        battery_cycles: state.battery_cycles.to_string(),
        battery_status: state.battery_status.to_string(),
        ac_online: state.ac_online.to_string(),
        gpu_power_display: state.gpu_power.to_string(),
    }
}

/// Создать окно, установить состояние и подключить обработчики.
fn build_app(
    state: &controller::UiState,
    worker_tx: Option<UnboundedSender<WorkerCommand>>,
) -> Result<AppWindow, slint::PlatformError> {
    let app = AppWindow::new()?;
    app.set_ui_state(to_slint(state));
    wire_callbacks(&app, worker_tx);
    Ok(app)
}

/// UI-boundary: преобразование UI-индекса карточки в доменный профиль.
///
/// 0 -> Silent, 1 -> Balanced, 2 -> Turbo; любое другое значение -> None.
fn performance_profile_from_index(index: i32) -> Option<PerformanceProfile> {
    match index {
        0 => Some(PerformanceProfile::Silent),
        1 => Some(PerformanceProfile::Balanced),
        2 => Some(PerformanceProfile::Turbo),
        _ => None,
    }
}

/// UI-boundary: индекс выбранной карточки из доменного профиля.
fn perf_selected_index(profile: PerformanceProfile) -> i32 {
    match profile {
        PerformanceProfile::Silent => 0,
        PerformanceProfile::Balanced => 1,
        PerformanceProfile::Turbo => 2,
    }
}

/// UI-boundary: существующая performance availability mask из списка доступных
/// профилей. Маска строится по значениям enum, порядок списка не важен.
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

/// Battery mutation requires the existing Hardware1 owner capability plus a
/// Ready read with both configured and effective values.
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

/// Rust-side guard matching the Slint Performance card `disabled` bindings.
/// Invalid indices, unavailable profiles, non-ready reads and absent write
/// capability must not reach the worker.
fn performance_click_allowed(state: &controller::UiState, index: i32) -> bool {
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

/// Rust-side guard for the Battery Charge Limit slider: mutation is allowed
/// only when the write capability is present AND the authoritative read is
/// ready. Mirrors the Slint `disabled` binding for the charge slider.
fn charge_mutation_allowed(state: &controller::UiState) -> bool {
    state.charge_limit_writable && state.charge_limit_state == controller::ChargeLimitState::Ready
}

/// Разрешён ли клик по product GPU Mode карточке (приводит ли он к
/// `SetGpuMode` в worker).
///
/// Production: `gpu_mode_state != Ready` или `!gpu_mode_writable` → false:
/// клик не должен приводить к mutation при отсутствии доказанного product-mode
/// backend. Mock/offscreen — true.
fn gpu_mode_click_allowed(state: &controller::UiState) -> bool {
    state.gpu_mode_writable && state.gpu_mode_state == controller::GpuModeHwState::Ready
}

/// Выделена ли product GPU Mode карточка как authoritative selected.
///
/// Только при `GpuModeHwState::Ready` (production Unavailable → никакой mode
/// не выглядит selected). Rust-спецификация slint binding `selected`.
#[cfg(test)]
fn gpu_mode_card_selected(state: &controller::UiState, index: i32) -> bool {
    state.gpu_mode_state == controller::GpuModeHwState::Ready && state.gpu_selected == index
}

/// Доступна ли product GPU Mode карточка (не disabled).
///
/// Rust-спецификация slint binding `disabled`: клик разрешён только при Ready
/// + writable + наличие бита в маске (`mask_bit` = 1, 2, 4, 8).
#[cfg(test)]
fn gpu_mode_card_disabled(state: &controller::UiState, _index: i32, mask_bit: i32) -> bool {
    state.gpu_mode_state != controller::GpuModeHwState::Ready
        || !state.gpu_mode_writable
        || state.available_gpu_mask & mask_bit == 0
}

/// UI-boundary: преобразование UI-индекса карточки GPU в доменный режим.
///
/// 0 -> Eco, 1 -> Standard, 2 -> Ultimate, 3 -> Optimized; любое другое
/// значение -> None.
fn gpu_mode_from_index(index: i32) -> Option<GpuMode> {
    match index {
        0 => Some(GpuMode::Eco),
        1 => Some(GpuMode::Standard),
        2 => Some(GpuMode::Ultimate),
        3 => Some(GpuMode::Optimized),
        _ => None,
    }
}

/// UI-boundary: индекс выбранной карточки GPU из доменного режима
/// (обратный `gpu_mode_from_index`).
fn gpu_selected_index(mode: GpuMode) -> i32 {
    match mode {
        GpuMode::Eco => 0,
        GpuMode::Standard => 1,
        GpuMode::Ultimate => 2,
        GpuMode::Optimized => 3,
    }
}

/// UI-boundary: преобразование Slint slider value в доменный `u8` percent.
///
/// Callback приходит как float. Допускаются только конечные целые значения в
/// диапазоне 20..=100. Шаг равен 1, поэтому дополнительных modulo checks нет.
/// NaN/infinity/дробные/вне диапазона -> None.
fn charge_limit_from_ui(value: f32) -> Option<u8> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if !(20.0..=100.0).contains(&value) {
        return None;
    }
    // Значение целое и в диапазоне: преобразование в u8 безопасно.
    u8::try_from(value as i64).ok()
}

/// Применить authoritative Performance-результат к UI-состоянию.
///
/// Обновляются только Performance-поля (selected и маска доступности);
/// GPU/Battery/pending/error поля сохраняются без изменений.
fn apply_performance_outcome(state: &mut controller::UiState, outcome: &PerformanceCommandOutcome) {
    state.perf_selected = perf_selected_index(outcome.state.current);
    state.available_perf_mask = performance_available_mask(&outcome.state.available);
}

/// Применить authoritative GPU-результат к UI-состоянию.
///
/// Обновляются только GPU-поля: selected (из outcome.state.requested),
/// Ultimate-pending indicator (только активный Ultimate/Reboot pending) и
/// error banner. Маска доступности и disabled-флаг сохраняются (GpuState не
/// содержит available modes); Performance/Battery поля не изменяются.
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
    }
}

/// Применить полный GPU-результат worker-а к UI-состоянию.
///
/// Ok -> authoritative state; Command error -> selected/pending/mask/disabled
/// сохраняются, выставляется существующий GPU error banner; ReadBack error ->
/// mutation могла выполниться, но authoritative read-back не получен: selected
/// не меняется, error banner выставляется.
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
    }
}

/// Применить authoritative Battery outcome к UI-состоянию.
///
/// Обновляется только `charge_limit` (из `outcome.state.configured_percent`, если он
/// присутствует); Performance/GPU и остальные поля сохраняются. При
/// `percent == None` прежнее UI-значение сохраняется, пишется warning.
fn apply_charge_limit_outcome(
    state: &mut controller::UiState,
    outcome: &ChargeLimitCommandOutcome,
) {
    match outcome.state.configured_percent {
        Some(percent) => {
            state.charge_limit = i32::from(percent.get());
            tracing::debug!("battery: лимит применён: percent={}", percent.get());
        }
        None => {
            tracing::warn!(
                "battery: authoritative percent отсутствует (None); UI сохраняет прежнее значение"
            );
        }
    }
    if !matches!(outcome.result, ApplyResult::Applied) {
        tracing::warn!("battery: результат не Applied: {:?}", outcome.result);
    }
}

/// Применить полный Battery-результат worker-а к UI-состоянию.
///
/// Ok -> authoritative percent применяется (независимо от варианта ApplyResult);
/// Command error -> UiState полностью сохраняется, точный ProviderError в
/// tracing; ReadBack error -> mutation могла выполниться, но authoritative
/// read-back отсутствует: UiState полностью сохраняется.
fn apply_charge_limit_result(
    state: &mut controller::UiState,
    result: Result<ChargeLimitCommandOutcome, SetChargeLimitError>,
) {
    match result {
        Ok(outcome) => apply_charge_limit_outcome(state, &outcome),
        Err(CommandError::Command(e)) => {
            tracing::warn!("battery: команда не выполнена: {e:?}");
        }
        Err(CommandError::ReadBack { result, source }) => {
            tracing::warn!(
                "battery: команда выполнена ({result:?}), но read-back не удался: {source:?}"
            );
        }
    }
}

/// Применить результат authoritative read-only refresh к UI-состоянию.
///
/// Ok + percent=Some(value): status=Ready, charge_limit=фактический percent,
/// charge_limit_enabled=фактический enabled.
/// Ok + percent=None: status=Unavailable (не подставлять fixture/default).
/// Err(ProviderError): status=Unavailable, точная ошибка в tracing.
///
/// Во всех случаях не выдавать числовое значение как authoritative, пока
/// status != Ready.
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

/// Применить результат read-only Performance Mode refresh.
///
/// Ok(state) → Ready + authoritative current/available (selected + маска из
/// state, не из mock fixture). Err → Unavailable (без mock fallback; fake
/// current не показывается).
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

/// Применить результат read-only GPU capability refresh.
///
/// Ok(value) → Ready + semantic value (domain `Unknown` — валидный Ready).
/// Err → Unavailable (backend/read недоступен; без mock fallback).
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

/// Wire-совместимые числовые представления (те же, что в session protocol).
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

/// Применить событие worker-а к UI-состоянию.
///
/// Performance: Ok -> authoritative state; Err (Command/ReadBack) -> UiState не
/// изменяется, ошибка сохраняется в диагностическом журнале.
/// GPU: Ok -> authoritative GPU state; Err -> error banner + tracing, selected
/// не меняется (authoritative state отсутствует).
/// Battery: Ok -> authoritative percent применяется; Err -> tracing, UiState не
/// меняется.
fn apply_performance_event(state: &mut controller::UiState, event: WorkerEvent) {
    match event {
        WorkerEvent::Performance(Ok(outcome)) => {
            apply_performance_outcome(state, &outcome);
            match &outcome.result {
                ApplyResult::Applied => {
                    tracing::debug!("performance: профиль применён");
                }
                r => {
                    tracing::warn!("performance: результат не Applied: {r:?}");
                }
            }
        }
        WorkerEvent::Performance(Err(CommandError::Command(e))) => {
            tracing::warn!("performance: команда не выполнена: {e:?}");
        }
        WorkerEvent::Performance(Err(CommandError::ReadBack { result, source })) => {
            tracing::warn!(
                "performance: команда выполнена ({result:?}), но read-back не удался: {source:?}"
            );
        }
        WorkerEvent::Gpu(result) => {
            apply_gpu_result(state, result);
        }
        WorkerEvent::ChargeLimit(result) => {
            apply_charge_limit_result(state, result);
        }
        WorkerEvent::ChargeLimitRefresh(result) => {
            apply_charge_limit_refresh(state, result);
        }
        WorkerEvent::GpuPowerRefresh(result) => {
            apply_gpu_power_refresh(state, result);
        }
        WorkerEvent::GpuMuxRefresh(result) => {
            apply_gpu_mux_refresh(state, result);
        }
        WorkerEvent::GpuAccessRefresh(result) => {
            apply_gpu_access_refresh(state, result);
        }
        WorkerEvent::PerformanceRefresh(result) => {
            apply_performance_refresh(state, result);
        }
        WorkerEvent::RegistryChange(Ok((generation, snapshot))) => {
            tracing::debug!("capability registry refreshed: generation={}", generation);
            state.update_capabilities(&snapshot);
        }
        WorkerEvent::RegistryChange(Err(e)) => {
            tracing::warn!("capability registry refresh failed: {e:?}");
        }
        WorkerEvent::TelemetryRefresh(Ok(telemetry)) => {
            // Authoritative telemetry snapshot: обновляем все telemetry-поля.
            // Отсутствующие (None) поля становятся "—" независимо друг от друга.
            state.update_telemetry(&telemetry);
        }
        WorkerEvent::TelemetryRefresh(Err(e)) => {
            // Ошибка read: НЕ затираем последний успешный telemetry state.
            tracing::warn!("telemetry refresh failed: {e:?}");
        }
    }
}

/// Обработчик события worker-а в UI event loop (через upgrade_in_event_loop).
fn handle_worker_event(app: &AppWindow, event: WorkerEvent) {
    let mut s = from_slint(&app.get_ui_state());
    apply_performance_event(&mut s, event);
    app.set_ui_state(to_slint(&s));
}

/// Регистрация Slint callbacks.
///
/// Performance, GPU Mode и Battery Charge Limit: отправляют типизированные
/// команды в общий worker (async результаты вернутся через event sink);
/// controller::apply для этих действий НЕ вызывается. UiState используется
/// только как boundary/model helper, не как кэш между callbacks.
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
                    tracing::warn!("perf-clicked вне интерактивного режима (worker отсутствует)");
                }
            }
        });
    }
    {
        let worker_tx = worker_tx.clone();
        app.on_gpu_clicked(move |i| {
            let Some(mode) = gpu_mode_from_index(i) else {
                tracing::warn!("gpu-clicked с неизвестным индексом: {i}");
                return;
            };
            // Production guard: клик по product GPU Mode карточке не должен
            // приводить к SetGpuMode, даже если кнопка не disabled (защита в
            // глубину поверх Slint disabled binding).
            if let Some(app) = app_weak.upgrade() {
                let s = from_slint(&app.get_ui_state());
                if !gpu_mode_click_allowed(&s) {
                    tracing::warn!(
                        "gpu-clicked игнорирован: product GPU mode недоступен/read-only"
                    );
                    return;
                }
            }
            // Безопасная политика для текущего UI: обычный клик не является
            // подтверждением потенциально чувствительной операции.
            let confirmed = false;
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(WorkerCommand::SetGpuMode { mode, confirmed }) {
                        tracing::warn!("worker закрыт, команда не отправлена: {e:?}");
                    }
                }
                None => {
                    tracing::warn!("gpu-clicked вне интерактивного режима (worker отсутствует)");
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
            // Production guard: disabled slider не должен отправлять mutation,
            // даже если Slint disabled binding не сработал (защита в глубину).
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
                    tracing::warn!("charge-changed вне интерактивного режима (worker отсутствует)");
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Детерминированный оффскрин-рендер (SoftwareRenderer)
// ---------------------------------------------------------------------------

/// WindowAdapter на программном рендерере (без окна и композитора).
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

/// Минимальная платформа для оффскрин-рендера.
struct SoftwarePlatform {
    adapter: Rc<SoftwareWindowAdapter>,
}

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.adapter.clone())
    }
}

/// Отрендерить окно в PNG (масштаб 100%, детерминированный размер).
fn render_screenshot(state: &controller::UiState, path: &str) -> anyhow::Result<()> {
    let height = window_height(state) as u32;
    let renderer = Rc::new(slint::platform::software_renderer::SoftwareRenderer::new());
    let adapter = Rc::new(SoftwareWindowAdapter {
        renderer: renderer.clone(),
        window: OnceCell::new(),
        size: Cell::new(PhysicalSize::new(425, height)),
    });
    {
        let dyn_adapter: Rc<dyn WindowAdapter> = adapter.clone();
        let weak: std::rc::Weak<dyn WindowAdapter> = Rc::downgrade(&dyn_adapter);
        let window = slint::Window::new(weak);
        adapter.window.set(window).ok();
    }
    slint::platform::set_platform(Box::new(SoftwarePlatform { adapter })).expect("platform once");

    // Offscreen path: runtime/worker не создаются; Performance callback no-op.
    let app = build_app(state, None)?;
    app.window()
        .set_size(LogicalSize::new(425.0, height as f32));
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

/// Инициализировать один global tracing subscriber для production GUI.
///
/// - stderr — обычный diagnostic destination;
/// - `RUST_LOG` (EnvFilter) полностью управляет фильтром, когда задан;
/// - без `RUST_LOG` default filter = `warn` (видны WARN и ERROR, debug/info
///   скрыты — не hard-code verbosity выше warn);
/// - malformed `RUST_LOG` не приводит к panic: используется безопасный
///   default `warn`;
/// - `try_init()` вместо `.init()`: при уже установленном global subscriber
///   возвращает Err без panic (duplicate-safe), произвольные configuration
///   errors не скрываются молча.
///
/// Ошибка инициализации не логируется через tracing (subscriber ещё не
/// установлен); поведение — тихо продолжить без subscriber, как раньше.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let mut state = state_for_scenario(&args.ui_state);

    if let Some(path) = args.screenshot {
        return render_screenshot(&state, &path);
    }

    // Tracing diagnostics: один global subscriber в GUI composition root.
    // Выполняется ДО создания runtime, session D-Bus connection, initial
    // Battery refresh и Slint event loop, чтобы существующие tracing::warn!/
    // debug! могли попадать в stderr.
    init_tracing();

    // Интерактивный запуск через штатный winit-бэкенд (без set_platform).
    // Один явный многопоточный runtime для worker-задачи.
    let runtime = tokio::runtime::Runtime::new()?;

    // Честный initial Battery state: fixture-профиль уже дал числовое значение
    // 80 из mock, но в интерактивном запуске оно НЕ должно быть видимо как
    // authoritative hardware state до первого provider read. Маскируем как
    // Loading; первый RefreshChargeLimit (ниже) переведёт в Ready/Unavailable.
    state.charge_limit_state = controller::ChargeLimitState::Loading;

    // Production Battery provider uses Session1 reads and a direct Hardware1
    // mutation source. The control is enabled only after the authoritative
    // Battery refresh confirms both configured and effective values.

    // Честный initial Performance state: fixture-профиль уже дал mock current
    // (Balanced), но в интерактивном запуске он НЕ должен быть видим как
    // authoritative hardware state до первого provider read. Маскируем как
    // Loading; первый RefreshPerformance переведёт в Ready/Unavailable.
    state.perf_state = controller::PerformanceHwState::Loading;

    // Честный initial Telemetry state: fixture-профиль дал mock значения
    // (72°C/65°C/28000 мВт), но они НЕ должны быть видимы как authoritative
    // hardware state до первого telemetry refresh. Сбрасываем в "—";
    // первый RefreshTelemetry (ниже) обновит из реального sysfs snapshot.
    state.reset_telemetry();

    // Production product GPU Mode: реального backend нет (read-only hardware
    // status Power/MUX/Access идёт через независимые capability providers).
    // Product policy остаётся недоказанной: не показывать selected mode как
    // authoritative и не разрешать mutation.
    state.gpu_mode_state = controller::GpuModeHwState::Unavailable;
    state.gpu_mode_writable = false;

    // Ровно одна user-session connection на composition/startup level.
    // Ошибка подключения завершает startup через существующий Result path;
    // никакого unwrap/expect и никакого silent fallback на MockProvider.
    let session_connection = runtime
        .block_on(zbus::Connection::session())
        .map_err(|e| anyhow::anyhow!("не удалось подключиться к session bus: {e}"))?;
    let system_connection = runtime
        .block_on(zbus::Connection::system())
        .map_err(|e| anyhow::anyhow!("не удалось подключиться к system bus: {e}"))?;
    // Read-only capability probe: the UI becomes writable only when the
    // production Hardware1 name is already owned. No mutation or polling.
    let (application_runtime, _hardware_owner) = runtime.block_on(build_production_runtime(
        session_connection,
        system_connection,
    ))?;
    // Mutation gating is derived exclusively from the registry snapshot write
    // capability (`WorkerCommand::RefreshCapabilities` ниже). Until the first
    // snapshot is published, evidence is absent and controls stay disabled.
    state.perf_writable = false;
    state.charge_limit_writable = false;

    let (worker_tx, worker_rx) = orbis_ui::worker::command_channel();

    let app = build_app(&state, Some(worker_tx.clone()))?;
    app.window()
        .set_size(LogicalSize::new(425.0, window_height(&state)));

    // Event sink: результат worker-а возвращается в UI event loop через
    // Weak<AppWindow>::upgrade_in_event_loop (безопасно при уничтоженном окне).
    let weak = app.as_weak();
    let event_sink = move |event: WorkerEvent| {
        let weak = weak.clone();
        if let Err(e) = weak.upgrade_in_event_loop(move |app| {
            handle_worker_event(&app, event);
        }) {
            tracing::warn!("не удалось вернуть событие worker-а в event loop: {e:?}");
        }
    };
    // Production telemetry polling: один владелец — worker. Интервал берётся
    // из provider contract (TelemetryProvider::default_poll_interval);
    // snapshot выполняется в фоне, не блокируя команды; первый tick пропущен
    // (initial RefreshTelemetry ниже не дублируется); остановка worker-а
    // останавливает polling.
    let poll_interval = application_runtime.telemetry.poll_interval();
    runtime.spawn(run_worker_with_polling(
        application_runtime,
        worker_rx,
        event_sink,
        poll_interval,
    ));

    // Ровно один authoritative initial Battery read при старте, без действия
    // пользователя и без polling. Ошибка provider (включая отсутствие
    // orbis-sessiond на будущем session backend) не превращается в mock data.
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshChargeLimit) {
        tracing::warn!("worker закрыт, initial battery refresh не отправлен: {e:?}");
    }

    // Ровно один initial refresh read-only GPU hardware capabilities.
    // Ошибка одного concept не блокирует остальные; без polling.
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshGpuCapabilities) {
        tracing::warn!("worker закрыт, initial gpu capabilities refresh не отправлен: {e:?}");
    }

    // Ровно один initial authoritative Performance refresh. Ошибка provider
    // (включая отсутствие orbis-sessiond) не превращается в mock data.
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshPerformance) {
        tracing::warn!("worker закрыт, initial performance refresh не отправлен: {e:?}");
    }

    // Ровно один initial capability registry refresh. Публикует initial
    // snapshot в UI state: отсюда derived mutation gating (perf/charge
    // writable) для существующих controls. Без polling; software failure
    // сохраняет previous snapshot и gating остаётся disabled.
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshCapabilities) {
        tracing::warn!("worker закрыт, initial capability refresh не отправлен: {e:?}");
    }

    // Ровно один initial telemetry refresh. Публикует authoritative sysfs
    // snapshot в UI state (CPU/GPU temp, fans, battery, AC). Без polling;
    // ошибка не затирает последний успешный state.
    if let Err(e) = worker_tx.send(WorkerCommand::RefreshTelemetry) {
        tracing::warn!("worker закрыт, initial telemetry refresh не отправлен: {e:?}");
    }

    app.show()?;
    slint::run_event_loop()?;

    // Завершение: освобождаем клоны sender (в callbacks), локальный sender и
    // runtime. Weak в worker-е не удерживает окно живым; после drop(app) sender
    // закрыт -> worker завершается.
    drop(app);
    drop(worker_tx);
    drop(runtime);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::battery::ChargeLimit;
    use orbis_core::battery::ChargeLimitBounds;
    use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
    use orbis_core::newtypes::Percent;

    fn base_state() -> controller::UiState {
        controller::UiState::from_mock_profile("zephyrus-full")
    }

    fn charge_outcome(percent: Option<u8>) -> ChargeLimitCommandOutcome {
        ChargeLimitCommandOutcome {
            result: ApplyResult::Applied,
            state: ChargeLimit::new(
                true,
                percent.map(|p| Percent::new(p).expect("range")),
                percent.map(|p| Percent::new(p).expect("range")),
                Some(
                    ChargeLimitBounds::new(
                        Percent::new(40).expect("const"),
                        Percent::new(100).expect("const"),
                        1,
                    )
                    .expect("valid"),
                ),
            )
            .expect("valid"),
        }
    }

    /// Собрать registry snapshot, где Performance/ChargeLimit имеют
    /// `read = Supported` и заданный `write` статус.
    fn snapshot_with_write(
        write: orbis_core::capability::CapabilityStatus,
    ) -> std::sync::Arc<orbis_capabilities::CapabilityRegistrySnapshot> {
        use orbis_core::capability::{
            Capability, CapabilityOperations, CapabilityStatus, OperationCapability,
        };
        let mut builder =
            orbis_capabilities::CapabilityRegistryBuilder::new(1, std::time::SystemTime::now());
        for feature in [
            orbis_core::FeatureId::Performance,
            orbis_core::FeatureId::ChargeLimit,
        ] {
            builder
                .add(
                    feature,
                    Capability::new(CapabilityStatus::Supported).with_operations(
                        CapabilityOperations {
                            read: OperationCapability::new(CapabilityStatus::Supported),
                            write: OperationCapability::new(write),
                        },
                    ),
                )
                .expect("capability must validate");
        }
        std::sync::Arc::new(builder.build().expect("snapshot must build"))
    }

    #[test]
    fn performance_index_mapping() {
        assert_eq!(
            performance_profile_from_index(0),
            Some(PerformanceProfile::Silent)
        );
        assert_eq!(
            performance_profile_from_index(1),
            Some(PerformanceProfile::Balanced)
        );
        assert_eq!(
            performance_profile_from_index(2),
            Some(PerformanceProfile::Turbo)
        );
        assert_eq!(performance_profile_from_index(-1), None);
        assert_eq!(performance_profile_from_index(7), None);
    }

    #[test]
    fn performance_available_mask_ignores_order() {
        let full = vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ];
        let shuffled = vec![
            PerformanceProfile::Turbo,
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
        ];
        assert_eq!(performance_available_mask(&full), 0b111);
        assert_eq!(performance_available_mask(&shuffled), 0b111);
        assert_eq!(
            performance_available_mask(&[PerformanceProfile::Turbo, PerformanceProfile::Silent]),
            0b101
        );
    }

    #[test]
    fn performance_write_probe_requires_owned_hardware_name() {
        assert!(performance_write_available(Some(true)));
        assert!(!performance_write_available(Some(false)));
        assert!(!performance_write_available(None));
    }

    #[test]
    fn performance_click_guard_requires_ready_writable_available_state() {
        let mut state = base_state();
        assert!(performance_click_allowed(&state, 0));

        state.perf_writable = false;
        assert!(!performance_click_allowed(&state, 0));

        state.perf_writable = true;
        state.perf_state = controller::PerformanceHwState::Unavailable;
        assert!(!performance_click_allowed(&state, 0));

        state.perf_state = controller::PerformanceHwState::Ready;
        state.available_perf_mask = 0b010;
        assert!(!performance_click_allowed(&state, 0));
        assert!(performance_click_allowed(&state, 1));
    }

    #[test]
    fn performance_click_guard_emits_only_authorized_command() {
        let mut state = base_state();
        assert_eq!(
            performance_command_for_click(&state, 0),
            Some(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
        );

        state.perf_writable = false;
        assert_eq!(performance_command_for_click(&state, 0), None);

        state.perf_writable = true;
        state.available_perf_mask = 0b010;
        assert_eq!(performance_command_for_click(&state, 0), None);
        assert_eq!(
            performance_command_for_click(&state, 1),
            Some(WorkerCommand::SetPerformance(PerformanceProfile::Balanced))
        );
    }

    #[test]
    fn authoritative_performance_result_updates_ui() {
        let mut s = base_state();
        assert_eq!(s.perf_selected, 1); // Balanced из mock

        let outcome = PerformanceCommandOutcome {
            result: ApplyResult::Applied,
            state: orbis_application::PerformanceState {
                current: PerformanceProfile::Silent,
                available: vec![
                    PerformanceProfile::Silent,
                    PerformanceProfile::Balanced,
                    PerformanceProfile::Turbo,
                ],
            },
        };
        apply_performance_event(&mut s, WorkerEvent::Performance(Ok(outcome)));

        assert_eq!(s.perf_selected, 0); // из authoritative state
        assert_eq!(s.available_perf_mask, 0b111);
    }

    #[test]
    fn performance_result_preserves_other_sections() {
        let mut s = base_state();
        // задаём отличимые не-Performance поля
        s.gpu_selected = 3;
        s.gpu_ultimate_pending = true;
        s.gpu_section_error = true;
        s.charge_limit = 60;
        s.cpu_temp = "99°C".into();

        let outcome = PerformanceCommandOutcome {
            result: ApplyResult::Applied,
            state: orbis_application::PerformanceState {
                current: PerformanceProfile::Turbo,
                available: vec![PerformanceProfile::Turbo],
            },
        };
        apply_performance_event(&mut s, WorkerEvent::Performance(Ok(outcome)));

        assert_eq!(s.perf_selected, 2);
        assert_eq!(s.available_perf_mask, 0b100);
        assert_eq!(s.gpu_selected, 3);
        assert!(s.gpu_ultimate_pending);
        assert!(s.gpu_section_error);
        assert_eq!(s.charge_limit, 60);
        assert_eq!(s.cpu_temp, "99°C");
    }

    #[test]
    fn performance_command_error_does_not_mutate_ui() {
        let mut s = base_state();
        let before = s.clone();
        apply_performance_event(
            &mut s,
            WorkerEvent::Performance(Err(CommandError::Command(
                orbis_providers::error::ProviderError::Unsupported("x".into()),
            ))),
        );
        assert_eq!(s, before);
    }

    #[test]
    fn performance_readback_error_does_not_mutate_ui() {
        let mut s = base_state();
        let before = s.clone();
        apply_performance_event(
            &mut s,
            WorkerEvent::Performance(Err(CommandError::ReadBack {
                result: ApplyResult::Applied,
                source: orbis_providers::error::ProviderError::Timeout("t".into()),
            })),
        );
        assert_eq!(s, before);
    }

    #[test]
    fn gpu_index_mapping() {
        assert_eq!(gpu_mode_from_index(0), Some(GpuMode::Eco));
        assert_eq!(gpu_mode_from_index(1), Some(GpuMode::Standard));
        assert_eq!(gpu_mode_from_index(2), Some(GpuMode::Ultimate));
        assert_eq!(gpu_mode_from_index(3), Some(GpuMode::Optimized));
        assert_eq!(gpu_mode_from_index(-1), None);
        assert_eq!(gpu_mode_from_index(9), None);
    }

    fn applied_outcome(requested: GpuMode) -> GpuCommandOutcome {
        GpuCommandOutcome {
            result: ApplyResult::Applied,
            state: orbis_application::GpuState {
                requested,
                mux: GpuMuxState::Integrated,
                access_policy: GpuAccessPolicy::Blocked,
                power_state: GpuPowerState::Active,
                requirement: ActionRequirement::None,
            },
        }
    }

    #[test]
    fn authoritative_gpu_applied_updates_ui() {
        let mut s = base_state();
        // отличимые Performance/Battery/GPU-поля
        s.perf_selected = 0;
        s.charge_limit = 65;
        s.available_gpu_mask = 0b1111;
        s.gpu_ultimate_disabled = false;
        s.gpu_section_error = true; // стартовая ошибка

        apply_gpu_result(&mut s, Ok(applied_outcome(GpuMode::Optimized)));

        assert_eq!(s.gpu_selected, 3); // Optimized
        assert!(!s.gpu_ultimate_pending);
        assert!(!s.gpu_section_error);
        // mask/disabled сохранены
        assert_eq!(s.available_gpu_mask, 0b1111);
        assert!(!s.gpu_ultimate_disabled);
        // Performance/Battery сохранены
        assert_eq!(s.perf_selected, 0);
        assert_eq!(s.charge_limit, 65);
    }

    #[test]
    fn ultimate_pending_updates_existing_ui() {
        let mut s = base_state();
        let outcome = GpuCommandOutcome {
            result: ApplyResult::Pending {
                requirement: ActionRequirement::Reboot,
            },
            state: orbis_application::GpuState {
                requested: GpuMode::Ultimate,
                mux: GpuMuxState::Integrated,
                access_policy: GpuAccessPolicy::Unblocked,
                power_state: GpuPowerState::Active,
                requirement: ActionRequirement::Reboot,
            },
        };
        apply_gpu_result(&mut s, Ok(outcome));

        assert_eq!(s.gpu_selected, 2); // Ultimate
        assert!(s.gpu_ultimate_pending);
        assert!(!s.gpu_section_error);
    }

    #[test]
    fn eco_logout_pending_does_not_set_ultimate_flag() {
        let mut s = base_state();
        let outcome = GpuCommandOutcome {
            result: ApplyResult::Pending {
                requirement: ActionRequirement::Logout,
            },
            state: orbis_application::GpuState {
                requested: GpuMode::Eco,
                mux: GpuMuxState::Integrated,
                access_policy: GpuAccessPolicy::Blocked,
                power_state: GpuPowerState::Suspended,
                requirement: ActionRequirement::Logout,
            },
        };
        apply_gpu_result(&mut s, Ok(outcome));

        assert_eq!(s.gpu_selected, 0); // Eco
        assert!(!s.gpu_ultimate_pending);
        assert!(!s.gpu_section_error);
    }

    #[test]
    fn gpu_command_error_preserves_state_and_sets_error() {
        let mut s = base_state();
        s.perf_selected = 0;
        s.charge_limit = 65;
        let selected_before = s.gpu_selected;
        let pending_before = s.gpu_ultimate_pending;
        let mask_before = s.available_gpu_mask;
        let disabled_before = s.gpu_ultimate_disabled;

        apply_gpu_result(
            &mut s,
            Err(CommandError::Command(
                orbis_providers::error::ProviderError::Unsupported("x".into()),
            )),
        );

        assert_eq!(s.gpu_selected, selected_before);
        assert_eq!(s.gpu_ultimate_pending, pending_before);
        assert_eq!(s.available_gpu_mask, mask_before);
        assert_eq!(s.gpu_ultimate_disabled, disabled_before);
        assert!(s.gpu_section_error);
        assert_eq!(s.perf_selected, 0);
        assert_eq!(s.charge_limit, 65);
    }

    #[test]
    fn gpu_readback_error_preserves_state_and_sets_error() {
        let mut s = base_state();
        let before = s.clone();

        apply_gpu_result(
            &mut s,
            Err(CommandError::ReadBack {
                result: ApplyResult::Applied,
                source: orbis_providers::error::ProviderError::Timeout("t".into()),
            }),
        );

        assert_eq!(s.gpu_selected, before.gpu_selected);
        assert_eq!(s.gpu_ultimate_pending, before.gpu_ultimate_pending);
        assert_eq!(s.available_gpu_mask, before.available_gpu_mask);
        assert_eq!(s.gpu_ultimate_disabled, before.gpu_ultimate_disabled);
        assert!(s.gpu_section_error);
        assert_eq!(s.perf_selected, before.perf_selected);
        assert_eq!(s.charge_limit, before.charge_limit);
    }

    #[test]
    fn successful_gpu_result_clears_previous_error() {
        let mut s = base_state();
        s.gpu_section_error = true;

        apply_gpu_result(&mut s, Ok(applied_outcome(GpuMode::Standard)));

        assert!(!s.gpu_section_error);
        assert_eq!(s.gpu_selected, 1); // Standard из authoritative state
    }

    #[test]
    fn charge_limit_ui_mapping() {
        assert_eq!(charge_limit_from_ui(19.0), None);
        assert_eq!(charge_limit_from_ui(20.0), Some(20));
        assert_eq!(charge_limit_from_ui(21.0), Some(21));
        assert_eq!(charge_limit_from_ui(80.0), Some(80));
        assert_eq!(charge_limit_from_ui(99.0), Some(99));
        assert_eq!(charge_limit_from_ui(100.0), Some(100));
        assert_eq!(charge_limit_from_ui(83.0), Some(83));
        assert_eq!(charge_limit_from_ui(-1.0), None);
        assert_eq!(charge_limit_from_ui(101.0), None);
        assert_eq!(charge_limit_from_ui(f32::NAN), None);
        assert_eq!(charge_limit_from_ui(f32::INFINITY), None);
        assert_eq!(charge_limit_from_ui(80.5), None); // дробное не усекается
    }

    #[test]
    fn battery_writable_requires_owner_ready_and_both_values() {
        let limit = ChargeLimit::new(
            false,
            Some(orbis_core::newtypes::Percent::new(80).unwrap()),
            Some(orbis_core::newtypes::Percent::new(100).unwrap()),
            None,
        )
        .unwrap();
        assert!(battery_write_available(
            true,
            controller::ChargeLimitState::Ready,
            &limit
        ));
        assert!(!battery_write_available(
            false,
            controller::ChargeLimitState::Ready,
            &limit
        ));
        assert!(!battery_write_available(
            true,
            controller::ChargeLimitState::Loading,
            &limit
        ));
        let missing_effective = ChargeLimit::new(
            false,
            Some(orbis_core::newtypes::Percent::new(80).unwrap()),
            None,
            None,
        )
        .unwrap();
        assert!(!battery_write_available(
            true,
            controller::ChargeLimitState::Ready,
            &missing_effective
        ));
    }

    #[test]
    fn charge_mutation_allowed_requires_writable_and_ready() {
        let mut s = base_state();
        assert!(s.charge_limit_writable); // mock default writable
        assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
        assert!(charge_mutation_allowed(&s));

        s.charge_limit_writable = false;
        assert!(!charge_mutation_allowed(&s));

        s.charge_limit_writable = true;
        s.charge_limit_state = controller::ChargeLimitState::Loading;
        assert!(!charge_mutation_allowed(&s));

        s.charge_limit_state = controller::ChargeLimitState::Unavailable;
        assert!(!charge_mutation_allowed(&s));
    }

    #[test]
    fn write_status_gating_derives_from_registry() {
        use orbis_core::capability::CapabilityStatus;

        for (write, expected) in [
            (CapabilityStatus::Supported, true),
            (CapabilityStatus::SupportedWithRequirement, true),
            (CapabilityStatus::ReadOnly, false),
            (CapabilityStatus::Unsupported, false),
            (CapabilityStatus::BackendMissing, false),
            (CapabilityStatus::TemporarilyUnavailable, false),
            (CapabilityStatus::PermissionDenied, false),
            (CapabilityStatus::Unknown, false),
        ] {
            let mut s = base_state();
            let snapshot = snapshot_with_write(write);
            s.update_capabilities(&snapshot);
            assert_eq!(s.perf_writable, expected, "perf write={write:?}");
            assert_eq!(s.charge_limit_writable, expected, "charge write={write:?}");
        }
    }

    #[test]
    fn registry_change_updates_gating_without_touching_observed() {
        use orbis_core::capability::CapabilityStatus;

        let mut s = base_state();
        // Отличимые observed values.
        s.perf_selected = 2;
        s.charge_limit = 60;
        s.gpu_power_value = 1;
        s.gpu_mux_value = 2;

        // Write Unsupported → gating disabled, observed values неизменны.
        let snapshot = snapshot_with_write(CapabilityStatus::Unsupported);
        apply_performance_event(
            &mut s,
            WorkerEvent::RegistryChange(Ok((2, snapshot.clone()))),
        );
        assert!(!s.perf_writable);
        assert!(!s.charge_limit_writable);
        assert_eq!(s.perf_selected, 2);
        assert_eq!(s.charge_limit, 60);
        assert_eq!(s.gpu_power_value, 1);
        assert_eq!(s.gpu_mux_value, 2);

        // Write Supported → gating enabled, observed values по-прежнему неизменны.
        let snapshot = snapshot_with_write(CapabilityStatus::Supported);
        apply_performance_event(
            &mut s,
            WorkerEvent::RegistryChange(Ok((3, snapshot.clone()))),
        );
        assert!(s.perf_writable);
        assert!(s.charge_limit_writable);
        assert_eq!(s.perf_selected, 2);
        assert_eq!(s.charge_limit, 60);
        assert_eq!(s.gpu_power_value, 1);
        assert_eq!(s.gpu_mux_value, 2);
    }

    #[test]
    fn disabled_charge_control_does_not_emit_mutation_command() {
        // Rust-side guard: даже если Slint disabled binding не сработал,
        // charge mutation разрешена только при writable + Ready.
        let mut s = base_state();
        s.charge_limit_writable = false;
        assert!(!charge_mutation_allowed(&s));

        s.charge_limit_writable = true;
        s.charge_limit_state = controller::ChargeLimitState::Unavailable;
        assert!(!charge_mutation_allowed(&s));
    }

    #[test]
    fn telemetry_refresh_updates_ui_and_error_keeps_previous_state() {
        let mut s = base_state();
        s.reset_telemetry();
        assert_eq!(s.cpu_temp, "—");

        // Ok: обновляет telemetry display из authoritative snapshot.
        let t = orbis_core::telemetry::Telemetry {
            cpu_temp: Some(orbis_core::newtypes::TemperatureC::new(46).unwrap()),
            gpu_temp: Some(orbis_core::newtypes::TemperatureC::new(43).unwrap()),
            fans: vec![orbis_core::telemetry::FanTelemetry {
                fan: orbis_core::fan::FanId::Cpu,
                rpm: orbis_core::newtypes::Rpm::new(2600).unwrap(),
                percent: None,
            }],
            power: orbis_core::telemetry::PowerTelemetry {
                ac: None,
                battery: None,
                total: None,
                gpu: Some(orbis_core::newtypes::MilliWatt::new(13_073).unwrap()),
            },
            ac_online: Some(false),
            battery: None,
            gpu_power_state: orbis_core::gpu::GpuPowerState::Unknown,
            ts: std::time::SystemTime::UNIX_EPOCH,
        };
        apply_performance_event(&mut s, WorkerEvent::TelemetryRefresh(Ok(t)));
        assert_eq!(s.cpu_temp, "46°C");
        assert_eq!(s.gpu_temp, "43°C");
        assert_eq!(s.cpu_fan_rpm, "2600 rpm");
        assert_eq!(s.gpu_fan_rpm, "—");
        assert_eq!(s.battery_percent, "—");
        assert_eq!(s.ac_online, "On battery");
        assert_eq!(s.gpu_power_display, "13 W");

        // Err: не затирает последний успешный telemetry state.
        apply_performance_event(
            &mut s,
            WorkerEvent::TelemetryRefresh(Err(orbis_providers::error::ProviderError::Io(
                std::io::Error::other("test"),
            ))),
        );
        assert_eq!(s.cpu_temp, "46°C");
        assert_eq!(s.gpu_power_display, "13 W");
    }

    #[test]
    fn authoritative_charge_limit_updates_ui() {
        let mut s = base_state();
        // отличимые Performance/GPU поля
        s.perf_selected = 0;
        s.gpu_selected = 3;
        s.gpu_ultimate_pending = true;
        s.gpu_section_error = true;

        apply_charge_limit_result(&mut s, Ok(charge_outcome(Some(40))));

        assert_eq!(s.charge_limit, 40);
        assert_eq!(s.perf_selected, 0);
        assert_eq!(s.gpu_selected, 3);
        assert!(s.gpu_ultimate_pending);
        assert!(s.gpu_section_error);
    }

    #[test]
    fn authoritative_charge_value_is_not_sent_value() {
        let mut s = base_state();
        // "отправлено" одно значение, но authoritative read-back вернул 45:
        // helper применяет outcome.state.configured_percent, не входное значение.
        apply_charge_limit_result(&mut s, Ok(charge_outcome(Some(45))));
        assert_eq!(s.charge_limit, 45);
    }

    #[test]
    fn charge_limit_none_preserves_ui() {
        let mut s = base_state();
        let before = s.clone();

        apply_charge_limit_result(&mut s, Ok(charge_outcome(None)));

        // percent None: прежнее значение сохраняется, остальные поля не тронуты.
        assert_eq!(s, before);
    }

    #[test]
    fn charge_limit_command_error_does_not_mutate_ui() {
        let mut s = base_state();
        let before = s.clone();

        apply_charge_limit_result(
            &mut s,
            Err(CommandError::Command(
                orbis_providers::error::ProviderError::Unsupported("x".into()),
            )),
        );

        assert_eq!(s, before);
    }

    #[test]
    fn charge_limit_readback_error_does_not_mutate_ui() {
        let mut s = base_state();
        let before = s.clone();

        apply_charge_limit_result(
            &mut s,
            Err(CommandError::ReadBack {
                result: ApplyResult::Applied,
                source: orbis_providers::error::ProviderError::Timeout("t".into()),
            }),
        );

        assert_eq!(s, before);
    }

    #[test]
    fn refresh_success_sets_ready_and_value() {
        let mut s = base_state();
        // Интерактивный startup маскирует Battery как Loading до read.
        s.charge_limit_state = controller::ChargeLimitState::Loading;

        apply_charge_limit_refresh(
            &mut s,
            Ok(ChargeLimit::new(
                true,
                Some(Percent::new(60).expect("range")),
                Some(Percent::new(60).expect("range")),
                None, // unknown bounds допустимы
            )
            .expect("valid")),
        );

        assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
        assert_eq!(s.charge_limit, 60);
        assert!(s.charge_limit_enabled);
    }

    #[test]
    fn refresh_disabled_keeps_configured_value_but_marks_limit_off() {
        let mut s = base_state();
        s.charge_limit_state = controller::ChargeLimitState::Loading;
        apply_charge_limit_refresh(
            &mut s,
            Ok(ChargeLimit::new(
                false,
                Some(Percent::new(80).expect("configured")),
                Some(Percent::new(100).expect("effective")),
                None,
            )
            .expect("valid")),
        );

        assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
        assert_eq!(s.charge_limit, 80);
        assert!(!s.charge_limit_enabled);
        // Slint renders the configured value only as secondary information and
        // uses the enabled=false branch instead of an active slider.
    }

    #[test]
    fn refresh_percent_none_is_unavailable_without_fixture() {
        let mut s = base_state();
        // Даже если numeric backing содержит fixture 80, при percent=None оно
        // НЕ должно стать authoritative.
        s.charge_limit_state = controller::ChargeLimitState::Loading;
        s.charge_limit = 80; // fixture/default из from_mock_profile

        apply_charge_limit_refresh(
            &mut s,
            Ok(ChargeLimit::new(false, None, None, None).expect("valid")),
        );

        assert_eq!(
            s.charge_limit_state,
            controller::ChargeLimitState::Unavailable
        );
        // fixture-значение не перезаписывается как authoritative и не
        // показывается: UI при Unavailable скрывает slider.
        assert_eq!(s.charge_limit, 80);
    }

    #[test]
    fn refresh_error_is_unavailable_without_fixture() {
        let mut s = base_state();
        s.charge_limit_state = controller::ChargeLimitState::Loading;
        s.charge_limit = 80; // fixture/default из from_mock_profile

        apply_charge_limit_refresh(
            &mut s,
            Err(orbis_providers::error::ProviderError::BackendUnavailable(
                "sessiond missing".into(),
            )),
        );

        assert_eq!(
            s.charge_limit_state,
            controller::ChargeLimitState::Unavailable
        );
        assert_eq!(s.charge_limit, 80);
        // Уже существовавший Ready value не помечается как authoritative при
        // Unavailable — slider скрыт.
    }

    // -----------------------------------------------------------------------
    // Read-only GPU hardware capability refresh semantics
    // -----------------------------------------------------------------------

    #[test]
    fn gpu_power_unknown_is_ready_not_unavailable() {
        let mut s = base_state();
        s.gpu_power = controller::GpuHwState::Loading;
        apply_gpu_power_refresh(&mut s, Ok(GpuPowerState::Unknown));
        // Domain Unknown — валидный Ready state, НЕ Unavailable.
        assert_eq!(s.gpu_power, controller::GpuHwState::Ready);
        assert_eq!(s.gpu_power_value, 4);
    }

    #[test]
    fn gpu_power_error_is_unavailable() {
        let mut s = base_state();
        s.gpu_power = controller::GpuHwState::Loading;
        apply_gpu_power_refresh(
            &mut s,
            Err(orbis_providers::error::ProviderError::Dbus("down".into())),
        );
        assert_eq!(s.gpu_power, controller::GpuHwState::Unavailable);
    }

    #[test]
    fn gpu_mux_and_access_values_mapped() {
        let mut s = base_state();
        apply_gpu_mux_refresh(&mut s, Ok(GpuMuxState::Discrete));
        assert_eq!(s.gpu_mux, controller::GpuHwState::Ready);
        assert_eq!(s.gpu_mux_value, 1);
        apply_gpu_access_refresh(&mut s, Ok(GpuAccessPolicy::Blocked));
        assert_eq!(s.gpu_access, controller::GpuHwState::Ready);
        assert_eq!(s.gpu_access_value, 1);
    }

    #[test]
    fn gpu_hw_states_default_loading() {
        let s = base_state();
        assert_eq!(s.gpu_power, controller::GpuHwState::Loading);
        assert_eq!(s.gpu_mux, controller::GpuHwState::Loading);
        assert_eq!(s.gpu_access, controller::GpuHwState::Loading);
    }

    // -----------------------------------------------------------------------
    // Read-only Performance Mode refresh semantics
    // -----------------------------------------------------------------------

    fn perf_state(
        current: PerformanceProfile,
        available: &[PerformanceProfile],
    ) -> PerformanceState {
        PerformanceState {
            current,
            available: available.to_vec(),
        }
    }

    #[test]
    fn perf_refresh_success_sets_ready_and_authoritative_state() {
        let mut s = base_state();
        // mock fixture: Balanced; production маскирует как Loading до read.
        s.perf_state = controller::PerformanceHwState::Loading;

        apply_performance_refresh(
            &mut s,
            Ok(perf_state(
                PerformanceProfile::Silent,
                &[
                    PerformanceProfile::Silent,
                    PerformanceProfile::Balanced,
                    PerformanceProfile::Turbo,
                ],
            )),
        );

        assert_eq!(s.perf_state, controller::PerformanceHwState::Ready);
        assert_eq!(s.perf_selected, 0); // Silent из authoritative state
        assert_eq!(s.available_perf_mask, 0b111);
    }

    #[test]
    fn perf_refresh_error_is_unavailable_without_mock_fallback() {
        let mut s = base_state();
        s.perf_state = controller::PerformanceHwState::Loading;
        s.perf_selected = 1; // mock Balanced из from_mock_profile

        apply_performance_refresh(
            &mut s,
            Err(orbis_providers::error::ProviderError::BackendUnavailable(
                "sessiond missing".into(),
            )),
        );

        assert_eq!(s.perf_state, controller::PerformanceHwState::Unavailable);
        // fake/mock current не остаётся выделенным как authoritative: карточки
        // disabled при Unavailable; значение сохраняется, но не показывается.
        assert_eq!(s.perf_selected, 1);
    }

    #[test]
    fn perf_refresh_available_partial_mask() {
        let mut s = base_state();
        s.perf_state = controller::PerformanceHwState::Loading;

        apply_performance_refresh(
            &mut s,
            Ok(perf_state(
                PerformanceProfile::Turbo,
                &[PerformanceProfile::Turbo],
            )),
        );

        assert_eq!(s.perf_state, controller::PerformanceHwState::Ready);
        assert_eq!(s.perf_selected, 2);
        assert_eq!(s.available_perf_mask, 0b100);
    }

    #[test]
    fn perf_hw_state_default_ready_writable_in_mock() {
        // mock/offscreen: Ready + writable (fake interactive semantics);
        // production main() отдельно выставляет Loading + writable=false.
        let s = base_state();
        assert_eq!(s.perf_state, controller::PerformanceHwState::Ready);
        assert!(s.perf_writable);
    }

    // -----------------------------------------------------------------------
    // Production product GPU Mode: disabled, не selected, click не мутирует
    // -----------------------------------------------------------------------

    fn production_gpu_mode_state() -> controller::UiState {
        let mut s = base_state();
        s.gpu_mode_state = controller::GpuModeHwState::Unavailable;
        s.gpu_mode_writable = false;
        s
    }

    #[test]
    fn production_gpu_mode_is_disabled_and_not_selected() {
        let s = production_gpu_mode_state();
        // Все 4 карточки (Eco/Standard/Ultimate/Optimized) в production
        // disabled (клик не приведёт к SetGpuMode) и ни одна не selected
        // (никакой mode не выглядит authoritative).
        for idx in 0..4 {
            let mask_bit = 1 << idx;
            assert!(
                gpu_mode_card_disabled(&s, idx, mask_bit),
                "карточка {idx} должна быть disabled в production"
            );
            assert!(
                !gpu_mode_card_selected(&s, idx),
                "карточка {idx} не должна быть selected в production"
            );
        }
    }

    #[test]
    fn production_gpu_mode_mock_selected_is_hidden() {
        let mut s = base_state();
        // fixture: gpu_selected = 1 (Standard) из MockProvider; production
        // должен скрыть его как fake/не-authoritative.
        assert_eq!(s.gpu_selected, 1);
        s.gpu_mode_state = controller::GpuModeHwState::Unavailable;
        s.gpu_mode_writable = false;
        assert!(!gpu_mode_card_selected(&s, 1));
    }

    #[test]
    fn gpu_click_allowed_production_is_false() {
        let s = production_gpu_mode_state();
        assert!(!gpu_mode_click_allowed(&s));
    }

    #[test]
    fn gpu_click_allowed_mock_is_true() {
        let s = base_state();
        assert!(gpu_mode_click_allowed(&s));
    }

    #[test]
    fn mock_gpu_mode_stays_interactive() {
        // mock/offscreen: Standard (1) selected; остальные по маске 0b1111
        // доступны (не disabled из-за mock semantics).
        let s = base_state();
        assert_eq!(s.gpu_mode_state, controller::GpuModeHwState::Ready);
        assert!(s.gpu_mode_writable);
        assert!(gpu_mode_card_selected(&s, 1));
        for idx in 0..4 {
            assert!(
                !gpu_mode_card_disabled(&s, idx, 1 << idx),
                "карточка {idx} должна быть доступна в mock (маска 0b1111)"
            );
        }
    }

    #[test]
    fn production_gpu_mode_setup_preserves_hardware_status() {
        // Production-сетап (Unavailable + writable=false) не трогает real
        // read-only GPU hardware status (Power/MUX/Access остаются Loading до
        // authoritative refresh).
        let s = production_gpu_mode_state();
        assert_eq!(s.gpu_power, controller::GpuHwState::Loading);
        assert_eq!(s.gpu_mux, controller::GpuHwState::Loading);
        assert_eq!(s.gpu_access, controller::GpuHwState::Loading);
    }
}
