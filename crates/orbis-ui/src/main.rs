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

// UiAction::Perf больше не конструируется в production: Performance Mode идёт
// через worker. Контроллер пока используется для GPU/Battery (синхронно).
#[allow(dead_code)]
mod controller;

use std::cell::{Cell, OnceCell};
use std::rc::Rc;
use std::sync::Arc;

use orbis_application::{AppService, CommandError, PerformanceCommandOutcome};
use orbis_core::action::ApplyResult;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::mock::MockProvider;
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

/// Высота окна: в состоянии error добавляется баннер GPU-ошибки.
fn window_height(state: &controller::UiState) -> f32 {
    // высота клиентской области без внутреннего titlebar (34px удалены)
    if state.gpu_section_error {
        406.0
    } else {
        381.0
    }
}

// ---------------------------------------------------------------------------
// Маппинг controller::UiState <-> сгенерированный Slint UiState
// ---------------------------------------------------------------------------

fn to_slint(state: &controller::UiState) -> UiState {
    UiState {
        perf_selected: state.perf_selected,
        available_perf_mask: state.available_perf_mask,
        gpu_selected: state.gpu_selected,
        available_gpu_mask: state.available_gpu_mask,
        gpu_ultimate_pending: state.gpu_ultimate_pending,
        gpu_ultimate_disabled: state.gpu_ultimate_disabled,
        gpu_section_error: state.gpu_section_error,
        charge_limit: state.charge_limit,
        charge_limit_enabled: state.charge_limit_enabled,
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
        gpu_selected: state.gpu_selected,
        available_gpu_mask: state.available_gpu_mask,
        gpu_ultimate_pending: state.gpu_ultimate_pending,
        gpu_ultimate_disabled: state.gpu_ultimate_disabled,
        gpu_section_error: state.gpu_section_error,
        charge_limit: state.charge_limit,
        charge_limit_enabled: state.charge_limit_enabled,
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
    perf_tx: Option<UnboundedSender<WorkerCommand>>,
) -> Result<AppWindow, slint::PlatformError> {
    let app = AppWindow::new()?;
    app.set_ui_state(to_slint(state));
    wire_callbacks(&app, perf_tx);
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

/// Применить authoritative Performance-результат к UI-состоянию.
///
/// Обновляются только Performance-поля (selected и маска доступности);
/// GPU/Battery/pending/error поля сохраняются без изменений.
fn apply_performance_outcome(state: &mut controller::UiState, outcome: &PerformanceCommandOutcome) {
    state.perf_selected = perf_selected_index(outcome.state.current);
    state.available_perf_mask = performance_available_mask(&outcome.state.available);
}

/// Применить событие worker-а к UI-состоянию.
///
/// Ok -> authoritative state; Err (Command/ReadBack) -> UiState не изменяется,
/// ошибка сохраняется в диагностическом журнале (UI-отображение ошибок — отдельный
/// микрошаг).
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
        // Временная совместимость: production UI пока отправляет только
        // Performance-команды, а публичный WorkerEvent уже содержит GPU.
        WorkerEvent::Gpu(_) => {
            tracing::debug!("GPU worker event ignored until GPU UI wiring is added");
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
/// Performance: отправляет типизированную команду в worker (async результат
/// вернётся через event sink); controller::apply(UiAction::Perf) НЕ вызывается.
/// GPU/Battery: читают самый свежий ui-state из AppWindow и используют
/// существующий controller::apply (синхронно).
fn wire_callbacks(app: &AppWindow, perf_tx: Option<UnboundedSender<WorkerCommand>>) {
    {
        let perf_tx = perf_tx;
        app.on_perf_clicked(move |i| {
            let Some(profile) = performance_profile_from_index(i) else {
                tracing::warn!("perf-clicked с неизвестным индексом: {i}");
                return;
            };
            match &perf_tx {
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
        let weak = app.as_weak();
        app.on_gpu_clicked(move |i| {
            if let Some(app) = weak.upgrade() {
                let mut s = from_slint(&app.get_ui_state());
                controller::apply(&mut s, controller::UiAction::Gpu(i));
                app.set_ui_state(to_slint(&s));
            }
        });
    }
    {
        let weak = app.as_weak();
        app.on_charge_changed(move |v| {
            if let Some(app) = weak.upgrade() {
                let mut s = from_slint(&app.get_ui_state());
                controller::apply(&mut s, controller::UiAction::Charge(v));
                app.set_ui_state(to_slint(&s));
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

fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let state = state_for_scenario(&args.ui_state);

    if let Some(path) = args.screenshot {
        return render_screenshot(&state, &path);
    }

    // Интерактивный запуск через штатный winit-бэкенд (без set_platform).
    // Один явный многопоточный runtime для worker-задачи.
    let runtime = tokio::runtime::Runtime::new()?;

    // Mock provider и application service (только интерактивный путь).
    let mock_state = build_state_arc("zephyrus-full")
        .ok_or_else(|| anyhow::anyhow!("mock profile 'zephyrus-full' отсутствует"))?;
    let provider = Arc::new(MockProvider::new(mock_state));
    let service = AppService::new(provider);
    let (perf_tx, perf_rx) = orbis_ui::worker::command_channel();

    let app = build_app(&state, Some(perf_tx.clone()))?;
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
    runtime.spawn(run_worker(service, perf_rx, event_sink));

    app.show()?;
    slint::run_event_loop()?;

    // Завершение: освобождаем клоны sender (в callbacks), локальный sender и
    // runtime. Weak в worker-е не удерживает окно живым; после drop(app) sender
    // закрыт -> worker завершается.
    drop(app);
    drop(perf_tx);
    drop(runtime);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_state() -> controller::UiState {
        controller::UiState::from_mock_profile("zephyrus-full")
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
}
