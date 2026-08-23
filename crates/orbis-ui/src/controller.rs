//! Local UI state and presentation mappings.

use orbis_core::capability::CapabilityStatus;
use orbis_core::fan::FanId;
use orbis_core::gpu::GpuMode;
use orbis_core::platform_profile::{PlatformProfileCapability, PlatformProfileTransaction};
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
    /// Independent authoritative sources disagree.
    Conflicted,
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
            CapabilityStatus::Experimental | CapabilityStatus::Unknown => Self::Unknown,
            CapabilityStatus::Conflicted => Self::Conflicted,
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

/// UI gate for the evidence-only ASUS platform-profile model.
///
/// Available choices and a current read never enable this control; only a
/// separately validated write capability can do so.
pub fn platform_profile_write_allows_mutation(capability: &PlatformProfileCapability) -> bool {
    write_allows_mutation(capability.write)
}

/// Whether the platform-profile UI should show an unconfirmed desired target.
pub fn platform_profile_has_pending(transaction: &PlatformProfileTransaction) -> bool {
    transaction.pending.is_some()
}

/// Единый user-facing disabled reason для mutation capability.
///
/// Возвращает `None`, когда mutation разрешена (Supported /
/// SupportedWithRequirement), иначе короткий нейтральный текст, точно
/// отражающий `CapabilityAvailability`. Это единственный mapping
/// `CapabilityAvailability -> reason`; каждый control использует его, не
/// дублируя логику.
pub fn mutation_unavailable_reason(availability: CapabilityAvailability) -> Option<String> {
    match availability {
        CapabilityAvailability::Supported => None,
        CapabilityAvailability::ReadOnly => Some("Read-only".to_string()),
        CapabilityAvailability::Unsupported => Some("Not supported on this system".to_string()),
        CapabilityAvailability::BackendMissing => {
            Some("Required backend is not available".to_string())
        }
        CapabilityAvailability::TemporarilyUnavailable => {
            Some("Temporarily unavailable".to_string())
        }
        CapabilityAvailability::PermissionDenied => Some("Permission denied".to_string()),
        CapabilityAvailability::Conflicted => Some("Conflicting evidence".to_string()),
        CapabilityAvailability::Unknown => Some("Availability is unknown".to_string()),
    }
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
    /// Latest capability registry generation accepted by the UI.
    pub capability_generation: u64,
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
    /// Authoritative queued ASUS product GPU target card index (-1 = none).
    ///
    /// Заполняется только из authoritative read-back `SetProductGpuMode`;
    /// никогда не из запрошенного значения.
    pub gpu_queued: i32,
    /// Authoritative evidence: queued firmware state applies at next
    /// shutdown/reboot.
    pub gpu_reboot_required: bool,
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
    /// User-facing disabled reason for Performance mutation (None when writable).
    pub perf_unavailable_reason: Option<String>,
    /// Capability availability: Battery Charge Limit read/write support.
    pub charge_limit_capability: CapabilityAvailability,
    /// User-facing disabled reason for Charge Limit mutation (None when writable).
    pub charge_limit_unavailable_reason: Option<String>,
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
    ///
    /// `telemetry_fresh` — true после успешного authoritative refresh, false
    /// после failed refresh. Когда false, отображаемые значения являются
    /// последним успешным снимком и НЕ должны выглядеть как актуальные.
    pub telemetry_fresh: bool,
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
    /// User-facing disabled reason for Fan Curve mutation (None when writable).
    pub fan_curve_unavailable_reason: Option<String>,
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
    /// Stored curve enabled-state evidence from the authoritative read.
    ///
    /// `Some(enabled)` — backend reports this stored FanCurveData as
    /// enabled/disabled; `None` — the read backend did not expose enabled state
    /// (e.g. active sysfs curve). Preserved through Session1/client per #116.
    pub fan_curve_enabled: Option<bool>,
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

/// Map an ASUS product GPU wire value to a UI mode-card index.
///
/// Hybrid renders on the Eco card (0), Integrated on Standard (1), Ultimate
/// stays Ultimate (2). `Optimized` is never produced: the ASUS Armoury
/// product API has no such mode, so unknown sentinels (`u32::MAX`) and any
/// other value map to `None` and must not overwrite UI evidence.
pub fn asus_product_gpu_index(raw: u32) -> Option<i32> {
    match raw {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        _ => None,
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
    /// Safe initial state for the interactive production UI.
    ///
    /// Every hardware value remains unknown until the worker receives an
    /// authoritative read. Screenshot and unit-test paths use
    /// [`Self::from_mock_profile`] instead.
    pub fn production_initial() -> Self {
        Self {
            capability_generation: 0,
            perf_selected: 0,
            available_perf_mask: 0,
            perf_state: PerformanceHwState::Loading,
            perf_writable: false,
            gpu_selected: 0,
            available_gpu_mask: 0,
            gpu_ultimate_pending: false,
            gpu_ultimate_disabled: true,
            gpu_section_error: false,
            gpu_mode_state: GpuModeHwState::Unavailable,
            gpu_mode_writable: false,
            gpu_queued: -1,
            gpu_reboot_required: false,
            charge_limit: 0,
            charge_limit_enabled: false,
            charge_limit_writable: false,
            charge_limit_state: ChargeLimitState::Loading,
            gpu_power: GpuHwState::Loading,
            gpu_mux: GpuHwState::Loading,
            gpu_access: GpuHwState::Loading,
            gpu_power_value: 2,
            gpu_mux_value: 2,
            gpu_access_value: 3,
            perf_capability: CapabilityAvailability::Unknown,
            perf_unavailable_reason: Some("Availability is unknown".into()),
            charge_limit_capability: CapabilityAvailability::Unknown,
            charge_limit_unavailable_reason: Some("Availability is unknown".into()),
            gpu_power_capability: CapabilityAvailability::Unknown,
            gpu_mux_capability: CapabilityAvailability::Unknown,
            gpu_access_capability: CapabilityAvailability::Unknown,
            telemetry_fresh: false,
            cpu_temp: "—".into(),
            gpu_temp: "—".into(),
            cpu_fan_rpm: "—".into(),
            gpu_fan_rpm: "—".into(),
            battery_percent: "—".into(),
            battery_health: "—".into(),
            battery_cycles: "—".into(),
            battery_status: String::new(),
            ac_online: "—".into(),
            gpu_power_display: "—".into(),
            power_ac: "—".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            mock_profile: "production".into(),
            fan_curve_state: FanCurveHwState::Loading,
            fan_curve_writable: false,
            fan_curve_capability: CapabilityAvailability::Unknown,
            fan_curve_unavailable_reason: Some("Availability is unknown".into()),
            fan_selected: 0,
            fan_profile_selected: 0,
            fan_curve_temps: [0; 8],
            fan_curve_pwms: [0; 8],
            fan_curve_error: false,
            fan_curve_dirty: false,
            fan_curve_enabled: None,
        }
    }

    /// Начальное состояние из mock-профиля (fixture-derived).
    ///
    /// Не входит в release-граф (#115): используется только unit-тестами и
    /// UI-review/скриншот сборками с фичей `ui-review`. Production startup
    /// использует [`Self::production_initial`].
    #[cfg(any(test, feature = "ui-review"))]
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
            capability_generation: 0,
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
            gpu_queued: -1,
            gpu_reboot_required: false,
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
            perf_unavailable_reason: None,
            charge_limit_capability: CapabilityAvailability::Unknown,
            charge_limit_unavailable_reason: None,
            gpu_power_capability: CapabilityAvailability::Unknown,
            gpu_mux_capability: CapabilityAvailability::Unknown,
            gpu_access_capability: CapabilityAvailability::Unknown,
            telemetry_fresh: false,
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
            fan_curve_unavailable_reason: None,
            fan_selected: 0,         // CPU
            fan_profile_selected: 0, // Balanced
            fan_curve_temps: [0; 8],
            fan_curve_pwms: [0; 8],
            fan_curve_error: false,
            fan_curve_dirty: false,
            fan_curve_enabled: None,
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
            self.perf_unavailable_reason = mutation_unavailable_reason(
                CapabilityAvailability::from_status(cap.operations.write.status),
            );
        }
        if let Some(cap) = snapshot.capability(FeatureId::ChargeLimit) {
            self.charge_limit_capability = CapabilityAvailability::from_status(cap.status);
            self.charge_limit_writable = write_allows_mutation(cap.operations.write.status);
            self.charge_limit_unavailable_reason = mutation_unavailable_reason(
                CapabilityAvailability::from_status(cap.operations.write.status),
            );
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
            self.fan_curve_unavailable_reason = mutation_unavailable_reason(
                CapabilityAvailability::from_status(cap.operations.write.status),
            );
        }
    }

    /// Apply a capability snapshot only when it is newer than the UI state.
    ///
    /// Registry generations are monotonic. Ignoring an older event prevents a
    /// delayed `Supported` snapshot from restoring write access after a newer
    /// Hardware1 disappearance snapshot removed it.
    pub fn update_capabilities_at(
        &mut self,
        generation: u64,
        snapshot: &orbis_capabilities::CapabilityRegistrySnapshot,
    ) {
        if generation <= self.capability_generation {
            return;
        }
        self.capability_generation = generation;
        self.update_capabilities(snapshot);
    }

    /// Сбросить все telemetry-поля в неизвестное состояние.
    ///
    /// Используется production interactive startup: до первого authoritative
    /// `Telemetry` refresh mock fixture значения не должны отображаться.
    pub fn reset_telemetry(&mut self) {
        self.telemetry_fresh = false;
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

    /// Пометить telemetry как stale после failed refresh.
    ///
    /// Отображаемые значения остаются последним успешным снимком, но
    /// `telemetry_fresh` становится false, чтобы UI не показывал их как
    /// актуальные.
    pub fn mark_telemetry_stale(&mut self) {
        self.telemetry_fresh = false;
    }

    /// Обновить telemetry-поля из authoritative `Telemetry` snapshot.
    ///
    /// Каждое поле обновляется независимо: отсутствующие (None) поля
    /// становятся "—", но не ломают остальные значения. Ошибка snapshot-а
    /// обновление не вызывает (caller сохраняет последний успешный state).
    pub fn update_telemetry(&mut self, telemetry: &orbis_core::telemetry::Telemetry) {
        use orbis_core::fan::FanId;

        self.telemetry_fresh = true;
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
    /// Profile is set from the provided `AsusdFanProfile` (lossless), not from
    /// `curve.profile` (which uses `PerformanceProfile` and loses Quiet/LowPower).
    pub fn load_fan_curve(
        &mut self,
        curve: &orbis_core::fan::FanCurve,
        profile: orbis_core::profile::AsusdFanProfile,
    ) {
        self.fan_curve_state = FanCurveHwState::Ready;
        self.fan_curve_error = false;
        // Map fan id to index
        self.fan_selected = match &curve.fan {
            FanId::Cpu => 0,
            FanId::Gpu => 1,
            _ => self.fan_selected,
        };
        // Map lossless asusd profile to index (lossless, preserves Quiet/LowPower)
        self.fan_profile_selected = match profile {
            orbis_core::profile::AsusdFanProfile::Balanced => 0,
            orbis_core::profile::AsusdFanProfile::Performance => 1,
            orbis_core::profile::AsusdFanProfile::Quiet => 2,
            orbis_core::profile::AsusdFanProfile::LowPower => 3,
        };
        // Fill 8 temp/pwm arrays from curve points
        let mut temps = [0i32; 8];
        let mut pwms = [0i32; 8];
        for (i, pt) in curve.points.iter().take(8).enumerate() {
            temps[i] = i32::from(pt.temp.get());
            pwms[i] = i32::from(pt.pwm.get());
        }
        self.fan_curve_temps = temps;
        self.fan_curve_pwms = pwms;
        self.fan_curve_dirty = false;
        // Preserve stored enabled-state evidence from the authoritative read (#116).
        self.fan_curve_enabled = curve.enabled;
    }

    /// Build `FanCurvePoints` from editor state for mutation.
    ///
    /// Returns `None` if any temp/pwm value cannot be represented by the wire
    /// primitive or violates the domain newtype range. Narrowing is checked
    /// before constructing newtypes so values such as PWM 256 cannot wrap to 0.
    pub fn build_fan_curve_points(&self) -> Option<orbis_providers::traits::FanCurvePoints> {
        use orbis_core::newtypes::{FanPwm, TemperatureC};
        let mut temps = [TemperatureC::new(0).ok()?; 8];
        let mut pwms = [FanPwm::new(0).ok()?; 8];
        for i in 0..8 {
            let temp = i16::try_from(self.fan_curve_temps[i]).ok()?;
            let pwm = u8::try_from(self.fan_curve_pwms[i]).ok()?;
            temps[i] = TemperatureC::new(temp).ok()?;
            pwms[i] = FanPwm::new(pwm).ok()?;
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
            enabled: None,
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
    fn platform_profile_ui_keeps_unvalidated_write_disabled_and_shows_pending() {
        let capability = orbis_core::PlatformProfileCapability::from_observations(
            None,
            None,
            vec![
                orbis_core::PlatformProfile::Quiet,
                orbis_core::PlatformProfile::Balanced,
                orbis_core::PlatformProfile::Performance,
            ],
        );
        let transaction = orbis_core::PlatformProfileTransaction::from_values(
            orbis_core::DesiredValue::Set(orbis_core::PlatformProfile::Performance),
            orbis_core::ObservedValue::Known(orbis_core::PlatformProfile::Balanced),
        );
        assert!(!platform_profile_write_allows_mutation(&capability));
        assert!(platform_profile_has_pending(&transaction));
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
                    source: "test".into(),
                    fan: orbis_core::fan::FanId::Cpu,
                    label: "cpu_fan".into(),
                    rpm: orbis_core::newtypes::Rpm::new(2600).unwrap(),
                    percent: None,
                    quality: orbis_core::telemetry::FanTelemetryQuality::Complete,
                },
                orbis_core::telemetry::FanTelemetry {
                    source: "test".into(),
                    fan: orbis_core::fan::FanId::Gpu,
                    label: "gpu_fan".into(),
                    rpm: orbis_core::newtypes::Rpm::new(2100).unwrap(),
                    percent: None,
                    quality: orbis_core::telemetry::FanTelemetryQuality::Complete,
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

    #[test]
    fn telemetry_freshness_tracks_refresh_success() {
        // Initially not fresh (no authoritative refresh yet).
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.reset_telemetry();
        assert!(!s.telemetry_fresh);

        // Successful refresh marks fresh.
        s.update_telemetry(&sample_telemetry());
        assert!(s.telemetry_fresh);

        // Failed refresh marks stale but preserves the last values.
        s.mark_telemetry_stale();
        assert!(!s.telemetry_fresh);
        assert_eq!(s.cpu_temp, "46°C"); // last successful value preserved
        assert_eq!(s.battery_percent, "100%");
    }

    #[test]
    fn ac_online_false_differs_from_unavailable() {
        // Some(false) = on battery (a real value), None = unavailable.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.reset_telemetry();

        let mut t = sample_telemetry();
        t.ac_online = Some(false);
        s.update_telemetry(&t);
        assert_eq!(s.ac_online, "On battery");

        let mut t = sample_telemetry();
        t.ac_online = None;
        s.update_telemetry(&t);
        assert_eq!(s.ac_online, "—");
    }

    #[test]
    fn fan_rpm_zero_is_valid_not_no_data() {
        // RPM 0 is a valid physical value (fan stopped), distinct from "no data".
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.reset_telemetry();

        let mut t = sample_telemetry();
        t.fans = vec![orbis_core::telemetry::FanTelemetry {
            source: "test".into(),
            fan: orbis_core::fan::FanId::Cpu,
            label: "cpu_fan".into(),
            rpm: orbis_core::newtypes::Rpm::new(0).unwrap(),
            percent: None,
            quality: orbis_core::telemetry::FanTelemetryQuality::Complete,
        }];
        s.update_telemetry(&t);
        assert_eq!(s.cpu_fan_rpm, "0 rpm");

        // No fan data at all → unknown placeholder.
        let mut t = sample_telemetry();
        t.fans.clear();
        s.update_telemetry(&t);
        assert_eq!(s.cpu_fan_rpm, "—");
    }

    #[test]
    fn absent_telemetry_shows_unknown_not_zero() {
        // Missing telemetry must render as the unknown placeholder, never "0".
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.reset_telemetry();
        let t = orbis_core::telemetry::Telemetry::empty();
        s.update_telemetry(&t);
        assert_eq!(s.cpu_temp, "—");
        assert_eq!(s.gpu_temp, "—");
        assert_eq!(s.battery_percent, "—");
        assert_eq!(s.ac_online, "—");
        assert_eq!(s.cpu_fan_rpm, "—");
        assert_eq!(s.gpu_fan_rpm, "—");
    }

    // -----------------------------------------------------------------------
    // Fan Curve profile-aware read tests
    // -----------------------------------------------------------------------

    #[test]
    fn load_fan_curve_sets_profile_from_asusd_profile() {
        use orbis_core::fan::{FanCurve, FanCurvePoint};
        use orbis_core::newtypes::{FanPwm, TemperatureC};
        use orbis_core::profile::PerformanceProfile;

        let mut s = UiState::from_mock_profile("zephyrus-full");
        let curve = FanCurve {
            profile: PerformanceProfile::Balanced,
            fan: FanId::Cpu,
            enabled: None,
            points: vec![
                FanCurvePoint::new(TemperatureC::new(50).unwrap(), FanPwm::new(0).unwrap()),
                FanCurvePoint::new(TemperatureC::new(85).unwrap(), FanPwm::new(100).unwrap()),
            ],
        };

        // Load with Quiet profile — should set fan_profile_selected = 2
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Quiet);
        assert_eq!(s.fan_profile_selected, 2); // Quiet

        // Load with LowPower profile — should set fan_profile_selected = 3
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::LowPower);
        assert_eq!(s.fan_profile_selected, 3); // LowPower

        // Load with Balanced profile — should set fan_profile_selected = 0
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
        assert_eq!(s.fan_profile_selected, 0); // Balanced

        // Load with Performance profile — should set fan_profile_selected = 1
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Performance);
        assert_eq!(s.fan_profile_selected, 1); // Performance
    }

    #[test]
    fn load_fan_curve_clears_dirty_and_error() {
        use orbis_core::fan::FanCurve;
        use orbis_core::profile::PerformanceProfile;

        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.fan_curve_dirty = true;
        s.fan_curve_error = true;

        let curve = FanCurve {
            profile: PerformanceProfile::Balanced,
            fan: FanId::Cpu,
            enabled: None,
            points: vec![],
        };
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);

        assert!(!s.fan_curve_dirty);
        assert!(!s.fan_curve_error);
        assert_eq!(s.fan_curve_state, FanCurveHwState::Ready);
    }

    #[test]
    fn quiet_and_low_power_are_distinguishable() {
        use orbis_core::fan::FanCurve;
        use orbis_core::profile::PerformanceProfile;

        let mut s = UiState::from_mock_profile("zephyrus-full");
        let curve = FanCurve {
            profile: PerformanceProfile::Silent,
            fan: FanId::Cpu,
            enabled: None,
            points: vec![],
        };

        // Quiet = index 2
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Quiet);
        assert_eq!(s.fan_profile_selected, 2);

        // LowPower = index 3 (NOT the same as Quiet!)
        s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::LowPower);
        assert_eq!(s.fan_profile_selected, 3);

        // Verify they are different
        assert_ne!(
            orbis_core::profile::AsusdFanProfile::Quiet,
            orbis_core::profile::AsusdFanProfile::LowPower
        );
    }

    #[test]
    fn fan_curve_points_reject_integer_narrowing_overflow() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        s.fan_curve_temps = [50; 8];
        s.fan_curve_pwms = [100; 8];
        assert!(s.build_fan_curve_points().is_some());

        s.fan_curve_temps[0] = i32::from(i16::MAX) + 1;
        assert!(s.build_fan_curve_points().is_none());

        s.fan_curve_temps[0] = 50;
        s.fan_curve_pwms[0] = 256;
        assert!(s.build_fan_curve_points().is_none());

        s.fan_curve_pwms[0] = -1;
        assert!(s.build_fan_curve_points().is_none());
    }

    // -----------------------------------------------------------------------
    // Runtime capability mapping: registry → controller state
    // -----------------------------------------------------------------------

    /// Build a `CapabilityRegistrySnapshot` with the given capabilities
    /// and return it. Helper for controller capability mapping tests.
    fn registry_with(
        entries: Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)>,
    ) -> orbis_capabilities::CapabilityRegistrySnapshot {
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        for (id, cap) in entries {
            builder.add(id, cap).unwrap();
        }
        builder.build().unwrap()
    }

    fn cap(
        status: CapabilityStatus,
        read: CapabilityStatus,
        write: CapabilityStatus,
    ) -> orbis_core::capability::Capability {
        use orbis_core::capability::{CapabilityOperations, OperationCapability};
        orbis_core::capability::Capability::new(status).with_operations(CapabilityOperations {
            read: OperationCapability::new(read),
            write: OperationCapability::new(write),
        })
    }

    #[test]
    fn battery_read_write_supported_maps_to_controller() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.charge_limit_capability, CapabilityAvailability::Supported);
        assert!(s.charge_limit_writable);
        assert_eq!(s.charge_limit_unavailable_reason, None);
    }

    #[test]
    fn battery_read_supported_write_unsupported_maps_to_controller() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Unsupported,
            ),
        )]);
        s.update_capabilities(&snapshot);
        // Overall status is Supported (read is Supported), but write is
        // Unsupported → not writable. Controller uses write status for gating.
        assert_eq!(s.charge_limit_capability, CapabilityAvailability::Supported);
        assert!(!s.charge_limit_writable);
        assert_eq!(
            s.charge_limit_unavailable_reason.as_deref(),
            Some("Not supported on this system")
        );
    }

    #[test]
    fn battery_backend_missing_maps_to_controller() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(
            s.charge_limit_capability,
            CapabilityAvailability::BackendMissing
        );
        assert!(!s.charge_limit_writable);
    }

    #[test]
    fn battery_permission_denied_maps_to_controller() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        // PermissionDenied on read → overall PermissionDenied.
        // Write stays Unsupported (no write evidence).
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::PermissionDenied,
                CapabilityStatus::PermissionDenied,
                CapabilityStatus::Unsupported,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(
            s.charge_limit_capability,
            CapabilityAvailability::PermissionDenied
        );
        assert!(!s.charge_limit_writable);
    }

    #[test]
    fn battery_write_temporarily_unavailable_does_not_enable_mutation() {
        // Read is fine, but the mutation backend is temporarily unavailable:
        // the UI must NOT enable the write control. The distinction is
        // preserved in the capability availability, not collapsed to a bool.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::TemporarilyUnavailable,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.charge_limit_capability, CapabilityAvailability::Supported);
        assert!(!s.charge_limit_writable);
        // write_allows_mutation rejects TemporarilyUnavailable directly.
        assert!(!write_allows_mutation(
            CapabilityStatus::TemporarilyUnavailable
        ));
    }

    #[test]
    fn performance_read_write_supported_maps_to_controller() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::Performance,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.perf_capability, CapabilityAvailability::Supported);
        assert!(s.perf_writable);
    }

    #[test]
    fn performance_write_temporarily_unavailable_does_not_enable_mutation() {
        // Read is fine, but the Performance mutation backend is temporarily
        // unavailable: the UI must NOT enable the write control. The
        // distinction is preserved, not collapsed to a bool.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::Performance,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::TemporarilyUnavailable,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.perf_capability, CapabilityAvailability::Supported);
        assert!(!s.perf_writable);
        // write_allows_mutation rejects TemporarilyUnavailable directly.
        assert!(!write_allows_mutation(
            CapabilityStatus::TemporarilyUnavailable
        ));
    }

    #[test]
    fn performance_write_permission_denied_does_not_enable_mutation() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::Performance,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::PermissionDenied,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.perf_writable);
        assert!(!write_allows_mutation(CapabilityStatus::PermissionDenied));
    }

    #[test]
    fn one_unavailable_domain_does_not_affect_others() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![
            (
                orbis_core::FeatureId::ChargeLimit,
                cap(
                    CapabilityStatus::BackendMissing,
                    CapabilityStatus::BackendMissing,
                    CapabilityStatus::BackendMissing,
                ),
            ),
            (
                orbis_core::FeatureId::Performance,
                cap(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                ),
            ),
            (
                orbis_core::FeatureId::GpuPower,
                cap(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::Unsupported,
                ),
            ),
        ]);
        s.update_capabilities(&snapshot);
        // Battery is BackendMissing.
        assert_eq!(
            s.charge_limit_capability,
            CapabilityAvailability::BackendMissing
        );
        assert!(!s.charge_limit_writable);
        // Performance is Supported + writable — independent of Battery.
        assert_eq!(s.perf_capability, CapabilityAvailability::Supported);
        assert!(s.perf_writable);
        // GPU Power is Supported but read-only — independent of Battery.
        assert_eq!(s.gpu_power_capability, CapabilityAvailability::Supported);
    }

    #[test]
    fn fan_curve_writable_matches_write_operation_status() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        // Write = Supported.
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::FanCurves,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.fan_curve_capability, CapabilityAvailability::Supported);
        assert!(s.fan_curve_writable);

        // Write = Unsupported.
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::FanCurves,
            cap(
                CapabilityStatus::ReadOnly,
                CapabilityStatus::Supported,
                CapabilityStatus::Unsupported,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.fan_curve_capability, CapabilityAvailability::ReadOnly);
        assert!(!s.fan_curve_writable);
    }

    #[test]
    fn fan_curve_write_temporarily_unavailable_does_not_enable_mutation() {
        // Read is fine, but the fan curve mutation backend is temporarily
        // unavailable: the UI must NOT enable the write control. The
        // distinction is preserved, not collapsed to a bool.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::FanCurves,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::TemporarilyUnavailable,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert_eq!(s.fan_curve_capability, CapabilityAvailability::Supported);
        assert!(!s.fan_curve_writable);
        assert!(!write_allows_mutation(
            CapabilityStatus::TemporarilyUnavailable
        ));
    }

    #[test]
    fn fan_curve_write_permission_denied_does_not_enable_mutation() {
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::FanCurves,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::PermissionDenied,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.fan_curve_writable);
        assert!(!write_allows_mutation(CapabilityStatus::PermissionDenied));
    }

    #[test]
    fn mutation_unavailable_reason_maps_each_availability() {
        // The single shared mapping must produce a distinct, honest reason for
        // every non-writable availability and None for Supported.
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::Supported),
            None
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::ReadOnly).as_deref(),
            Some("Read-only")
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::Unsupported).as_deref(),
            Some("Not supported on this system")
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::BackendMissing).as_deref(),
            Some("Required backend is not available")
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::TemporarilyUnavailable).as_deref(),
            Some("Temporarily unavailable")
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::PermissionDenied).as_deref(),
            Some("Permission denied")
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::Unknown).as_deref(),
            Some("Availability is unknown")
        );
        assert_eq!(
            mutation_unavailable_reason(CapabilityAvailability::Conflicted).as_deref(),
            Some("Conflicting evidence")
        );
    }

    #[test]
    fn battery_reasons_distinguish_statuses() {
        // PermissionDenied and TemporarilyUnavailable must not be masked as a
        // generic unavailable.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::PermissionDenied,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.charge_limit_writable);
        assert_eq!(
            s.charge_limit_unavailable_reason.as_deref(),
            Some("Permission denied")
        );

        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::ChargeLimit,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::TemporarilyUnavailable,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.charge_limit_writable);
        assert_eq!(
            s.charge_limit_unavailable_reason.as_deref(),
            Some("Temporarily unavailable")
        );
    }

    #[test]
    fn performance_reasons_distinguish_statuses() {
        // BackendMissing and Unknown must not be masked as unsupported.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::Performance,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::BackendMissing,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.perf_writable);
        assert_eq!(
            s.perf_unavailable_reason.as_deref(),
            Some("Required backend is not available")
        );

        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::Performance,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Unknown,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.perf_writable);
        assert_eq!(
            s.perf_unavailable_reason.as_deref(),
            Some("Availability is unknown")
        );
    }

    #[test]
    fn fan_curve_reasons_distinguish_statuses() {
        // Unknown and PermissionDenied must not be masked as unsupported.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::FanCurves,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Unknown,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.fan_curve_writable);
        assert_eq!(
            s.fan_curve_unavailable_reason.as_deref(),
            Some("Availability is unknown")
        );

        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![(
            orbis_core::FeatureId::FanCurves,
            cap(
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::PermissionDenied,
            ),
        )]);
        s.update_capabilities(&snapshot);
        assert!(!s.fan_curve_writable);
        assert_eq!(
            s.fan_curve_unavailable_reason.as_deref(),
            Some("Permission denied")
        );
    }

    #[test]
    fn unavailable_write_does_not_disable_independent_read_domains() {
        // Battery write PermissionDenied must not affect Performance or GPU
        // read capability in the same controller state.
        let mut s = UiState::from_mock_profile("zephyrus-full");
        let snapshot = registry_with(vec![
            (
                orbis_core::FeatureId::ChargeLimit,
                cap(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::PermissionDenied,
                ),
            ),
            (
                orbis_core::FeatureId::Performance,
                cap(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                ),
            ),
            (
                orbis_core::FeatureId::GpuPower,
                cap(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::Unsupported,
                ),
            ),
        ]);
        s.update_capabilities(&snapshot);
        // Battery write denied, but read capability still present.
        assert!(!s.charge_limit_writable);
        assert_eq!(
            s.charge_limit_unavailable_reason.as_deref(),
            Some("Permission denied")
        );
        // Performance remains writable.
        assert!(s.perf_writable);
        assert_eq!(s.perf_unavailable_reason, None);
        // GPU Power read remains Supported.
        assert_eq!(s.gpu_power_capability, CapabilityAvailability::Supported);
    }
}
