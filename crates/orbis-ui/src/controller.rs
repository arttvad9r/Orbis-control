//! Локальное состояние UI для визуального прототипа.
//!
//! Источник данных — mock-профиль `zephyrus-full` из `orbis-test-support`
//! (публичный API). После загрузки UI работает полностью in-process:
//! кнопки меняют только локальное состояние интерфейса, никаких аппаратных
//! вызовов, системных интерфейсов и фоновых демонов здесь нет.

use orbis_core::fan::FanId;
use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;

/// Состояние готовности/доступности Battery Charge Limit.
///
/// Отделено от `charge_limit_enabled` (фактический hardware/backend state):
/// `Enabled` может быть и при Unavailable (недоступен backend), и не является
/// признаком known/unknown значения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChargeLimitState {
    /// Первый authoritative read ещё не выполнен (initial interactive state).
    #[default]
    Loading,
    /// Authoritative read успешен и процент известен.
    Ready,
    /// Backend/read недоступен; значение не должно показываться как
    /// authoritative hardware state.
    Unavailable,
}

/// Состояние готовности read-only GPU hardware capability.
///
/// Domain `Unknown` является валидным `Ready` значением (backend сообщил
/// semantic unknown), а не `Unavailable`. `Unavailable` — только backend/read
/// error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuHwState {
    /// Первый authoritative read ещё не выполнен.
    #[default]
    Loading,
    /// Authoritative read успешен (значение может быть `Unknown`).
    Ready,
    /// Backend/read недоступен.
    Unavailable,
}

/// Состояние готовности read-only Performance Mode.
///
/// `Ready` означает получен authoritative current + available. `Unavailable` —
/// только backend/read error. Отделено от `perf_writable` (write-capability):
/// read-only session backend в production не позволяет запись.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PerformanceHwState {
    /// Первый authoritative read ещё не выполнен.
    #[default]
    Loading,
    /// Authoritative read успешен (current + available известны).
    Ready,
    /// Backend/read недоступен.
    Unavailable,
}

/// Отображаемое состояние главного окна.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiState {
    /// Выбранный профиль производительности: 0=Silent, 1=Balanced, 2=Turbo.
    pub perf_selected: i32,
    /// Битовая маска доступных профилей (bit0=Silent, bit1=Balanced, bit2=Turbo).
    pub available_perf_mask: i32,
    /// Состояние готовности read-only Performance Mode.
    pub perf_state: PerformanceHwState,
    /// Можно ли применять Performance profile (write-capability).
    ///
    /// Отдельно от `perf_state`: read-only session backend при Ready всё равно
    /// не позволяет запись; mock/offscreen могут сохранять writable behavior.
    pub perf_writable: bool,
    /// Выбранный GPU-режим: 0=Eco, 1=Standard, 2=Ultimate, 3=Optimized.
    pub gpu_selected: i32,
    /// Битовая маска доступных GPU-режимов (bit0=Eco, bit1=Standard,
    /// bit2=Ultimate, bit3=Optimized).
    pub available_gpu_mask: i32,
    /// Ultimate ожидает применения после перезагрузки (pending state).
    pub gpu_ultimate_pending: bool,
    /// Ultimate недоступен (например, MUX unavailable).
    pub gpu_ultimate_disabled: bool,
    /// Ошибка backend в GPU-секции (остальное окно остаётся рабочим).
    pub gpu_section_error: bool,
    /// Лимит зарядки, % (authoritative value; не показывать при state != Ready).
    pub charge_limit: i32,
    /// Функция Battery Charge Limit доступна (из mock-состояния).
    pub charge_limit_enabled: bool,
    /// Можно ли применять Battery Charge Limit (write-capability).
    ///
    /// Отдельно от `charge_limit_enabled` (hardware/backend state): read-only
    /// session backend при `enabled=true` всё равно не позволяет запись.
    pub charge_limit_writable: bool,
    /// Состояние готовности/доступности Battery Charge Limit.
    pub charge_limit_state: ChargeLimitState,
    /// Read-only GPU hardware capability: dGPU power state.
    pub gpu_power: GpuHwState,
    /// Read-only GPU hardware capability: physical MUX state.
    pub gpu_mux: GpuHwState,
    /// Read-only GPU hardware capability: dGPU access policy.
    pub gpu_access: GpuHwState,
    /// Значение dGPU power state (0=Active,1=Suspended,2=Off,4=Unknown).
    pub gpu_power_value: i32,
    /// Значение MUX state (0=Integrated,1=Discrete,2=Unknown).
    pub gpu_mux_value: i32,
    /// Значение access policy (0=Unblocked,1=Blocked,2=Pending,3=Unknown).
    pub gpu_access_value: i32,
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

/// Явное исчерпывающее сопоставление GPU-режима с индексом кнопки
/// (без wildcard-ветки, чтобы добавление новых режимов было заметным).
fn gpu_index(m: GpuMode) -> i32 {
    match m {
        GpuMode::Eco => 0,
        GpuMode::Standard => 1,
        GpuMode::Ultimate => 2,
        GpuMode::Optimized => 3,
    }
}

/// Явное исчерпывающее сопоставление профиля с индексом кнопки
/// (без wildcard-ветки, чтобы добавление новых режимов было заметным).
fn perf_index(p: PerformanceProfile) -> i32 {
    match p {
        PerformanceProfile::Silent => 0,
        PerformanceProfile::Balanced => 1,
        PerformanceProfile::Turbo => 2,
    }
}

impl UiState {
    /// Начальное состояние из mock-профиля `zephyrus-full`.
    ///
    /// Используется существующий публичный API `orbis-test-support::devices::build_state`;
    /// `MockProvider` для прототипа не требуется.
    pub fn from_mock_profile(profile_name: &str) -> Self {
        let state =
            orbis_test_support::devices::build_state(profile_name).expect("mock profile exists");

        let perf_selected = perf_index(state.profile);
        let mut available_perf_mask = 0;
        for p in &state.profiles {
            available_perf_mask |= 1 << perf_index(*p);
        }

        let gpu_selected = gpu_index(state.gpu_mode);

        // Доступность GPU-режимов из mock-состояния: Standard доступен всегда
        // (гибрид), Eco/Ultimate/Optimized — только при наличии физического MUX.
        let mut available_gpu_mask = 0b0010; // Standard
        if state.mux != orbis_core::gpu::GpuMuxState::Unknown {
            available_gpu_mask |= 0b1101; // Eco | Ultimate | Optimized
        }

        let charge_limit = state
            .charge_limit
            .percent
            .map(|p| i32::from(p.get()))
            .unwrap_or(80);
        let charge_limit_enabled = state.charge_limit.enabled;

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
            available_perf_mask,
            // mock/offscreen: готово сразу и writable (fake interactive
            // semantics); production interactive выставляет Loading + writable=false
            // отдельно в main().
            perf_state: PerformanceHwState::Ready,
            perf_writable: true,
            gpu_selected,
            available_gpu_mask,
            gpu_ultimate_pending: false,
            gpu_ultimate_disabled: false,
            gpu_section_error: false,
            charge_limit,
            charge_limit_enabled,
            // mock/offscreen/tests могут применять лимит (fake interactive semantics);
            // production interactive выставляет writable=false отдельно в main().
            charge_limit_writable: true,
            // fixture-профиль: первое значение готово сразу (offscreen/tests).
            charge_limit_state: ChargeLimitState::Ready,
            // GPU hardware capabilities: Loading до первого authoritative read;
            // mock profile не предоставляет real hardware states.
            gpu_power: GpuHwState::Loading,
            gpu_mux: GpuHwState::Loading,
            gpu_access: GpuHwState::Loading,
            gpu_power_value: 0,
            gpu_mux_value: 0,
            gpu_access_value: 0,
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
            // Только симуляция: меняем локальное UI-состояние; провайдеры и
            // оборудование не вызываются (временный in-process срез).
            if state.available_perf_mask & (1 << i) != 0 {
                state.perf_selected = i;
            }
        }
        UiAction::Gpu(i) if (0..=3).contains(&i) => {
            // Только симуляция: меняем локальное запрошенное состояние; MUX,
            // доступ приложений и power state не изменяются, провайдеры не
            // вызываются (временный in-process срез).
            let available = state.available_gpu_mask & (1 << i) != 0;
            let not_disabled_ultimate = !(i == 2 && state.gpu_ultimate_disabled);
            if available && not_disabled_ultimate {
                state.gpu_selected = i;
                // Ultimate ожидает применения (pending); applied-режим не меняется.
                state.gpu_ultimate_pending = i == 2;
            }
        }
        UiAction::Charge(v) => {
            // Только симуляция: обновляем локальное UI-состояние; аппаратный
            // лимит заряда не применяется (временный in-process срез).
            let iv = v.round() as i32;
            let integral = (v - iv as f32).abs() < 1e-3;
            if integral
                && state.charge_limit_enabled
                && (40..=100).contains(&iv)
                && (iv - 40) % 5 == 0
            {
                state.charge_limit = iv;
            }
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
    fn initial_state_battery_is_ready() {
        // fixture-профиль: значение готово сразу (offscreen/tests).
        let s = UiState::from_mock_profile("zephyrus-full");
        assert_eq!(s.charge_limit_state, ChargeLimitState::Ready);
        assert_eq!(s.charge_limit, 80);
        assert!(s.charge_limit_enabled);
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
    fn initial_zephyrus_is_balanced() {
        let s = UiState::from_mock_profile("zephyrus-full");
        assert_eq!(s.perf_selected, 1); // Balanced
        assert_eq!(s.available_perf_mask, 0b111);
    }

    #[test]
    fn balanced_to_silent() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Perf(0));
        assert_eq!(s.perf_selected, 0);
    }

    #[test]
    fn silent_to_turbo() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Perf(0));
        apply(&mut s, UiAction::Perf(2));
        assert_eq!(s.perf_selected, 2);
    }

    #[test]
    fn turbo_to_balanced() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Perf(2));
        apply(&mut s, UiAction::Perf(1));
        assert_eq!(s.perf_selected, 1);
    }

    #[test]
    fn repeated_selection_is_idempotent() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Perf(1));
        let before = s.clone();
        apply(&mut s, UiAction::Perf(1));
        assert_eq!(s, before); // состояние не изменилось, ошибки нет
        assert_eq!(s.perf_selected, 1);
    }

    #[test]
    fn unsupported_profile_is_rejected() {
        // non-asus: доступны только Balanced и Turbo (Silent отсутствует в маске).
        let mut s = UiState::from_mock_profile("non-asus");
        assert_eq!(s.available_perf_mask, 0b110);
        let before = s.clone();
        apply(&mut s, UiAction::Perf(0)); // Silent недоступен
        assert_eq!(s, before);
        assert_eq!(s.perf_selected, 1);
    }

    #[test]
    fn profile_bit_mapping() {
        // Silent = bit 0, Balanced = bit 1, Turbo = bit 2 (исчерпывающее сопоставление).
        assert_eq!(perf_index(PerformanceProfile::Silent), 0);
        assert_eq!(perf_index(PerformanceProfile::Balanced), 1);
        assert_eq!(perf_index(PerformanceProfile::Turbo), 2);
        assert_eq!(1 << perf_index(PerformanceProfile::Silent), 0b001);
        assert_eq!(1 << perf_index(PerformanceProfile::Balanced), 0b010);
        assert_eq!(1 << perf_index(PerformanceProfile::Turbo), 0b100);
    }

    #[test]
    fn rust_to_slint_preserves_mask() {
        // Конвертация Rust -> Slint сохраняет маску 0b111 для zephyrus-full.
        let s = UiState::from_mock_profile("zephyrus-full");
        let slint_state = crate::to_slint(&s);
        assert_eq!(slint_state.available_perf_mask, 0b111);
        assert_eq!(slint_state.perf_selected, 1); // Balanced
    }

    #[test]
    fn disabled_by_mask_is_rejected() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.available_perf_mask = 0b001; // доступен только Silent
        apply(&mut s, UiAction::Perf(0)); // разрешён
        assert_eq!(s.perf_selected, 0);
        let before = s.clone();
        apply(&mut s, UiAction::Perf(1)); // Balanced недоступен -> без изменений
        assert_eq!(s, before);
        assert_eq!(s.perf_selected, 0);
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
    fn ultimate_disabled_is_rejected() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.gpu_ultimate_disabled = true;
        let before = s.clone();
        apply(&mut s, UiAction::Gpu(2));
        assert_eq!(s, before); // disabled Ultimate не меняет состояние
        assert_eq!(s.gpu_selected, 1);
        assert!(!s.gpu_ultimate_pending);
    }

    #[test]
    fn initial_gpu_zephyrus_is_standard() {
        let s = UiState::from_mock_profile("zephyrus-full");
        assert_eq!(s.gpu_selected, 1); // Standard из mock-состояния
        assert_eq!(s.available_gpu_mask, 0b1111);
    }

    #[test]
    fn standard_to_eco() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(0));
        assert_eq!(s.gpu_selected, 0);
        assert!(!s.gpu_ultimate_pending);
    }

    #[test]
    fn eco_to_standard() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(0));
        apply(&mut s, UiAction::Gpu(1));
        assert_eq!(s.gpu_selected, 1);
        assert!(!s.gpu_ultimate_pending);
    }

    #[test]
    fn standard_to_optimized() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(3));
        assert_eq!(s.gpu_selected, 3);
        assert!(!s.gpu_ultimate_pending);
    }

    #[test]
    fn repeated_gpu_selection_is_idempotent() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(1));
        let before = s.clone();
        apply(&mut s, UiAction::Gpu(1));
        assert_eq!(s, before);
    }

    #[test]
    fn repeated_ultimate_selection_is_idempotent() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(2));
        assert_eq!(s.gpu_selected, 2);
        assert!(s.gpu_ultimate_pending);
        let before = s.clone();
        apply(&mut s, UiAction::Gpu(2));
        assert_eq!(s, before); // не создаёт новый pending state
    }

    #[test]
    fn unsupported_gpu_mode_is_rejected() {
        // non-asus: MUX отсутствует -> маска 0b0010 (только Standard)
        let mut s = UiState::from_mock_profile("non-asus");
        assert_eq!(s.available_gpu_mask, 0b0010);
        let before = s.clone();
        apply(&mut s, UiAction::Gpu(0)); // Eco недоступен
        apply(&mut s, UiAction::Gpu(2)); // Ultimate недоступен
        apply(&mut s, UiAction::Gpu(3)); // Optimized недоступен
        assert_eq!(s, before);
        assert_eq!(s.gpu_selected, 1);
    }

    #[test]
    fn gpu_error_banner_preserved_after_rejected_action() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.gpu_section_error = true;
        s.available_gpu_mask = 0b0010; // только Standard
        let before = s.clone();
        apply(&mut s, UiAction::Gpu(0)); // отклонено
        assert_eq!(s, before);
        assert!(s.gpu_section_error); // баннер не исчезает
    }

    #[test]
    fn gpu_bit_mapping() {
        assert_eq!(gpu_index(GpuMode::Eco), 0);
        assert_eq!(gpu_index(GpuMode::Standard), 1);
        assert_eq!(gpu_index(GpuMode::Ultimate), 2);
        assert_eq!(gpu_index(GpuMode::Optimized), 3);
        assert_eq!(1 << gpu_index(GpuMode::Eco), 0b0001);
        assert_eq!(1 << gpu_index(GpuMode::Standard), 0b0010);
        assert_eq!(1 << gpu_index(GpuMode::Ultimate), 0b0100);
        assert_eq!(1 << gpu_index(GpuMode::Optimized), 0b1000);
    }

    #[test]
    fn rust_to_slint_preserves_gpu_state() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(2)); // Ultimate pending
        let slint_state = crate::to_slint(&s);
        assert_eq!(slint_state.gpu_selected, 2);
        assert_eq!(slint_state.available_gpu_mask, 0b1111);
        assert!(slint_state.gpu_ultimate_pending);
    }

    #[test]
    fn initial_charge_is_80() {
        let s = UiState::from_mock_profile("zephyrus-full");
        assert_eq!(s.charge_limit, 80);
        assert!(s.charge_limit_enabled);
    }

    #[test]
    fn charge_80_to_40() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(40.0));
        assert_eq!(s.charge_limit, 40);
    }

    #[test]
    fn charge_40_to_100() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(40.0));
        apply(&mut s, UiAction::Charge(100.0));
        assert_eq!(s.charge_limit, 100);
    }

    #[test]
    fn charge_100_to_75() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(100.0));
        apply(&mut s, UiAction::Charge(75.0));
        assert_eq!(s.charge_limit, 75);
    }

    #[test]
    fn repeated_charge_is_idempotent() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(75.0));
        let before = s.clone();
        apply(&mut s, UiAction::Charge(75.0));
        assert_eq!(s, before);
        assert_eq!(s.charge_limit, 75);
    }

    #[test]
    fn charge_below_minimum_rejected() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let before = s.clone();
        apply(&mut s, UiAction::Charge(35.0));
        apply(&mut s, UiAction::Charge(39.0));
        assert_eq!(s, before);
    }

    #[test]
    fn charge_above_maximum_rejected() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let before = s.clone();
        apply(&mut s, UiAction::Charge(101.0));
        apply(&mut s, UiAction::Charge(105.0));
        assert_eq!(s, before);
    }

    #[test]
    fn charge_off_step_rejected() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let before = s.clone();
        apply(&mut s, UiAction::Charge(41.0));
        apply(&mut s, UiAction::Charge(83.0));
        assert_eq!(s, before);
    }

    #[test]
    fn charge_disabled_rejected() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.charge_limit_enabled = false;
        let before = s.clone();
        apply(&mut s, UiAction::Charge(60.0));
        assert_eq!(s, before);
        assert_eq!(s.charge_limit, 80);
    }

    #[test]
    fn charge_does_not_affect_other_sections() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Gpu(2)); // Ultimate pending
        let before = s.clone();
        apply(&mut s, UiAction::Charge(60.0));
        // Меняется только charge_limit.
        assert_eq!(s.charge_limit, 60);
        assert_eq!(s.perf_selected, before.perf_selected);
        assert_eq!(s.gpu_selected, before.gpu_selected);
        assert_eq!(s.gpu_ultimate_pending, before.gpu_ultimate_pending);
        assert_eq!(s.gpu_section_error, before.gpu_section_error);
        assert_eq!(s.cpu_temp, before.cpu_temp);
        assert_eq!(s.battery_percent, before.battery_percent);
    }

    #[test]
    fn rust_to_slint_preserves_charge() {
        let s = UiState::from_mock_profile("zephyrus-full");
        let slint_state = crate::to_slint(&s);
        assert_eq!(slint_state.charge_limit, 80);
        assert!(slint_state.charge_limit_enabled);
    }
}
