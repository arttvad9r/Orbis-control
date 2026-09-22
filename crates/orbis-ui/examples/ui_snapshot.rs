//! Offscreen section snapshots for the single-window UI (ui-review companion).
//!
//! Usage: `cargo run -p orbis-ui --example ui_snapshot -- <section> [path] [theme]`
//! Sections: dashboard | performance | power | cooling | graphics | backlight
//! | display | system | settings | about | dialog.

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

/// Product-complete review state. It is intentionally richer than production
/// startup state so visual review judges the finished surface rather than a
/// collection of unavailable placeholders. This example is never shipped.
fn demo_state(component: &AppWindow) {
    let mut state = component.get_ui_state();
    state.perf_state = PerformanceHwState::Ready;
    state.perf_writable = true;
    state.perf_selected = 1;
    state.available_perf_mask = 0b0111;

    state.gpu_mode_state = GpuModeHwState::Ready;
    state.gpu_mode_writable = true;
    state.gpu_selected = 1;
    state.available_gpu_mask = 0b1111;
    state.gpu_queued = -1;
    state.gpu_reboot_required = false;

    state.charge_limit_state = ChargeLimitState::Ready;
    state.charge_limit_writable = true;
    state.charge_limit = 80;

    state.cpu_temp = "56°C".into();
    state.gpu_temp = "51°C".into();
    state.cpu_fan_rpm = "2300 rpm".into();
    state.gpu_fan_rpm = "2100 rpm".into();
    state.battery_percent = "78%".into();
    state.battery_percent_value = 78;
    state.battery_health = "89%".into();
    state.battery_cycles = "142".into();
    state.battery_status = "Charging".into();
    state.ac_online = "On AC".into();
    state.power_ac_mw = "23 W".into();
    state.gpu_power = "8 W".into();
    state.telemetry_fresh = true;

    state.gpu_power_state = GpuHwState::Ready;
    state.gpu_mux_state = GpuHwState::Ready;
    state.gpu_access_state = GpuHwState::Ready;
    state.gpu_power_value = 0;
    state.gpu_mux_value = 0;
    state.gpu_access_value = 0;

    state.fan_curve_state = FanCurveHwState::Ready;
    state.fan_curve_writable = true;
    state.fan_curve_dirty = false;
    state.fan_curve_enabled_known = true;
    state.fan_curve_enabled = true;
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

    component.set_device_name("ASUS TUF Gaming A17 FA707NV".into());
    component.set_device_board("FA707NV".into());
    component.set_bios_version("FA707NV.318".into());
    component.set_bios_date("2026-07-14".into());

    component.set_keyboard_state_ready(true);
    component.set_keyboard_control_ready(true);
    component.set_keyboard_brightness(2);
    component.set_aura_state_ready(true);
    component.set_aura_control_ready(true);
    component.set_keyboard_effect(0);
    component.set_keyboard_speed(1);

    component.set_display_state_ready(true);
    component.set_display_status("1920×1080 · 144 Hz".into());
    component.set_panel_overdrive_state_ready(true);
    component.set_panel_overdrive_control_ready(true);
    component.set_panel_overdrive(true);

    component.set_boot_sound_state_ready(true);
    component.set_boot_sound(true);
    component.set_backend_ready(true);
    component.set_status("Состояние расширенных параметров синхронизировано".into());
    component.set_status_led(true);
    component.set_disable_aspm(false);
    component.set_disable_standby_networking(false);
    component.set_igpu_memory(2);
    component.set_hibernate_after(30);
    component.set_p_cores(8);
    component.set_e_cores(0);
    component.set_m1_action(2);
    component.set_m2_action(3);
    component.set_m3_action(4);
    component.set_m4_action(7);
    component.set_m5_action(8);

    component.set_startup(true);
    component.set_startup_enabled(true);
    component.set_startup_status("Автозапуск включён".into());
    component.set_start_minimized(false);
    component.set_start_minimized_enabled(true);
    component.set_remember_position(true);
    component.set_remember_position_enabled(true);
    component.set_close_action(1);
    component.set_close_action_enabled(true);
    component.set_hide_to_tray_enabled(true);
    component.set_settings_local_status("Настройки сохранены".into());

    component.set_refresh_enabled(true);
    component.set_refresh_pending(false);
    component.set_export_enabled(true);
    component.set_diagnostics_summary("Orbis diagnostics review state".into());
    component.set_diagnostics_status("Диагностика актуальна".into());
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
    let kind = args.next().unwrap_or_else(|| "dashboard".to_string());
    let path = args.next().unwrap_or_else(|| format!("{kind}.png"));
    let theme = args.next().unwrap_or_else(|| "dark".to_string());
    let light = match theme.as_str() {
        "dark" => false,
        "light" => true,
        other => anyhow::bail!("unknown theme: {other}"),
    };

    let (width, height) = match kind.as_str() {
        "dialog" => (470, 228),
        _ => (1240, 820),
    };

    let renderer = setup(width, height);

    match kind.as_str() {
        "dashboard" | "performance" | "power" | "fans" | "cooling" | "graphics" | "backlight"
        | "display" | "extra" | "system" | "settings" | "about" => {
            let component = AppWindow::new()?;
            component.global::<ThemeState>().set_mode(if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            });
            demo_state(&component);
            let section = match kind.as_str() {
                "performance" => Section::Performance,
                "power" => Section::Power,
                "fans" | "cooling" => Section::Cooling,
                "graphics" => Section::Graphics,
                "backlight" => Section::Backlight,
                "display" => Section::Display,
                "extra" | "system" => Section::System,
                "settings" => Section::Settings,
                "about" => Section::About,
                _ => Section::Dashboard,
            };
            component.set_active_section(section);
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
        other => anyhow::bail!("unknown section: {other}"),
    }

    Ok(())
}
