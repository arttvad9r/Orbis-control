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

mod controller;

use std::cell::{Cell, OnceCell};
use std::rc::Rc;

use slint::platform::{Platform, PlatformError, Renderer, WindowAdapter, WindowEvent};
use slint::{LogicalSize, PhysicalSize, Rgb8Pixel, WindowSize};

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
        gpu_ultimate_pending: state.gpu_ultimate_pending,
        gpu_ultimate_disabled: state.gpu_ultimate_disabled,
        gpu_section_error: state.gpu_section_error,
        charge_limit: state.charge_limit,
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
        gpu_ultimate_pending: state.gpu_ultimate_pending,
        gpu_ultimate_disabled: state.gpu_ultimate_disabled,
        gpu_section_error: state.gpu_section_error,
        charge_limit: state.charge_limit,
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
fn build_app(state: &controller::UiState) -> Result<AppWindow, slint::PlatformError> {
    let app = AppWindow::new()?;
    app.set_ui_state(to_slint(state));
    wire_callbacks(&app);
    Ok(app)
}

/// Локальные изменения состояния (только память, без аппаратных вызовов).
fn wire_callbacks(app: &AppWindow) {
    {
        let weak = app.as_weak();
        app.on_perf_clicked(move |i| {
            if let Some(app) = weak.upgrade() {
                let mut s = from_slint(&app.get_ui_state());
                controller::apply(&mut s, controller::UiAction::Perf(i));
                app.set_ui_state(to_slint(&s));
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

    let app = build_app(state)?;
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
    let app = build_app(&state)?;
    app.window()
        .set_size(LogicalSize::new(425.0, window_height(&state)));
    app.show()?;
    slint::run_event_loop()?;
    Ok(())
}
