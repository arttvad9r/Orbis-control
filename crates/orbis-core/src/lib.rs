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
pub mod alerts;
pub mod aura;
pub mod automation;
pub mod automation_policy;
pub mod battery;
pub mod capability;
pub mod control_flow;
pub mod desired_observed;
pub mod diagnostics;
pub mod display;
pub mod display_output;
pub mod error;
pub mod fan;
pub mod fan_policy;
pub mod gpu;
pub mod identity;
pub mod keyboard_backlight;
pub mod lifecycle;
pub mod lighting;
pub mod limits;
pub mod mutation_audit;
pub mod newtypes;
pub mod pending_transitions;
pub mod power;
pub mod preset;
pub mod profile;
pub mod provider;
pub mod readiness;
pub mod reconcile_schedule;
pub mod reconciliation;
pub mod restoration;
pub mod telemetry;
pub mod telemetry_history;
pub mod telemetry_stats;
pub mod thermal_control;
pub mod transaction;
pub mod validation;
pub mod warning;

pub use action::{ActionRequirement, ApplyResult, PendingAction};
pub use alerts::{
    AlertEvent, AlertSeverity, fan_stopped_when_hot, persistent_divergence, telemetry_stale,
    temperature_above,
};
pub use aura::{
    AuraBrightness, AuraDirection, AuraEffect, AuraMode, AuraRgb, AuraSpeed, AuraState, AuraZone,
};
pub use automation::{AutomationAction, AutomationRule, AutomationTrigger};
pub use automation_policy::{
    PolicyCondition, PolicyContext, PolicyEvent, PolicyRule, PolicySelection, PolicyTrigger,
    select_policy_preset,
};
pub use battery::ChargeLimit;
pub use capability::{
    Capability, CapabilityReason, CapabilityStatus, DeviceCapabilities, FeatureId,
};
pub use control_flow::{
    ControlFlowEdge, ControlFlowError, ControlFlowGraph, ControlFlowNode, ControlFlowNodeKind,
};
pub use desired_observed::{DesiredObservedState, DesiredValue, ObservedValue, PendingValue};
pub use diagnostics::{
    ApplicationDiagnostics, CapabilitySnapshotDiagnostics, DIAGNOSTICS_SNAPSHOT_SCHEMA_VERSION,
    DiagnosticEntry, DiagnosticObservation, DiagnosticReport, DiagnosticsServiceId,
    DiagnosticsSnapshot, DiagnosticsSnapshotSections, DisplayDiagnostics, DisplayProtocol,
    GpuDiagnostics, HardwareDiagnostics, ServiceAvailability, ServiceBusScope, ServiceCriticality,
    ServiceDiagnostics, SessionType, SystemDiagnostics, TelemetryCollectionStatus,
    TelemetryDiagnostics, TelemetryFreshness,
};
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
pub use fan_policy::{SoftwareFanPolicy, TemperatureDeadband, interpolate_curve};
pub use gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
pub use identity::{BackendIdentity, DeviceIdentity};
pub use keyboard_backlight::{KeyboardBacklightState, KeyboardBrightnessLevel};
pub use lifecycle::LifecycleEvent;
pub use lighting::LightingMode;
pub use limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
pub use mutation_audit::{
    LastMutationAudit, MutationAuditEntry, MutationAuditOutcome, outcome_from_apply_result,
    outcome_from_phase,
};
pub use newtypes::{EnergyMWh, FanPwm, MilliWatt, Percent, PowerW, RefreshHz, Rpm, TemperatureC};
pub use pending_transitions::{PendingTransition, PendingTransitionRegistry};
pub use power::PowerSource;
pub use preset::{PowerPresetPolicy, Preset, PresetIntent};
pub use profile::{AsusdFanProfile, PerformanceProfile, PlatformProfile};
pub use provider::ProviderStatus;
pub use readiness::{
    OwnershipClaim, OwnershipConflict, PermissionState, ReadinessItem, ReadinessReport,
    ReadinessState, detect_ownership_conflicts,
};
pub use reconcile_schedule::{
    DispatchPermission, ReconcileTrigger, dispatch_permission, may_dispatch,
    transaction_blocks_dispatch,
};
pub use reconciliation::{ReconcileDecision, decide_reconciliation};
pub use restoration::{RestorationPlan, RestorationState};
pub use telemetry::{BatteryTelemetry, FanTelemetry, HardwareSnapshot, PowerTelemetry, Telemetry};
pub use telemetry_history::{BoundedHistory, HistoryError, HistorySample};
pub use telemetry_stats::{IntegerStats, integer_stats};
pub use thermal_control::{
    EmaFilter, HysteresisGate, PwmRateLimiter, ThermalControlError, average_temperature,
    max_temperature, min_temperature, weighted_temperature,
};
pub use transaction::{MutationPhase, MutationTransaction};
pub use validation::{HardwareValidationEvidence, HardwareValidationStage};
pub use warning::{Warning, WarningSeverity};

/// Результат операции, общий для провайдеров и демона.
pub type Result<T> = std::result::Result<T, CoreError>;
