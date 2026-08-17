//! Локальное состояние UI для визуального прототипа.
//!
//! Источник данных — mock-профиль `zephyrus-full` из `orbis-test-support`
//! (публичный API). После загрузки UI работает полностью in-process:
//! кнопки меняют только локальное состояние интерфейса, никаких аппаратных
//! вызовов, системных интерфейсов и фоновых демонов здесь нет.

use orbis_core::capability::CapabilityStatus;
use orbis_core::fan::FanId;
use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;

/// Состояние capability (read-only).
///
/// Отображает CapabilityStatus из registry snapshot, различая между:
/// - Supported / ReadOnly (capability доступна)
/// - Unsupported / BackendMissing / TemporarilyUnavailable / PermissionDenied / Unknown
///   (capability недоступна по разным причинам)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapabilityAvailability {
    /// Capability поддерживается.
    Supported,
    /// Capability существует, но только read-only (write запрещена).
    ReadOnly,
    /// Capability не поддерживается.
    Unsupported,
    /// Required backend/service отсутствует.
    BackendMissing,
    /// Capability временно недоступна.
    TemporarilyUnavailable,
    /// Недостаточно прав для capability.
    PermissionDenied,
    /// Unknown capability status.
    #[default]
    Unknown,
}

impl CapabilityAvailability {
    /// Construct from CapabilityStatus.
    pub fn from_status(status: CapabilityStatus) -> Self {
        match status {
            CapabilityStatus::Supported | CapabilityStatus::SupportedWithRequirement => {
                Self::Supported
            }
            CapabilityStatus::ReadOnly => Self::ReadOnly,
            CapabilityStatus::Unsupported => Self::Unsupported,
            CapabilityStatus::BackendMissing => Self::BackendMissing,
            CapabilityStatus::TemporarilyUnavailable => Self::TemporarilyUnavailable,
            CapabilityStatus::PermissionDenied => Self::PermissionDenied,
            CapabilityStatus::Experimental
            | CapabilityStatus::Conflicted
            | CapabilityStatus::Unknown => Self::Unknown,
        }
    }
}

/// Разрешена ли мутация при данном write-operation статусе.
///
/// Только `Supported` и `SupportedWithRequirement` открывают mutation control.
/// Все остальные статусы (`ReadOnly`, `Unsupported`, `BackendMissing`,
/// `TemporarilyUnavailable`, `PermissionDenied`, `Unknown`) — disabled.
pub fn write_allows_mutation(status: CapabilityStatus) -> bool {
    matches!(
        status,
        CapabilityStatus::Supported | CapabilityStatus::SupportedWithRequirement
    )
}

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

/// Состояние product GPU Mode (Eco/Standard/Ultimate/Optimized).
///
/// `Ready` — есть authoritative product mode (пока mock/offscreen path).
/// `Unavailable` — управление product mode недоступно (production: реального
/// backend нет, только read-only hardware status Power/MUX/Access).
/// Отделено от `gpu_mode_writable` (write-capability): production запрещает и
/// показ authoritative selected, и изменение.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuModeHwState {
    /// Authoritative product mode ещё не получен.
    #[default]
    Loading,
    /// Authoritative product mode доступен (mock/offscreen).
    Ready,
    /// Управление product mode недоступно (production).
    Unavailable,
}

/// Состояние готовности/доступности fan curve.
///
/// Отделено от `fan_curve_writable` (write-capability): read-only backend
/// при Ready всё равно не позволяет запись; mock/offscreen могут сохранять
/// writable behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FanCurveHwState {
    /// Первый authoritative read ещё не выполнен.
    #[default]
    Loading,
    /// Authoritative read успешен (curve points известны).
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
    /// Состояние product GPU Mode (Eco/Standard/Ultimate/Optimized).
    pub gpu_mode_state: GpuModeHwState,
    /// Можно ли менять product GPU Mode (write-capability).
    ///
    /// Отдельно от `gpu_mode_state`: production запрещает mutation, пока
    /// доказанный product-mode backend отсутствует.
    pub gpu_mode_writable: bool,
    /// Configured/reported charge threshold, % (не показывать при state != Ready).
    pub charge_limit: i32,
    /// Состояние UPower `ChargeThresholdEnabled`.
    pub charge_limit_enabled: bool,
    /// Можно ли применять Battery Charge Limit (write-capability).
    ///
    /// Отдельно от `charge_limit_writable`: `false` не запрещает asusd/kernel
    /// mutation, поскольку это независимая UPower policy state.
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
    /// Capability availability: Performance Mode read/write support.
    pub perf_capability: CapabilityAvailability,
    /// Capability availability: Battery Charge Limit read/write support.
    pub charge_limit_capability: CapabilityAvailability,
    /// Capability availability: GPU Power state read support.
    pub gpu_power_capability: CapabilityAvailability,
    /// Capability availability: GPU MUX state read support.
    pub gpu_mux_capability: CapabilityAvailability,
    /// Capability availability: GPU Access Policy read support.
    pub gpu_access_capability: CapabilityAvailability,
    /// Телеметрия (authoritative значения из `SysfsTelemetryProvider`).
    ///
    /// Хранятся как display-строки: "—" = значение ещё не получено или
    /// отсутствует у backend. Production не показывает mock/placeholder
    /// значения до первого refresh.
    pub cpu_temp: String,
    /// Температура dGPU, °C ("—" = неизвестно).
    pub gpu_temp: String,
    /// CPU fan RPM ("—" = неизвестно).
    pub cpu_fan_rpm: String,
    /// GPU fan RPM ("—" = неизвестно).
    pub gpu_fan_rpm: String,
    /// Battery percent, % ("—" = неизвестно).
    pub battery_percent: String,
    /// Battery health (capacity), % от design ("—" = неизвестно).
    pub battery_health: String,
    /// Число циклов заряда ("—" = неизвестно).
    pub battery_cycles: String,
    /// Battery status ("Charging"/"Discharging"/"Full"/...) (пусто = неизвестно).
    pub battery_status: String,
    /// Подключён ли AC-адаптер ("On AC"/"On battery"/"—").
    pub ac_online: String,
    /// Потребление dGPU, Вт (telemetry, не `GpuPowerState`; "—" = неизвестно).
    pub gpu_power_display: String,
    /// Потребление от AC, Вт ("—" = неизвестно/не вычисляется).
    pub power_ac: String,
    /// Служебная информация.
    pub version: String,
    pub mock_profile: String,

    // ── Fan Curve Editor ──────────────────────────────────────────────
    /// Состояние готовности fan curve (Loading/Ready/Unavailable).
    pub fan_curve_state: FanCurveHwState,
    /// Можно ли применять fan curve (write-capability из FanCurves.operations.write).
    pub fan_curve_writable: bool,
    /// Capability availability: Fan Curves read/write support.
    pub fan_curve_capability: CapabilityAvailability,
    /// Выбранный вентилятор: 0=CPU, 1=GPU.
    pub fan_selected: i32,
    /// Выбранный lossless asusd профиль: 0=Balanced, 1=Performance, 2=Quiet, 3=LowPower.
    pub fan_profile_selected: i32,
    /// 8 температур точек кривой (°C).
    pub fan_curve_temps: [i32; 8],
    /// 8 raw PWM точек кривой (0..255).
    pub fan_curve_pwms: [i32; 8],
    /// Ошибка backend в fan-секции (остальное окно остаётся рабочим).
    pub fan_curve_error: bool,
    /// Есть ли несохранённые изменения ( dirty flag для UI кнопки Apply).
    pub fan_curve_dirty: bool,
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
            .configured_percent
            .map(|p| i32::from(p.get()))
            .unwrap_or(80);
        let charge_limit_enabled = state.charge_limit.enabled;

        let cpu_temp = format_celsius(state.telemetry.cpu_temp);
        let gpu_temp = format_celsius(state.telemetry.gpu_temp);

        let mut cpu_fan_rpm = None;
        let mut gpu_fan_rpm = None;
        for f in &state.telemetry.fans {
            let rpm = f.rpm;
            match &f.fan {
                FanId::Cpu => cpu_fan_rpm = Some(rpm),
                FanId::Gpu => gpu_fan_rpm = Some(rpm),
                _ => {}
            }
        }
        let cpu_fan_rpm = format_rpm(cpu_fan_rpm);
        let gpu_fan_rpm = format_rpm(gpu_fan_rpm);

        let battery_percent = state.telemetry.battery.as_ref().map(|b| b.percent);
        let battery_health = state.telemetry.battery.as_ref().and_then(|b| b.capacity);
        let battery_cycles = state
            .telemetry
            .battery
            .as_ref()
            .and_then(|b| b.charge_cycles.map(|c| c as i32));
        let battery_status = state
            .telemetry
            .battery
            .as_ref()
            .map(|b| b.state.clone())
            .unwrap_or_default();
        let ac_online = match state.telemetry.ac_online {
            Some(true) => "On AC".to_string(),
            Some(false) => "On battery".to_string(),
            None => "—".to_string(),
        };
        let gpu_power = format_watts(state.telemetry.power.gpu);
        let power_ac = format_watts(state.telemetry.power.ac);

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
            // mock/offscreen: product GPU mode готов и writable (fake interactive
            // semantics); production interactive выставляет Unavailable + writable=false
            // отдельно в main().
            gpu_mode_state: GpuModeHwState::Ready,
            gpu_mode_writable: true,
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
            perf_capability: CapabilityAvailability::Unknown,
            charge_limit_capability: CapabilityAvailability::Unknown,
            gpu_power_capability: CapabilityAvailability::Unknown,
            gpu_mux_capability: CapabilityAvailability::Unknown,
            gpu_access_capability: CapabilityAvailability::Unknown,
            cpu_temp,
            gpu_temp,
            cpu_fan_rpm,
            gpu_fan_rpm,
            battery_percent: format_percent(battery_percent),
            battery_health: format_percent(battery_health),
            battery_cycles: battery_cycles
                .map(|c| c.to_string())
                .unwrap_or_else(|| "—".into()),
            battery_status,
            ac_online,
            gpu_power_display: gpu_power,
            power_ac,
            version: "0.1.0".to_string(),
            mock_profile: profile_name.to_string(),
            // Fan Curve Editor: Loading до первого authoritative refresh;
            // mock profile не предоставляет real fan curve hardware state.
            fan_curve_state: FanCurveHwState::Loading,
            fan_curve_writable: false,
            fan_curve_capability: CapabilityAvailability::Unknown,
            fan_selected: 0,         // CPU
            fan_profile_selected: 0, // Balanced
            fan_curve_temps: [0; 8],
            fan_curve_pwms: [0; 8],
            fan_curve_error: false,
            fan_curve_dirty: false,
        }
    }

    /// Update capability availability from a registry snapshot.
    ///
    /// Called when the registry is refreshed (generation changes) or initially
    /// published. Maps each capability's `CapabilityStatus` to
    /// `CapabilityAvailability` and derives mutation gating from the
    /// `operations.write.status` of Performance and ChargeLimit.
    ///
    /// Observed state (current profile, battery values, GPU states) is never
    /// touched: capability metadata and observed values stay separate.
    pub fn update_capabilities(
        &mut self,
        snapshot: &orbis_capabilities::CapabilityRegistrySnapshot,
    ) {
        use orbis_core::FeatureId;

        if let Some(cap) = snapshot.capability(FeatureId::Performance) {
            self.perf_capability = CapabilityAvailability::from_status(cap.status);
            self.perf_writable = write_allows_mutation(cap.operations.write.status);
        }
        if let Some(cap) = snapshot.capability(FeatureId::ChargeLimit) {
            self.charge_limit_capability = CapabilityAvailability::from_status(cap.status);
            self.charge_limit_writable = write_allows_mutation(cap.operations.write.status);
        }
        if let Some(cap) = snapshot.capability(FeatureId::GpuPower) {
            self.gpu_power_capability = CapabilityAvailability::from_status(cap.status);
        }
        if let Some(cap) = snapshot.capability(FeatureId::GpuMux) {
            self.gpu_mux_capability = CapabilityAvailability::from_status(cap.status);
        }
        if let Some(cap) = snapshot.capability(FeatureId::GpuAccess) {
            self.gpu_access_capability = CapabilityAvailability::from_status(cap.status);
        }
        if let Some(cap) = snapshot.capability(FeatureId::FanCurves) {
            self.fan_curve_capability = CapabilityAvailability::from_status(cap.status);
            self.fan_curve_writable = write_allows_mutation(cap.operations.write.status);
        }
    }

    /// Сбросить все telemetry-поля в неизвестное состояние.
    ///
    /// Используется production interactive startup: до первого authoritative
    /// `Telemetry` refresh mock fixture значения не должны отображаться.
    pub fn reset_telemetry(&mut self) {
        self.cpu_temp = "—".into();
        self.gpu_temp = "—".into();
        self.cpu_fan_rpm = "—".into();
        self.gpu_fan_rpm = "—".into();
        self.battery_percent = "—".into();
        self.battery_health = "—".into();
        self.battery_cycles = "—".into();
        self.battery_status.clear();
        self.ac_online = "—".into();
        self.gpu_power_display = "—".into();
        self.power_ac = "—".into();
    }

    /// Обновить telemetry-поля из authoritative `Telemetry` snapshot.
    ///
    /// Каждое поле обновляется независимо: отсутствующие (None) поля
    /// становятся "—", но не ломают остальные значения. Ошибка snapshot-а
    /// обновление не вызывает (caller сохраняет последний успешный state).
    pub fn update_telemetry(&mut self, telemetry: &orbis_core::telemetry::Telemetry) {
        use orbis_core::fan::FanId;

        self.cpu_temp = format_celsius(telemetry.cpu_temp);
        self.gpu_temp = format_celsius(telemetry.gpu_temp);

        let mut cpu_fan_rpm = None;
        let mut gpu_fan_rpm = None;
        for f in &telemetry.fans {
            let rpm = f.rpm;
            match &f.fan {
                FanId::Cpu => cpu_fan_rpm = Some(rpm),
                FanId::Gpu => gpu_fan_rpm = Some(rpm),
                _ => {}
            }
        }
        self.cpu_fan_rpm = format_rpm(cpu_fan_rpm);
        self.gpu_fan_rpm = format_rpm(gpu_fan_rpm);

        self.battery_percent = format_percent(telemetry.battery.as_ref().map(|b| b.percent));
        self.battery_health = format_percent(telemetry.battery.as_ref().and_then(|b| b.capacity));
        self.battery_cycles = telemetry
            .battery
            .as_ref()
            .and_then(|b| b.charge_cycles.map(|c| c as i32))
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".into());
        self.battery_status = telemetry
            .battery
            .as_ref()
            .map(|b| b.state.clone())
            .unwrap_or_default();
        self.ac_online = match telemetry.ac_online {
            Some(true) => "On AC".to_string(),
            Some(false) => "On battery".to_string(),
            None => "—".to_string(),
        };
        self.gpu_power_display = format_watts(telemetry.power.gpu);
        self.power_ac = format_watts(telemetry.power.ac);
    }

    // ── Fan Curve helpers ─────────────────────────────────────────────

    /// Map UI fan index (0=CPU, 1=GPU) to domain `FanId`.
    pub fn fan_id_from_index(index: i32) -> Option<FanId> {
        match index {
            0 => Some(FanId::Cpu),
            1 => Some(FanId::Gpu),
            _ => None,
        }
    }

    /// Map UI profile index to lossless `AsusdFanProfile`.
    pub fn asusd_profile_from_index(index: i32) -> Option<orbis_core::profile::AsusdFanProfile> {
        use orbis_core::profile::AsusdFanProfile;
        match index {
            0 => Some(AsusdFanProfile::Balanced),
            1 => Some(AsusdFanProfile::Performance),
            2 => Some(AsusdFanProfile::Quiet),
            3 => Some(AsusdFanProfile::LowPower),
            _ => None,
        }
    }

    /// Load authoritative `FanCurve` into editor state.
    ///
    /// Points are truncated/padded to exactly 8. Missing points become 0.
    pub fn load_fan_curve(&mut self, curve: &orbis_core::fan::FanCurve) {
        self.fan_curve_state = FanCurveHwState::Ready;
        self.fan_curve_error = false;
        // Map fan id to index
        self.fan_selected = match &curve.fan {
            FanId::Cpu => 0,
            FanId::Gpu => 1,
            _ => self.fan_selected,
        };
        // Map profile to index
        use orbis_core::profile::PerformanceProfile;
        self.fan_profile_selected = match curve.profile {
            PerformanceProfile::Balanced => 0,
            PerformanceProfile::Turbo => 1,
            PerformanceProfile::Silent => 2,
        };
        // Fill 8 temp/pwm arrays from curve points
        let mut temps = [0i32; 8];
        let mut pwms = [0i32; 8];
        for (i, pt) in curve.points.iter().take(8).enumerate() {
            temps[i] = pt.temp.get() as i32;
            pwms[i] = pt.pwm.get() as i32;
        }
        self.fan_curve_temps = temps;
        self.fan_curve_pwms = pwms;
        self.fan_curve_dirty = false;
    }

    /// Build `FanCurvePoints` from editor state for mutation.
    ///
    /// Returns `None` if any temp/pwm value is out of the valid newtype range.
    pub fn build_fan_curve_points(&self) -> Option<orbis_providers::traits::FanCurvePoints> {
        use orbis_core::newtypes::{FanPwm, TemperatureC};
        let mut temps = [TemperatureC::new(0).ok()?; 8];
        let mut pwms = [FanPwm::new(0).ok()?; 8];
        for i in 0..8 {
            temps[i] = TemperatureC::new(self.fan_curve_temps[i] as i16).ok()?;
            pwms[i] = FanPwm::new(self.fan_curve_pwms[i] as u8).ok()?;
        }
        Some(orbis_providers::traits::FanCurvePoints { temps, pwms })
    }

    /// Check if fan curve mutation is allowed (writable + not dirty + valid).
    ///
    /// Used as Rust-side guard: disabled or invalid curves never send mutation.
    pub fn fan_curve_can_mutate(&self) -> bool {
        if !self.fan_curve_writable {
            return false;
        }
        if self.fan_curve_error {
            return false;
        }
        if !self.fan_curve_dirty {
            return false;
        }
        // Validate via domain FanCurve::validate
        let Some(points) = self.build_fan_curve_points() else {
            return false;
        };
        use orbis_core::fan::{FanCurve, FanCurvePoint};
        let curve = FanCurve {
            profile: PerformanceProfile::Silent, // profile doesn't affect validate
            fan: FanId::Cpu,
            points: points
                .temps
                .iter()
                .zip(points.pwms.iter())
                .map(|(t, p)| FanCurvePoint::new(*t, *p))
                .collect(),
        };
        curve.validate(8, false).is_ok()
    }
}

/// Отформатировать температуру: °C или "—".
fn format_celsius(t: Option<orbis_core::newtypes::TemperatureC>) -> String {
    match t {
        Some(t) => format!("{}°C", t.get()),
        None => "—".into(),
    }
}

/// Отформатировать RPM или "—".
fn format_rpm(rpm: Option<orbis_core::newtypes::Rpm>) -> String {
    match rpm {
        Some(rpm) => format!("{} rpm", rpm.get()),
        None => "—".into(),
    }
}

/// Отформатировать процент или "—".
fn format_percent(percent: Option<orbis_core::newtypes::Percent>) -> String {
    match percent {
        Some(p) => format!("{}%", p.get()),
        None => "—".into(),
    }
}

/// Отформатировать мощность: мВт → Вт или "—".
fn format_watts(mw: Option<orbis_core::newtypes::MilliWatt>) -> String {
    match mw {
        Some(mw) => format!("{} W", mw.get() / 1000),
        None => "—".into(),
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
            if integral && (20..=100).contains(&iv) {
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
        assert!(s.cpu_temp != "—");
        assert!(s.cpu_fan_rpm != "—");
        assert!(s.gpu_fan_rpm != "—");
        assert_eq!(s.power_ac, "28 W");
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
    fn charge_80_to_20() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(20.0));
        assert_eq!(s.charge_limit, 20);
    }

    #[test]
    fn charge_20_to_100() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(20.0));
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
        apply(&mut s, UiAction::Charge(19.0));
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
    fn charge_integer_values_are_accepted() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        apply(&mut s, UiAction::Charge(21.0));
        apply(&mut s, UiAction::Charge(83.0));
        assert_eq!(s.charge_limit, 83);
    }

    #[test]
    fn charge_disabled_does_not_block_mutation() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.charge_limit_enabled = false;
        apply(&mut s, UiAction::Charge(60.0));
        assert_eq!(s.charge_limit, 60);
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

    // -----------------------------------------------------------------------
    // Telemetry display + authoritative update
    // -----------------------------------------------------------------------

    fn sample_telemetry() -> orbis_core::telemetry::Telemetry {
        orbis_core::telemetry::Telemetry {
            cpu_temp: Some(orbis_core::newtypes::TemperatureC::new(46).unwrap()),
            gpu_temp: Some(orbis_core::newtypes::TemperatureC::new(43).unwrap()),
            fans: vec![
                orbis_core::telemetry::FanTelemetry {
                    fan: orbis_core::fan::FanId::Cpu,
                    rpm: orbis_core::newtypes::Rpm::new(2600).unwrap(),
                    percent: None,
                },
                orbis_core::telemetry::FanTelemetry {
                    fan: orbis_core::fan::FanId::Gpu,
                    rpm: orbis_core::newtypes::Rpm::new(2100).unwrap(),
                    percent: None,
                },
            ],
            power: orbis_core::telemetry::PowerTelemetry {
                ac: None,
                battery: None,
                total: None,
                gpu: Some(orbis_core::newtypes::MilliWatt::new(13_073).unwrap()),
            },
            ac_online: Some(true),
            battery: Some(orbis_core::telemetry::BatteryTelemetry {
                percent: orbis_core::newtypes::Percent::new(100).unwrap(),
                capacity: Some(orbis_core::newtypes::Percent::new(87).unwrap()),
                energy_now: None,
                energy_full: None,
                charge_cycles: Some(5),
                state: "Full".into(),
            }),
            gpu_power_state: orbis_core::gpu::GpuPowerState::Unknown,
            ts: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn mock_profile_formats_telemetry() {
        let s = UiState::from_mock_profile("zephyrus-full");
        // mock fixture: 72°C CPU, 65°C GPU, fans (2400/2600 rpm), 80% battery, 28 W AC.
        assert_eq!(s.cpu_temp, "72°C");
        assert_eq!(s.gpu_temp, "65°C");
        assert_eq!(s.cpu_fan_rpm, "2400 rpm");
        assert_eq!(s.gpu_fan_rpm, "2600 rpm");
        assert_eq!(s.battery_percent, "80%");
        assert_eq!(s.battery_health, "89%");
        assert_eq!(s.battery_cycles, "0");
        assert_eq!(s.battery_status, "discharging");
        assert_eq!(s.ac_online, "On AC");
        assert_eq!(s.power_ac, "28 W");
    }

    #[test]
    fn update_telemetry_formats_authoritative_values() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.reset_telemetry();
        s.update_telemetry(&sample_telemetry());

        assert_eq!(s.cpu_temp, "46°C");
        assert_eq!(s.gpu_temp, "43°C");
        assert_eq!(s.cpu_fan_rpm, "2600 rpm");
        assert_eq!(s.gpu_fan_rpm, "2100 rpm");
        assert_eq!(s.battery_percent, "100%");
        assert_eq!(s.battery_health, "87%");
        assert_eq!(s.battery_cycles, "5");
        assert_eq!(s.battery_status, "Full");
        assert_eq!(s.ac_online, "On AC");
        assert_eq!(s.gpu_power_display, "13 W");
        assert_eq!(s.power_ac, "—");
    }

    #[test]
    fn reset_telemetry_clears_mock_values() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        assert_ne!(s.cpu_temp, "—");
        s.reset_telemetry();
        assert_eq!(s.cpu_temp, "—");
        assert_eq!(s.gpu_temp, "—");
        assert_eq!(s.cpu_fan_rpm, "—");
        assert_eq!(s.gpu_fan_rpm, "—");
        assert_eq!(s.battery_percent, "—");
        assert_eq!(s.battery_health, "—");
        assert_eq!(s.battery_cycles, "—");
        assert_eq!(s.battery_status, "");
        assert_eq!(s.ac_online, "—");
        assert_eq!(s.gpu_power_display, "—");
        assert_eq!(s.power_ac, "—");
    }

    #[test]
    fn missing_telemetry_fields_do_not_break_others() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.reset_telemetry();

        let mut t = sample_telemetry();
        // Убираем CPU temp, fans и battery — остальные поля должны обновиться.
        t.cpu_temp = None;
        t.fans.clear();
        t.battery = None;
        t.ac_online = None;
        s.update_telemetry(&t);

        assert_eq!(s.cpu_temp, "—");
        assert_eq!(s.gpu_temp, "43°C"); // независимо от отсутствия cpu_temp
        assert_eq!(s.cpu_fan_rpm, "—");
        assert_eq!(s.gpu_fan_rpm, "—");
        assert_eq!(s.battery_percent, "—");
        assert_eq!(s.battery_status, "");
        assert_eq!(s.ac_online, "—");
        assert_eq!(s.gpu_power_display, "13 W"); // power.gpu не зависит от battery
    }

    #[test]
    fn telemetry_does_not_touch_gpu_capability_state() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.gpu_power = GpuHwState::Ready;
        s.gpu_power_value = 1;
        let before = s.clone();

        s.update_telemetry(&sample_telemetry());

        // GPU capability state (power/mux/access) не изменяется telemetry.
        assert_eq!(s.gpu_power, before.gpu_power);
        assert_eq!(s.gpu_mux, before.gpu_mux);
        assert_eq!(s.gpu_access, before.gpu_access);
        assert_eq!(s.gpu_power_value, before.gpu_power_value);
        assert_eq!(s.gpu_mux_value, before.gpu_mux_value);
        assert_eq!(s.gpu_access_value, before.gpu_access_value);
        // Perf/charge/gpu-selected поля не затрагиваются.
        assert_eq!(s.perf_selected, before.perf_selected);
        assert_eq!(s.charge_limit, before.charge_limit);
        assert_eq!(s.gpu_selected, before.gpu_selected);
    }
}
