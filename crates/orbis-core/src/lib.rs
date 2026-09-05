//! # Orbis Core
//!
//! Платформенно-независимая доменная модель проекта Orbis Control.
//!
//! Гарантии:
//! - никакого Slint, D-Bus, Tokio, sysfs, platform-specific кода;
//! - никакого `unsafe` (см. SAFETY.md);
//! - строгие newtype для процентов, температур, RPM, ватт, частот;
//! - валидацию диапазонов в конструкторах;
//! - `serde` для сериализации (D-Bus/JSON/TOML).
//!
//! Человекочитаемые строки локализации НЕ хранятся в enum-ах модели;
//! отображение выполняется на уровне UI/CLI.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod action;
pub mod aura;
pub mod battery;
pub mod capability;
pub mod desired_observed;
pub mod diagnostics;
pub mod display;
pub mod display_output;
pub mod display_refresh;
pub mod display_refresh_identity;
pub mod display_refresh_request;
pub mod display_refresh_state;
pub mod error;
pub mod fan;
pub mod fan_policy;
pub mod firmware;
pub mod gpu;
pub mod identity;
pub mod keyboard_backlight;
pub mod lifecycle;
pub mod lighting;
pub mod limits;
pub mod mutation_audit;
pub mod newtypes;
pub mod pending_transitions;
pub mod platform_profile;
pub mod power;
pub mod preset;
pub mod preset_bundle;
pub mod profile;
pub mod provider;
pub mod readiness;
pub mod reconcile_schedule;
pub mod reconciliation;
pub mod restoration;
pub mod system_telemetry;
pub mod telemetry;
pub mod telemetry_export;
pub mod telemetry_history;
pub mod telemetry_stats;
pub mod thermal_control;
pub mod transaction;
pub mod validation;
pub mod warning;

pub use action::{ActionRequirement, ApplyResult, PendingAction};
pub use aura::{
    AuraBrightness, AuraDirection, AuraEffect, AuraMode, AuraRgb, AuraSpeed, AuraState, AuraZone,
};
pub use battery::{
    BatteryThresholdConfidence, BatteryThresholdEvidence, BatteryThresholdEvidenceState,
    BatteryThresholdFreshness, BatteryThresholdObservation, BatteryThresholdSource, ChargeLimit,
};
pub use capability::{
    Capability, CapabilityReason, CapabilityStatus, DeviceCapabilities, FeatureId,
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
pub use display_refresh::{
    DisplayRefreshConstraints, DisplayRefreshEvidence, DisplayRefreshPreset,
    DisplayRefreshPresetTarget, DisplayRefreshTargetId, DisplayRefreshTargetRole,
};
pub use display_refresh_identity::{
    CompositorDisplayHeadEvidence, DisplayPhysicalSizeMm, DisplaySinkIdentity,
    DisplayTargetIdentityBlock, DisplayTargetIdentityProof, DrmConnectorTypeEvidence,
    DrmDisplayConnectorEvidence, prove_display_target_identity,
};
pub use display_refresh_request::DisplayRefreshRequest;
pub use display_refresh_state::{DisplayRefreshActivePolicy, DisplayRefreshAppliedState};
pub use error::CoreError;
pub use fan::{FanCurve, FanCurvePoint, FanId};
pub use fan_policy::{SoftwareFanPolicy, TemperatureDeadband, interpolate_curve};
pub use firmware::BootSoundState;
pub use gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
pub use identity::{BackendIdentity, DeviceIdentity};
pub use keyboard_backlight::{KeyboardBacklightState, KeyboardBrightnessLevel};
pub use lifecycle::{LifecycleEvent, ResumeGateOutcome, ResumeTelemetryGate};
pub use lighting::LightingMode;
pub use limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
pub use mutation_audit::{
    LastMutationAudit, MutationAuditEntry, MutationAuditOutcome, outcome_from_apply_result,
    outcome_from_phase,
};
pub use newtypes::{EnergyMWh, FanPwm, MilliWatt, Percent, PowerW, RefreshHz, Rpm, TemperatureC};
pub use pending_transitions::{PendingTransition, PendingTransitionRegistry};
pub use platform_profile::{
    PlatformProfileCapability, PlatformProfileEvidenceState, PlatformProfileSource,
    PlatformProfileTelemetry, PlatformProfileTelemetryQuality, PlatformProfileTransaction,
};
pub use power::PowerSource;
pub use preset::{PowerPresetPolicy, Preset, PresetIntent};
pub use preset_bundle::{PRESET_BUNDLE_SCHEMA_VERSION, PresetBundle, PresetBundleError};
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
pub use system_telemetry::{
    MemoryPressureTelemetry, PsiPressureLine, SystemMemoryTelemetry, ZramTelemetry, ZswapTelemetry,
};
pub use telemetry::{
    BatteryTelemetry, FanTelemetry, HardwareSnapshot, PowerTelemetry, Telemetry, TelemetryQuality,
};
pub use telemetry_export::{MetricExportRow, MetricFreshness, metric_rows_to_csv};
pub use telemetry_history::{BoundedHistory, HistoryError, HistorySample};
pub use telemetry_stats::{IntegerStats, integer_stats};
pub use thermal_control::{
    EmaFilter, HysteresisGate, PwmRateLimiter, ThermalControlError, average_temperature,
    max_temperature, min_temperature, offset_temperature, temperature_delta, weighted_temperature,
};
pub use transaction::{MutationPhase, MutationTransaction};
pub use validation::{HardwareValidationEvidence, HardwareValidationStage};
pub use warning::{Warning, WarningSeverity};

/// Результат операции, общий для провайдеров и демона.
pub type Result<T> = std::result::Result<T, CoreError>;
