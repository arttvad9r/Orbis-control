//! Локальное состояние UI для визуального прототипа.
//!
//! Источник данных — mock-профиль `zephyrus-full` из `orbis-test-support`
//! (публичный API). После загрузки UI работает полностью in-process:
//! кнопки меняют только локальное состояние интерфейса, никаких аппаратных
//! вызовов, D-Bus, sysfs и sessiond здесь нет.

use orbis_core::fan::FanId;
use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;

/// Отображаемое состояние главного окна.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiState {
    /// Выбранный профиль производительности: 0=Silent, 1=Balanced, 2=Turbo.
    pub perf_selected: i32,
    /// Выбранный GPU-режим: 0=Eco, 1=Standard, 2=Ultimate, 3=Optimized.
    pub gpu_selected: i32,
    /// Ultimate ожидает перезагрузки (pending reboot).
    pub gpu_ultimate_pending: bool,
    /// Ultimate недоступен (например, MUX unavailable).
    pub gpu_ultimate_disabled: bool,
    /// Ошибка backend в GPU-секции (остальное окно остаётся рабочим).
    pub gpu_section_error: bool,
    /// Лимит зарядки, %.
    pub charge_limit: i32,
    /// Телеметрия.
    pub cpu_temp: i32,
    pub gpu_temp: i32,
    pub cpu_fan_rpm: i32,
    pub gpu_fan_rpm: i32,
    pub battery_percent: i32,
    pub power_ac_mw: i32,
    /// Служебная информация.
    pub version: String,
    pub mock_profile: String,
}

impl UiState {
    /// Начальное состояние из mock-профиля `zephyrus-full`.
    ///
    /// Используется существующий публичный API `orbis-test-support::devices::build_state`;
    /// `MockProvider` для прототипа не требуется.
    pub fn from_mock_profile(profile_name: &str) -> Self {
        let state =
            orbis_test_support::devices::build_state(profile_name).expect("mock profile exists");

        let perf_selected = match state.profile {
            PerformanceProfile::Silent => 0,
            PerformanceProfile::Balanced => 1,
            PerformanceProfile::Turbo => 2,
        };

        let gpu_selected = match state.gpu_mode {
            GpuMode::Eco => 0,
            GpuMode::Standard => 1,
            GpuMode::Ultimate => 2,
            GpuMode::Optimized => 3,
        };

        let charge_limit = state
            .charge_limit
            .percent
            .map(|p| i32::from(p.get()))
            .unwrap_or(80);

        let cpu_temp = state
            .telemetry
            .cpu_temp
            .map(|t| i32::from(t.get()))
            .unwrap_or(0);
        let gpu_temp = state
            .telemetry
            .gpu_temp
            .map(|t| i32::from(t.get()))
            .unwrap_or(0);

        let mut cpu_fan_rpm = 0;
        let mut gpu_fan_rpm = 0;
        for f in &state.telemetry.fans {
            let rpm = i32::from(f.rpm.get());
            match &f.fan {
                FanId::Cpu => cpu_fan_rpm = rpm,
                FanId::Gpu => gpu_fan_rpm = rpm,
                _ => {}
            }
        }

        let battery_percent = state
            .telemetry
            .battery
            .map(|b| i32::from(b.percent.get()))
            .unwrap_or(0);
        let power_ac_mw = state
            .telemetry
            .power
            .ac
            .map(|m| m.get() as i32)
            .unwrap_or(0);

        Self {
            perf_selected,
            gpu_selected,
            gpu_ultimate_pending: false,
            gpu_ultimate_disabled: false,
            gpu_section_error: false,
            charge_limit,
            cpu_temp,
            gpu_temp,
            cpu_fan_rpm,
            gpu_fan_rpm,
            battery_percent,
            power_ac_mw,
            version: "0.1.0".to_string(),
            mock_profile: profile_name.to_string(),
        }
    }
}

/// Локальное действие пользователя.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiAction {
    /// Клик по карточке профиля (0..=2).
    Perf(i32),
    /// Клик по карточке GPU (0..=3).
    Gpu(i32),
    /// Изменение ползунка лимита зарядки.
    Charge(f32),
}

/// Применить локальное действие к состоянию (только память).
pub fn apply(state: &mut UiState, action: UiAction) {
    match action {
        UiAction::Perf(i) if (0..=2).contains(&i) => {
            state.perf_selected = i;
        }
        UiAction::Gpu(i) if (0..=3).contains(&i) => {
            state.gpu_selected = i;
            // Ultimate после нажатия показывает pending reboot;
            // выбор другого режима снимает pending.
            state.gpu_ultimate_pending = i == 2 && !state.gpu_ultimate_disabled;
        }
        UiAction::Charge(v) => {
            let raw = v.round() as i32;
            let clamped = raw.clamp(40, 100);
            // шаг 5 от 40: 40, 45, ..., 100
            let snapped = 40 + ((clamped - 40 + 2) / 5) * 5;
            state.charge_limit = snapped.clamp(40, 100);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_from_zephyrus() {
        let s = UiState::from_mock_profile("zephyrus-full");
        assert_eq!(s.perf_selected, 1); // Balanced
        assert_eq!(s.gpu_selected, 1); // Standard
        assert_eq!(s.charge_limit, 80);
        assert!(s.cpu_temp > 0);
        assert!(s.cpu_fan_rpm > 0);
        assert!(s.gpu_fan_rpm > 0);
        assert_eq!(s.power_ac_mw, 28_000);
        assert_eq!(s.mock_profile, "zephyrus-full");
    }

    #[test]
    fn perf_switch_local() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Perf(2));
        assert_eq!(s.perf_selected, 2);
        apply(&mut s, UiAction::Perf(9)); // вне диапазона — игнор
        assert_eq!(s.perf_selected, 2);
    }

    #[test]
    fn ultimate_sets_pending() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(2));
        assert_eq!(s.gpu_selected, 2);
        assert!(s.gpu_ultimate_pending);
        // выбор другого режима снимает pending
        apply(&mut s, UiAction::Gpu(0));
        assert_eq!(s.gpu_selected, 0);
        assert!(!s.gpu_ultimate_pending);
    }

    #[test]
    fn ultimate_disabled_stays_off_pending() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.gpu_ultimate_disabled = true;
        apply(&mut s, UiAction::Gpu(2));
        assert_eq!(s.gpu_selected, 2);
        assert!(!s.gpu_ultimate_pending);
    }

    #[test]
    fn charge_snap_to_step() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(43.0));
        assert_eq!(s.charge_limit, 45);
        apply(&mut s, UiAction::Charge(99.0));
        assert_eq!(s.charge_limit, 100);
        apply(&mut s, UiAction::Charge(10.0));
        assert_eq!(s.charge_limit, 40);
    }
}
