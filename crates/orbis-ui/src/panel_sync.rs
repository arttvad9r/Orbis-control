// Property mirroring and request forwarding between the main window and the
// secondary windows (one line per property/callback declared in
// ui/panels.slint). Included by panel_windows.rs and the ui_snapshot example,
// each with its own generated `AppWindow`, `FansWindow`, `ExtraWindow` and
// `ThemeState` in scope.

/// Mirror the main window state into the fans window.
pub(crate) fn sync_fans(app: &AppWindow, window: &FansWindow) {
    window
        .global::<ThemeState>()
        .set_mode(app.global::<ThemeState>().get_mode());
    window.set_ui_state(app.get_ui_state());
    window.set_cpu_boost(app.get_cpu_boost());
    window.set_cpu_boost_known(app.get_cpu_boost_known());
    window.set_cpu_boost_writable(app.get_cpu_boost_writable());
    window.set_cpu_co_applied(app.get_cpu_co_applied());
    window.set_cpu_co_applied_known(app.get_cpu_co_applied_known());
    window.set_cpu_co_supported(app.get_cpu_co_supported());
    window.set_cpu_co_writable(app.get_cpu_co_writable());
    window.set_cpu_epp(app.get_cpu_epp());
    window.set_cpu_epp_supported(app.get_cpu_epp_supported());
    window.set_cpu_epp_writable(app.get_cpu_epp_writable());
    window.set_cpu_tuning_error(app.get_cpu_tuning_error());
    window.set_cpu_tuning_ready(app.get_cpu_tuning_ready());
    window.set_factory_reset_available(app.get_factory_reset_available());
    window.set_fan_eco_profile_available(app.get_fan_eco_profile_available());
    window.set_mutation_safety_blocked(app.get_mutation_safety_blocked());
    window.set_nvidia_error(app.get_nvidia_error());
    window.set_nvidia_rows(app.get_nvidia_rows());
    window.set_nvidia_status(app.get_nvidia_status());
    window.set_nvidia_visible(app.get_nvidia_visible());
    window.set_nvidia_writable(app.get_nvidia_writable());
    window.set_policy_status(app.get_policy_status());
}

/// Forward the fans window requests to the main window handlers.
pub(crate) fn forward_fans(app: &AppWindow, window: &FansWindow) {
    {
        let app = app.as_weak();
        window.on_cpu_boost_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_cpu_boost_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_cpu_co_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_cpu_co_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_cpu_epp_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_cpu_epp_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_fan_apply_clicked(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_fan_apply_clicked(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_fan_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_fan_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_fan_profile_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_fan_profile_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_fan_pwm_point_changed(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_fan_pwm_point_changed(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_fan_temp_point_changed(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_fan_temp_point_changed(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_nvidia_tuning_requested(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_nvidia_tuning_requested(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_power_limit_apply_clicked(move || {
            if let Some(app) = app.upgrade() {
                app.invoke_power_limit_apply_clicked();
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_power_limit_changed(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_power_limit_changed(a0, a1);
            }
        });
    }
}

/// Mirror the main window state into the extra window.
pub(crate) fn sync_extra(app: &AppWindow, window: &ExtraWindow) {
    window
        .global::<ThemeState>()
        .set_mode(app.global::<ThemeState>().get_mode());
    window.set_ui_state(app.get_ui_state());
    window.set_advanced_apply_ready(app.get_advanced_apply_ready());
    window.set_applying(app.get_applying());
    window.set_aspm_control_ready(app.get_aspm_control_ready());
    window.set_aspm_state_ready(app.get_aspm_state_ready());
    window.set_aura_control_ready(app.get_aura_control_ready());
    window.set_aura_power_awake(app.get_aura_power_awake());
    window.set_aura_power_boot(app.get_aura_power_boot());
    window.set_aura_power_ready(app.get_aura_power_ready());
    window.set_aura_power_shutdown(app.get_aura_power_shutdown());
    window.set_aura_power_sleep(app.get_aura_power_sleep());
    window.set_aura_secondary_blue(app.get_aura_secondary_blue());
    window.set_aura_secondary_green(app.get_aura_secondary_green());
    window.set_aura_secondary_red(app.get_aura_secondary_red());
    window.set_aura_state_ready(app.get_aura_state_ready());
    window.set_auto_clamshell_state(app.get_auto_clamshell_state());
    window.set_backend_ready(app.get_backend_ready());
    window.set_bios_date(app.get_bios_date());
    window.set_bios_version(app.get_bios_version());
    window.set_boot_sound(app.get_boot_sound());
    window.set_boot_sound_control_ready(app.get_boot_sound_control_ready());
    window.set_boot_sound_state_ready(app.get_boot_sound_state_ready());
    window.set_close_action(app.get_close_action());
    window.set_close_action_enabled(app.get_close_action_enabled());
    window.set_cpu_boost_writable(app.get_cpu_boost_writable());
    window.set_cpu_co_writable(app.get_cpu_co_writable());
    window.set_cpu_epp_writable(app.get_cpu_epp_writable());
    window.set_cpu_model(app.get_cpu_model());
    window.set_device_name(app.get_device_name());
    window.set_diagnostics_status(app.get_diagnostics_status());
    window.set_diagnostics_summary(app.get_diagnostics_summary());
    window.set_disable_aspm(app.get_disable_aspm());
    window.set_export_enabled(app.get_export_enabled());
    window.set_gpu_model(app.get_gpu_model());
    window.set_hide_to_tray_enabled(app.get_hide_to_tray_enabled());
    window.set_igpu_memory(app.get_igpu_memory());
    window.set_igpu_memory_control_ready(app.get_igpu_memory_control_ready());
    window.set_igpu_memory_pending(app.get_igpu_memory_pending());
    window.set_igpu_memory_state_ready(app.get_igpu_memory_state_ready());
    window.set_kernel_release(app.get_kernel_release());
    window.set_keyboard_effect(app.get_keyboard_effect());
    window.set_keyboard_speed(app.get_keyboard_speed());
    window.set_keyboard_state_ready(app.get_keyboard_state_ready());
    window.set_keyboard_timeout_ac(app.get_keyboard_timeout_ac());
    window.set_keyboard_timeout_battery(app.get_keyboard_timeout_battery());
    window.set_keyboard_timeout_problem(app.get_keyboard_timeout_problem());
    window.set_keyboard_timeout_status(app.get_keyboard_timeout_status());
    window.set_lid_other_handler(app.get_lid_other_handler());
    window.set_memory_total(app.get_memory_total());
    window.set_mode_notifications(app.get_mode_notifications());
    window.set_panel_refresh_known(app.get_panel_refresh_known());
    window.set_panel_refresh_rates(app.get_panel_refresh_rates());
    window.set_power_rules_ac(app.get_power_rules_ac());
    window.set_power_rules_ac_hz(app.get_power_rules_ac_hz());
    window.set_power_rules_battery(app.get_power_rules_battery());
    window.set_power_rules_battery_hz(app.get_power_rules_battery_hz());
    window.set_power_rules_enabled(app.get_power_rules_enabled());
    window.set_power_rules_error(app.get_power_rules_error());
    window.set_power_rules_known(app.get_power_rules_known());
    window.set_profile_limits_auto_apply(app.get_profile_limits_auto_apply());
    window.set_profile_limits_known(app.get_profile_limits_known());
    window.set_profile_limits_summary(app.get_profile_limits_summary());
    window.set_refresh_enabled(app.get_refresh_enabled());
    window.set_refresh_pending(app.get_refresh_pending());
    window.set_remember_position(app.get_remember_position());
    window.set_remember_position_enabled(app.get_remember_position_enabled());
    window.set_rgb_blue(app.get_rgb_blue());
    window.set_rgb_green(app.get_rgb_green());
    window.set_rgb_red(app.get_rgb_red());
    window.set_settings_local_status(app.get_settings_local_status());
    window.set_start_minimized(app.get_start_minimized());
    window.set_start_minimized_enabled(app.get_start_minimized_enabled());
    window.set_startup_status(app.get_startup_status());
    window.set_status(app.get_status());
    window.set_update_busy(app.get_update_busy());
    window.set_update_problem(app.get_update_problem());
    window.set_update_status(app.get_update_status());
}

/// Forward the extra window requests to the main window handlers.
pub(crate) fn forward_extra(app: &AppWindow, window: &ExtraWindow) {
    {
        let app = app.as_weak();
        window.on_apply_requested(move || {
            if let Some(app) = app.upgrade() {
                app.invoke_apply_requested();
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_aspm_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_aspm_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_aura_effect_requested(move |a0, a1, a2, a3, a4, a5, a6, a7| {
            if let Some(app) = app.upgrade() {
                app.invoke_aura_effect_requested(a0, a1, a2, a3, a4, a5, a6, a7);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_aura_power_requested(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_aura_power_requested(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_auto_clamshell_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_auto_clamshell_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_boot_sound_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_boot_sound_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_close_action_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_close_action_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_diagnostics_export_requested(move || {
            if let Some(app) = app.upgrade() {
                app.invoke_diagnostics_export_requested();
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_diagnostics_refresh_requested(move || {
            if let Some(app) = app.upgrade() {
                app.invoke_diagnostics_refresh_requested();
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_igpu_memory_requested(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_igpu_memory_requested(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_keyboard_timeout_requested(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_keyboard_timeout_requested(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_mode_notifications_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_mode_notifications_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_power_rules_enabled_toggled(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_power_rules_enabled_toggled(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_power_rules_profile_requested(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_power_rules_profile_requested(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_power_rules_refresh_requested(move |a0, a1| {
            if let Some(app) = app.upgrade() {
                app.invoke_power_rules_refresh_requested(a0, a1);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_profile_limits_auto_apply_toggled(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_profile_limits_auto_apply_toggled(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_reload_requested(move || {
            if let Some(app) = app.upgrade() {
                app.invoke_reload_requested();
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_remember_position_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_remember_position_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_start_minimized_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_start_minimized_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_theme_changed(move |a0| {
            if let Some(app) = app.upgrade() {
                app.invoke_theme_changed(a0);
            }
        });
    }
    {
        let app = app.as_weak();
        window.on_update_check_requested(move || {
            if let Some(app) = app.upgrade() {
                app.invoke_update_check_requested();
            }
        });
    }
}
