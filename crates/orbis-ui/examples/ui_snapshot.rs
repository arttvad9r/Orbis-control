use std::cell::{Cell, OnceCell};
use std::rc::Rc;

use slint::platform::{Platform, PlatformError, Renderer, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalSize, PhysicalSize, Rgb8Pixel, WindowSize};

slint::include_modules!();

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
        let (logical, physical) = match size {
            WindowSize::Physical(p) => (p.to_logical(1.0), p),
            WindowSize::Logical(l) => (l, l.to_physical(1.0)),
        };
        self.size.set(physical);
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

fn setup(width: u32, height: u32) -> Rc<slint::platform::software_renderer::SoftwareRenderer> {
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
    renderer
}

fn save(
    renderer: &slint::platform::software_renderer::SoftwareRenderer,
    window: &slint::Window,
    path: &str,
) -> anyhow::Result<()> {
    let size = window.size();
    let (width, height) = (size.width as usize, size.height as usize);
    let mut buffer = vec![Rgb8Pixel::new(0, 0, 0); width * height];
    renderer.render(&mut buffer, width);
    let mut raw = Vec::with_capacity(width * height * 3);
    for pixel in &buffer {
        raw.extend_from_slice(&[pixel.r, pixel.g, pixel.b]);
    }
    image::save_buffer(
        path,
        &raw,
        width as u32,
        height as u32,
        image::ColorType::Rgb8,
    )?;
    println!("snapshot: {path} ({width}x{height})");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let kind = args.next().unwrap_or_else(|| "extra".to_string());
    let path = args.next().unwrap_or_else(|| format!("{kind}.png"));
    let theme = args.next().unwrap_or_else(|| "dark".to_string());
    let light = match theme.as_str() {
        "dark" => false,
        "light" => true,
        other => anyhow::bail!("unknown theme: {other}"),
    };

    let (width, height) = match kind.as_str() {
        "main" => (500, 680),
        "fans" => (760, 590),
        "extra" => (560, 760),
        "automation" => (700, 600),
        "preferences" => (580, 550),
        "diagnostics" => (640, 540),
        "updates" => (470, 350),
        "dialog" => (430, 220),
        other => anyhow::bail!("unknown window: {other}"),
    };

    let renderer = setup(width, height);

    match kind.as_str() {
        "main" => {
            let component = AppWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "fans" => {
            let component = FansWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            let mut state = component.get_ui_state();
            state.fan_curve_state = FanCurveHwState::Ready;
            state.fan_curve_writable = true;
            state.fan_curve_dirty = true;
            state.fan_selected = 0;
            state.fan_profile_selected = 0;
            state.fan_temp_0 = 30;
            state.fan_temp_1 = 40;
            state.fan_temp_2 = 50;
            state.fan_temp_3 = 60;
            state.fan_temp_4 = 70;
            state.fan_temp_5 = 80;
            state.fan_temp_6 = 90;
            state.fan_temp_7 = 100;
            state.fan_pwm_0 = 0;
            state.fan_pwm_1 = 24;
            state.fan_pwm_2 = 48;
            state.fan_pwm_3 = 72;
            state.fan_pwm_4 = 104;
            state.fan_pwm_5 = 144;
            state.fan_pwm_6 = 196;
            state.fan_pwm_7 = 255;
            component.set_ui_state(state);
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "extra" => {
            let component = ExtraWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "automation" => {
            let component = AutomationWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "preferences" => {
            let component = PreferencesWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "diagnostics" => {
            let component = DiagnosticsWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component.set_version_value("0.1.0-audit".into());
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "updates" => {
            let component = UpdatesWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component.set_version("0.1.0-audit".into());
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        "dialog" => {
            let component = PreviewDialogWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            component.set_kind(3);
            component
                .window()
                .set_size(LogicalSize::new(width as f32, height as f32));
            component.show()?;
            save(&renderer, component.window(), &path)?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
