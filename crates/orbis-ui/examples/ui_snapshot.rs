//! Offscreen section snapshots for the single-window UI (ui-review companion).
//!
//! Usage: `cargo run -p orbis-ui --example ui_snapshot -- <section> [path] [theme] [width] [height] [state]`
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
    state.available_gpu_mask = 0b0111;
    state.gpu_queued = -1;
    state.gpu_reboot_required = false;

    state.charge_limit_state = ChargeLimitState::Ready;
    state.charge_limit_writable = true;
    state.charge_limit = 80;

    state.cpu_temp = "56°C".into();
    state.gpu_temp = "51°C".into();
    state.cpu_temp_value = 56;
    state.gpu_temp_value = 51;
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

    state.power_limits_ready = true;
    state.power_limits_writable = true;
    state.spl_unit = "W".into();
    state.spl_value = 45;
    state.spl_min = 15;
    state.spl_max = 80;
    state.spl_step = 1;
    state.sppt_unit = "W".into();
    state.sppt_value = 65;
    state.sppt_min = 15;
    state.sppt_max = 90;
    state.sppt_step = 1;
    state.fppt_unit = "W".into();
    state.fppt_value = 80;
    state.fppt_min = 15;
    state.fppt_max = 100;
    state.fppt_step = 1;
    state.gpu_dynamic_boost_unit = "W".into();
    state.gpu_dynamic_boost_value = 25;
    state.gpu_dynamic_boost_min = 5;
    state.gpu_dynamic_boost_max = 25;
    state.gpu_dynamic_boost_step = 1;
    component.set_ui_state(state);

    component.set_cpu_tuning_ready(true);
    component.set_cpu_epp_supported(true);
    component.set_cpu_epp_writable(true);
    component.set_cpu_epp(2);
    component.set_cpu_boost_known(true);
    component.set_cpu_boost(true);
    component.set_cpu_boost_writable(true);
    component.set_cpu_co_supported(true);
    component.set_cpu_co_writable(true);
    component.set_profile_limits_known(true);
    component.set_profile_limits_auto_apply(true);
    component.set_power_rules_known(true);
    component.set_power_rules_enabled(true);
    component.set_power_rules_ac(3);
    component.set_power_rules_battery(1);
    component.set_panel_refresh_known(true);
    component.set_panel_refresh_rates(slint::ModelRc::new(slint::VecModel::from(vec![60, 144])));
    component.set_panel_refresh_current(144);
    component.set_power_rules_ac_hz(144);
    component.set_power_rules_battery_hz(60);
    component.set_panel_brightness_known(true);
    component.set_panel_brightness(70);
    component.set_nvidia_visible(true);
    component.set_nvidia_writable(true);
    component.set_nvidia_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        NvidiaRow {
            field: 0,
            label: "Частота ядра".into(),
            unit: "МГц".into(),
            current: 100,
            min: -200,
            max: 300,
            default_known: true,
            default: 0,
            saved_known: false,
            saved: 0,
        },
        NvidiaRow {
            field: 1,
            label: "Частота памяти".into(),
            unit: "МГц".into(),
            current: 200,
            min: -500,
            max: 1000,
            default_known: true,
            default: 0,
            saved_known: false,
            saved: 0,
        },
        NvidiaRow {
            field: 2,
            label: "Лимит мощности".into(),
            unit: "W".into(),
            current: 115,
            min: 60,
            max: 140,
            default_known: true,
            default: 115,
            saved_known: false,
            saved: 0,
        },
    ])));
    component.set_aura_rainbow_supported(true);
    component.set_aura_breathe_supported(true);
    component.set_aura_static_supported(true);
    component.set_aura_star_supported(true);

    demo_history(component);
    component.set_cpu_model("AMD Ryzen 7 7735HS".into());
    component.set_cpu_detail("16 потоков".into());
    component.set_gpu_model("GeForce RTX 4060".into());
    component.set_memory_total("16 ГБ".into());
    component.set_kernel_release("6.16.8-arch1-1".into());
    component.set_display_resolution("1920 × 1080".into());
    component.set_battery_health_value(89);
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
    component.set_boot_sound_control_ready(true);
    component.set_boot_sound(true);
    component.set_aspm_state_ready(true);
    component.set_aspm_control_ready(true);
    component.set_disable_aspm(false);
    component.set_igpu_memory_state_ready(true);
    component.set_igpu_memory_control_ready(true);
    component.set_igpu_memory(2);
    component.set_auto_clamshell_state(ClamshellState::Inactive);
    component.set_backend_ready(true);
    component.set_status("".into());

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

/// Deterministic telemetry history so sparklines and the temperature chart
/// render as they do after a couple of minutes of real polling.
fn demo_history(component: &AppWindow) {
    use orbis_ui::sparkline::{CHART, Range, SPARK, line_chart_svg};
    let wave = |base: f32, amp: f32, phase: f32, n: usize| -> Vec<i32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / 6.0 + phase;
                (base + amp * (t.sin() * 0.7 + (t * 2.3).cos() * 0.3) + i as f32 * amp / 40.0)
                    .round() as i32
            })
            .collect()
    };
    let image = |values: &[i32], range: Range, canvas| {
        line_chart_svg(values, range, canvas)
            .and_then(|svg| slint::Image::load_from_svg_data(svg.as_bytes()).ok())
            .unwrap_or_default()
    };
    let cpu = wave(52.0, 4.0, 0.0, 60);
    let gpu = wave(46.0, 3.0, 1.3, 60);
    let auto = Range::Auto { min_span: 8.0 };
    component.set_cpu_temp_spark(image(&cpu, auto, SPARK));
    component.set_gpu_temp_spark(image(&gpu, auto, SPARK));
    component.set_power_spark(image(
        &wave(23.0, 3.0, 2.1, 60),
        Range::Auto { min_span: 6.0 },
        SPARK,
    ));
    component.set_battery_spark(image(
        &(0..60).map(|i| 70 + i / 8).collect::<Vec<_>>(),
        Range::Auto { min_span: 4.0 },
        SPARK,
    ));
    component.set_cpu_fan_spark(image(
        &wave(2300.0, 180.0, 0.4, 60),
        Range::Auto { min_span: 600.0 },
        SPARK,
    ));
    component.set_gpu_fan_spark(image(
        &wave(2100.0, 160.0, 2.4, 60),
        Range::Auto { min_span: 600.0 },
        SPARK,
    ));
    component.set_cpu_fan_level(2300.0 / 6000.0);
    component.set_gpu_fan_level(2100.0 / 6000.0);
    let axis = Range::Fixed {
        min: 20.0,
        max: 100.0,
    };
    component.set_cpu_temp_chart(image(&cpu, axis, CHART));
    component.set_gpu_temp_chart(image(&gpu, axis, CHART));
    component.set_history_span("−2 мин".into());
}

fn apply_snapshot_state(
    component: &AppWindow,
    section: &str,
    state_name: &str,
) -> anyhow::Result<()> {
    let applicable = match state_name {
        "normal" => true,
        "dirty" => section == "cooling",
        "pending" | "error" | "unsupported" | "readonly" => matches!(
            section,
            "dashboard" | "performance" | "cooling" | "graphics"
        ),
        other => anyhow::bail!("unknown snapshot state: {other}"),
    };
    if !applicable {
        anyhow::bail!("snapshot state {state_name:?} is not applicable to section {section:?}");
    }

    let mut ui_state = component.get_ui_state();
    match (section, state_name) {
        (_, "normal") => {}
        ("cooling", "dirty") => ui_state.fan_curve_dirty = true,
        ("cooling", "pending") => ui_state.fan_curve_pending = true,
        ("cooling", "error") => ui_state.fan_curve_error = true,
        ("cooling", "unsupported") => {
            ui_state.fan_curve_state = FanCurveHwState::Unavailable;
            ui_state.fan_curve_writable = false;
            ui_state.fan_curve_unavailable_reason =
                "Поддержка кривой вентиляторов не обнаружена".into();
        }
        ("cooling", "readonly") => ui_state.fan_curve_writable = false,
        ("performance", "pending") => ui_state.performance_delegated_pending = true,
        ("performance", "error") => {
            ui_state.performance_delegated_error =
                "Ошибка чтения профиля производительности".into();
        }
        ("dashboard", "pending") => {
            ui_state.perf_state = PerformanceHwState::Unavailable;
            ui_state.perf_writable = false;
            ui_state.perf_unavailable_reason = "Ожидание подтверждения".into();
        }
        ("dashboard", "error") => {
            ui_state.perf_state = PerformanceHwState::Unavailable;
            ui_state.perf_writable = false;
            ui_state.perf_unavailable_reason = "Ошибка чтения профиля производительности".into();
        }
        ("performance", "unsupported") | ("dashboard", "unsupported") => {
            ui_state.perf_state = PerformanceHwState::Unavailable;
            ui_state.perf_writable = false;
            ui_state.perf_unavailable_reason = "Поддержка профиля не обнаружена".into();
        }
        ("performance", "readonly") | ("dashboard", "readonly") => ui_state.perf_writable = false,
        ("graphics", "pending") => ui_state.gpu_mode_pending = true,
        ("graphics", "error") => ui_state.gpu_section_error = true,
        ("graphics", "unsupported") => {
            ui_state.gpu_mode_state = GpuModeHwState::Unavailable;
            ui_state.gpu_mode_writable = false;
        }
        ("graphics", "readonly") => ui_state.gpu_mode_writable = false,
        _ => unreachable!("applicability checked above"),
    }
    component.set_ui_state(ui_state);
    Ok(())
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
    let requested_width = args.next().map(|value| value.parse()).transpose()?;
    let requested_height = args.next().map(|value| value.parse()).transpose()?;
    let state = args.next().unwrap_or_else(|| "normal".to_string());
    if args.next().is_some() {
        anyhow::bail!("too many arguments");
    }
    let light = match theme.as_str() {
        "dark" => false,
        "light" => true,
        other => anyhow::bail!("unknown theme: {other}"),
    };

    let (default_width, default_height) = if kind == "dialog" {
        (470, 228)
    } else {
        (1240, 820)
    };
    let width = requested_width.unwrap_or(default_width);
    let height = requested_height.unwrap_or(default_height);

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
            apply_snapshot_state(&component, &kind, &state)?;
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
