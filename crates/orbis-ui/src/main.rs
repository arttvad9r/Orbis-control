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
use std::sync::Arc;

use orbis_application::{
    AppService, ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome,
    PerformanceCommandOutcome, PerformanceState, SetChargeLimitError, SetGpuModeError,
};
use orbis_core::action::{ActionRequirement, ApplyResult};
use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_providers::mock::MockProvider;
use orbis_session_client::{
    SessionChargeLimitProvider, SessionGpuAccessProvider, SessionGpuMuxProvider,
    SessionGpuPowerProvider, SessionPerformanceProvider, ZbusSessionChargeLimitSource,
    ZbusSessionGpuSource, ZbusSessionPerformanceSource,
};
use orbis_test_support::devices::build_state_arc;
use orbis_ui::worker::{WorkerCommand, WorkerEvent, run_worker};
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
/// Performance-секции статус-строка Loading/Unavailable (+30px).
fn window_height(state: &controller::UiState) -> f32 {
    // высота клиентской области без внутреннего titlebar (34px удалены)
    if state.gpu_section_error {
        436.0
    } else {
        411.0
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
        cpu_temp: state.cpu_temp,
        gpu_temp: state.gpu_temp,
        cpu_fan_rpm: state.cpu_fan_rpm,
        gpu_fan_rpm: state.gpu_fan_rpm,
        battery_percent: state.battery_percent,
        power_ac_mw: state.power_ac_mw,
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
        cpu_temp: state.cpu_temp,
        gpu_temp: state.gpu_temp,
        cpu_fan_rpm: state.cpu_fan_rpm,
        gpu_fan_rpm: state.gpu_fan_rpm,
        battery_percent: state.battery_percent,
        power_ac_mw: state.power_ac_mw,
        version: state.version.to_string(),
        mock_profile: state.mock_profile.to_string(),
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
/// диапазоне 40..=100 (шаг 5 — политика Slint slider, здесь не проверяется;
/// 83 допустимо на Rust boundary). NaN/infinity/дробные/вне диапазона -> None.
fn charge_limit_from_ui(value: f32) -> Option<u8> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if !(40.0..=100.0).contains(&value) {
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
/// Обновляется только `charge_limit` (из `outcome.state.percent`, если он
/// присутствует); Performance/GPU и остальные поля сохраняются. При
/// `percent == None` прежнее UI-значение сохраняется, пишется warning.
fn apply_charge_limit_outcome(
    state: &mut controller::UiState,
    outcome: &ChargeLimitCommandOutcome,
) {
    match outcome.state.percent {
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
        Ok(limit) => match limit.percent {
            Some(percent) => {
                state.charge_limit = i32::from(percent.get());
                state.charge_limit_enabled = limit.enabled;
                state.charge_limit_state = controller::ChargeLimitState::Ready;
                tracing::debug!(
                    "battery: refresh OK, percent={}, enabled={}",
                    percent.get(),
                    limit.enabled
                );
            }
            None => {
                state.charge_limit_state = controller::ChargeLimitState::Unavailable;
                tracing::warn!(
                    "battery: refresh OK, но authoritative percent отсутствует (None); не подставляю fixture/default"
                );
            }
        },
        Err(e) => {
            state.charge_limit_state = controller::ChargeLimitState::Unavailable;
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
    {
        let worker_tx = worker_tx.clone();
        app.on_perf_clicked(move |i| {
            let Some(profile) = performance_profile_from_index(i) else {
                tracing::warn!("perf-clicked с неизвестным индексом: {i}");
                return;
            };
            match &worker_tx {
                Some(tx) => {
                    if let Err(e) = tx.send(WorkerCommand::SetPerformance(profile)) {
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
        app.on_charge_changed(move |v| {
            let Some(percent) = charge_limit_from_ui(v) else {
                tracing::warn!("charge-changed с недопустимым значением: {v}");
                return;
            };
            match &worker_tx {
                Some(tx) => {
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

    // Production Battery backend — read-only session client (SessionChargeLimitProvider
    // возвращает Unsupported для set_charge_limit): mutation control не должен
    // выглядеть рабочим. Это независимо от charge_limit_enabled (hardware state).
    state.charge_limit_writable = false;

    // Честный initial Performance state: fixture-профиль уже дал mock current
    // (Balanced), но в интерактивном запуске он НЕ должен быть видим как
    // authoritative hardware state до первого provider read. Маскируем как
    // Loading; первый RefreshPerformance переведёт в Ready/Unavailable.
    state.perf_state = controller::PerformanceHwState::Loading;

    // Production Performance backend — read-only session client
    // (SessionPerformanceProvider возвращает Unsupported для set_profile):
    // mutation control не должен выглядеть рабочим; кнопки read-only.
    state.perf_writable = false;

    // Mock provider и application service для Performance/GPU (только
    // интерактивный путь). Battery production больше через MockProvider не читается.
    let mock_state = build_state_arc("zephyrus-full")
        .ok_or_else(|| anyhow::anyhow!("mock profile 'zephyrus-full' отсутствует"))?;
    let mock_provider = Arc::new(MockProvider::new(mock_state));
    let main_service = AppService::new(mock_provider);

    // Ровно одна user-session connection на composition/startup level.
    // Ошибка подключения завершает startup через существующий Result path;
    // никакого unwrap/expect и никакого silent fallback на MockProvider.
    let session_connection = runtime
        .block_on(zbus::Connection::session())
        .map_err(|e| anyhow::anyhow!("не удалось подключиться к session bus: {e}"))?;
    let battery_source = ZbusSessionChargeLimitSource::new(session_connection.clone());
    let battery_provider = SessionChargeLimitProvider::new(battery_source);
    let battery_service = AppService::new(Arc::new(battery_provider));

    // Read-only GPU hardware capabilities через тот же session connection
    // (по ADR 0005: независимые capability providers, без mega-GpuProvider).
    let gpu_power_service = AppService::new(Arc::new(SessionGpuPowerProvider::new(
        ZbusSessionGpuSource::new(session_connection.clone()),
    )));
    let gpu_mux_service = AppService::new(Arc::new(SessionGpuMuxProvider::new(
        ZbusSessionGpuSource::new(session_connection.clone()),
    )));
    let gpu_access_service = AppService::new(Arc::new(SessionGpuAccessProvider::new(
        ZbusSessionGpuSource::new(session_connection.clone()),
    )));

    // Read-only Performance Mode через тот же session connection
    // (отдельный real read service; НЕ main MockProvider).
    let performance_service = AppService::new(Arc::new(SessionPerformanceProvider::new(
        ZbusSessionPerformanceSource::new(session_connection),
    )));

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
    runtime.spawn(run_worker(
        main_service,
        battery_service,
        gpu_power_service,
        gpu_mux_service,
        gpu_access_service,
        performance_service,
        worker_rx,
        event_sink,
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
        s.cpu_temp = 99;

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
        assert_eq!(s.cpu_temp, 99);
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
        assert_eq!(charge_limit_from_ui(40.0), Some(40));
        assert_eq!(charge_limit_from_ui(80.0), Some(80));
        assert_eq!(charge_limit_from_ui(100.0), Some(100));
        assert_eq!(charge_limit_from_ui(83.0), Some(83));
        assert_eq!(charge_limit_from_ui(-1.0), None);
        assert_eq!(charge_limit_from_ui(39.0), None);
        assert_eq!(charge_limit_from_ui(101.0), None);
        assert_eq!(charge_limit_from_ui(f32::NAN), None);
        assert_eq!(charge_limit_from_ui(f32::INFINITY), None);
        assert_eq!(charge_limit_from_ui(80.5), None); // дробное не усекается
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
        // helper применяет outcome.state.percent, не входное значение.
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
                None, // unknown bounds допустимы
            )
            .expect("valid")),
        );

        assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
        assert_eq!(s.charge_limit, 60);
        assert!(s.charge_limit_enabled);
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
            Ok(ChargeLimit::new(false, None, None).expect("valid")),
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
}
