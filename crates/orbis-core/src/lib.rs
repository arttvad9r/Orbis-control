//! # Orbis Core
//!
//! Платформенно-независимая доменная модель проекта Orbis Control.
//!
//! Гарантии:
//! - никакого Slint, D-Bus, Tokio, sysfs, platform-specific кода;
//! - никакого `unsafe` (см. SAFETY.md);
//! - строгие newtype для процентов, температур, RPM, ватт, частот;
//! - валидация диапазонов в конструкторах;
//! - `serde` для сериализации (D-Bus/JSON/TOML).
//!
//! Человекочитаемые строки локализации НЕ хранятся в enum-ах модели;
//! отображение выполняется на уровне UI/CLI.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod action;
pub mod aura;
pub mod automation;
pub mod battery;
pub mod capability;
pub mod diagnostics;
pub mod display;
pub mod display_output;
pub mod error;
pub mod fan;
pub mod gpu;
pub mod identity;
pub mod keyboard_backlight;
pub mod lighting;
pub mod limits;
pub mod newtypes;
pub mod power;
pub mod profile;
pub mod provider;
pub mod telemetry;
pub mod warning;

pub use action::{ActionRequirement, ApplyResult, PendingAction};
pub use aura::{
    AuraBrightness, AuraDirection, AuraEffect, AuraMode, AuraRgb, AuraSpeed, AuraState, AuraZone,
};
pub use automation::{AutomationAction, AutomationRule, AutomationTrigger};
pub use battery::ChargeLimit;
pub use capability::{
    Capability, CapabilityReason, CapabilityStatus, DeviceCapabilities, FeatureId,
};
pub use diagnostics::{DiagnosticEntry, DiagnosticReport};
pub use display::{
    DisplayMode, HdrState, MiniLedModeKind, MiniLedModeState, MiniLedModeValue,
    PanelOverdriveState, RefreshMode, ScreenAutoBrightnessState, interpret_mini_led_mode,
};
pub use display_output::{
    CurrentDisplayMode, DisplayMode as OutputDisplayMode, DisplayOutputId, DisplayOutputSnapshot,
    DisplayOutputState,
};
pub use error::CoreError;
pub use fan::{FanCurve, FanCurvePoint, FanId};
pub use gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
pub use identity::{BackendIdentity, DeviceIdentity};
pub use keyboard_backlight::{KeyboardBacklightState, KeyboardBrightnessLevel};
pub use lighting::LightingMode;
pub use limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
pub use newtypes::{EnergyMWh, FanPwm, MilliWatt, Percent, PowerW, RefreshHz, Rpm, TemperatureC};
pub use power::PowerSource;
pub use profile::{AsusdFanProfile, PerformanceProfile, PlatformProfile};
pub use provider::ProviderStatus;
pub use telemetry::{BatteryTelemetry, FanTelemetry, HardwareSnapshot, PowerTelemetry, Telemetry};
pub use warning::{Warning, WarningSeverity};

/// Результат операции, общий для провайдеров и демона.
pub type Result<T> = std::result::Result<T, CoreError>;
