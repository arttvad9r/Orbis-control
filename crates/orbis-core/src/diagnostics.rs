//! Диагностика.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::capability::DeviceCapabilities;
use crate::display_output::DisplayOutputSnapshot;
use crate::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use crate::identity::DeviceIdentity;
use crate::limits::PowerLimitValue;
use crate::limits::PowerLimits;
use crate::newtypes::{MilliWatt, TemperatureC};
use crate::telemetry::{Telemetry, TelemetryQuality};
use crate::warning::WarningSeverity;

/// Версия typed in-memory diagnostics snapshot contract.
pub const DIAGNOSTICS_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Запись диагностики.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticEntry {
    /// Ключ (стабильный).
    pub key: String,
    /// Значение.
    pub value: String,
    /// Серьёзность.
    pub severity: WarningSeverity,
    /// Источник (провайдер/backend).
    pub source: Option<String>,
}

impl DiagnosticEntry {
    /// Обычная запись.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            severity: WarningSeverity::Info,
            source: None,
        }
    }

    /// Запись с серьёзностью.
    pub fn with_severity(
        key: impl Into<String>,
        value: impl Into<String>,
        severity: WarningSeverity,
    ) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            severity,
            source: None,
        }
    }
}

/// Полный диагностический отчёт.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Записи.
    pub entries: Vec<DiagnosticEntry>,
    /// Отметка времени генерации (Unix seconds).
    pub generated_at: u64,
}

impl DiagnosticReport {
    /// Найти запись по ключу.
    pub fn get(&self, key: &str) -> Option<&DiagnosticEntry> {
        self.entries.iter().find(|e| e.key == key)
    }
}

/// Typed result of observing a value for diagnostics.
///
/// `Value(T)` means the read succeeded even when `T` itself has a domain
/// `Unknown` variant. The other variants describe inability to obtain a value
/// and must not be collapsed into a synthetic domain value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticObservation<T> {
    /// A value was observed successfully.
    Value(T),
    /// The source is known but currently unavailable.
    Unavailable,
    /// The read itself was denied.
    PermissionDenied,
    /// There is not enough evidence to classify the observation.
    Unknown,
}

/// Metadata about the running application binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationDiagnostics {
    /// Exact package version of the running application.
    pub package_version: String,
    /// Build revision only when it was deliberately embedded at build time.
    pub build_revision: Option<String>,
    /// Build channel/profile only when a deterministic source exists.
    pub build_channel: Option<String>,
}

/// Normalized login/session type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    /// Wayland graphical session.
    Wayland,
    /// X11 graphical session.
    X11,
    /// Text/TTY session.
    Tty,
    /// Session type could not be determined reliably.
    Unknown,
}

/// Normalized graphical display protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayProtocol {
    /// Wayland protocol.
    Wayland,
    /// X11 protocol.
    X11,
    /// Display protocol could not be determined reliably.
    Unknown,
}

/// System and session metadata for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemDiagnostics {
    /// Exact kernel release when available.
    pub kernel_release: Option<String>,
    /// Architecture of the running system/process when available.
    pub architecture: Option<String>,
    /// Normalized login/session type.
    pub session_type: SessionType,
    /// Normalized graphical display protocol.
    pub display_protocol: DisplayProtocol,
    /// Compositor identity only from a reliable compositor-specific source.
    pub compositor: Option<String>,
}

/// Privacy-safe hardware metadata.
///
/// `DeviceIdentity` intentionally contains vendor/product/board/BIOS data and
/// no serial-number field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareDiagnostics {
    /// DMI identity when it was obtained from a trusted read-only source.
    pub identity: Option<DeviceIdentity>,
}

/// Stable service identity used by diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsServiceId {
    /// Orbis session daemon (`Session1`).
    OrbisSessiond,
    /// Orbis privileged hardware daemon (`Hardware1`).
    OrbisHardwared,
    /// asusd service.
    Asusd,
    /// supergfxd service.
    Supergfxd,
    /// UPower service.
    Upower,
}

/// D-Bus scope used for a service-presence observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceBusScope {
    /// System D-Bus.
    System,
    /// User/session D-Bus.
    Session,
}

/// Presence/health of one service.
///
/// This is deliberately separate from capability support and from observed
/// feature values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAvailability {
    /// The well-known service name currently has an owner.
    Running,
    /// No owner exists, but the service is known to be D-Bus activatable.
    Activatable,
    /// Neither a current owner nor an activatable service is available.
    Unavailable,
    /// The presence query itself was denied.
    PermissionDenied,
    /// Transport/query evidence is insufficient for classification.
    Unknown,
}

/// Runtime importance of a service to diagnostics consumers.
///
/// The collector must assign this from the current architecture; the domain
/// does not infer it from service presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCriticality {
    /// A core production read path currently depends on the service.
    CoreReadPath,
    /// Only one or more specific capabilities depend on the service.
    CapabilityLocal,
    /// The service is optional supplementary diagnostics evidence.
    Optional,
}

/// Point-in-time service diagnostics record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDiagnostics {
    /// Stable Orbis-owned service identifier.
    pub service: DiagnosticsServiceId,
    /// Bus on which service presence was checked.
    pub bus: ServiceBusScope,
    /// Presence/health classification.
    pub availability: ServiceAvailability,
    /// Current architectural importance of the service.
    pub criticality: ServiceCriticality,
    /// Time of the presence/health check, if known.
    pub checked_at: Option<SystemTime>,
}

/// Core-owned view of one immutable capability registry snapshot.
///
/// This deliberately stores `DeviceCapabilities` plus registry metadata instead
/// of depending on the `orbis-capabilities` crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySnapshotDiagnostics {
    /// Registry generation.
    pub generation: u64,
    /// Time at which the registry snapshot was checked/assembled.
    pub checked_at: SystemTime,
    /// Canonical core-owned capability data.
    pub capabilities: DeviceCapabilities,
}

/// Independent GPU primitive observations.
///
/// There is intentionally no product `GpuMode` field and no inference helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuDiagnostics {
    /// Authoritative physical MUX observation.
    pub mux: DiagnosticObservation<GpuMuxState>,
    /// Authoritative dGPU access-policy observation.
    pub access_policy: DiagnosticObservation<GpuAccessPolicy>,
    /// Authoritative runtime dGPU power observation.
    pub runtime_power: DiagnosticObservation<GpuPowerState>,
    /// Read-only NVIDIA power/thermal evidence from the driver hwmon node.
    pub nvidia: DiagnosticObservation<NvidiaGpuDiagnostics>,
}

/// NVIDIA observations that do not imply a writable power-limit owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NvidiaGpuDiagnostics {
    /// Current board power draw, when exposed by the driver.
    pub power: Option<MilliWatt>,
    /// Current GPU temperature, when exposed by the driver.
    pub temperature: Option<TemperatureC>,
    /// Current/default/min/max driver-reported power limit.
    pub power_limit: Option<PowerLimitValue>,
}

/// Result of the telemetry collection path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryCollectionStatus {
    /// Latest collection completed normally.
    Available,
    /// Collection produced useful but incomplete/degraded evidence.
    Degraded,
    /// Telemetry source is unavailable.
    Unavailable,
    /// Telemetry collection was denied.
    PermissionDenied,
    /// Collection state cannot be classified reliably.
    Unknown,
}

/// Freshness classification for the latest telemetry sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryFreshness {
    /// Sample age is inside the collector-defined freshness threshold.
    Fresh,
    /// A successful sample exists but exceeds the freshness threshold.
    Stale,
    /// No successful sample or reliable age comparison is available.
    Unknown,
}

/// Typed telemetry values plus collection/freshness metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryDiagnostics {
    /// Latest successful typed telemetry snapshot; absent means no synthetic read.
    pub latest: Option<Telemetry>,
    /// Current collection-path status.
    pub status: TelemetryCollectionStatus,
    /// Last collection attempt timestamp, if known.
    pub last_attempt_at: Option<SystemTime>,
    /// Last successful collection timestamp, if known.
    pub last_success_at: Option<SystemTime>,
    /// Freshness classification independent from the telemetry values.
    pub freshness: TelemetryFreshness,
}

impl TelemetryDiagnostics {
    /// Classify data quality independently from collection availability and
    /// freshness.
    pub fn quality(&self) -> TelemetryQuality {
        if self.latest.is_none() {
            return TelemetryQuality::Failed;
        }
        if matches!(self.freshness, TelemetryFreshness::Stale) {
            return TelemetryQuality::Stale;
        }
        match self.status {
            TelemetryCollectionStatus::Unavailable
            | TelemetryCollectionStatus::PermissionDenied
            | TelemetryCollectionStatus::Unknown => TelemetryQuality::Stale,
            TelemetryCollectionStatus::Available | TelemetryCollectionStatus::Degraded => self
                .latest
                .as_ref()
                .map(Telemetry::quality)
                .unwrap_or(TelemetryQuality::Failed),
        }
    }
}

/// Read-only display/output diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayDiagnostics {
    /// Observed compositor output snapshot, kept distinct from writability.
    pub outputs: DiagnosticObservation<DisplayOutputSnapshot>,
    /// Time of the output observation, if known.
    pub checked_at: Option<SystemTime>,
}

/// Read-only CPU package power-limit observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuPackagePowerLimitsObservation {
    /// Current values and metadata were read successfully.
    Value(PowerLimits),
    /// The backend was present but the read was unavailable.
    Unavailable,
    /// The attributes are absent or the feature is unsupported.
    Unsupported,
    /// The read was denied.
    PermissionDenied,
    /// The backend returned malformed or inconsistent metadata.
    Malformed,
    /// The result could not be classified reliably.
    Unknown,
}

/// Read-only CPU frequency policy evidence; this does not imply write ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuFrequencyDiagnostics {
    /// Kernel CPU frequency driver name.
    pub driver: String,
    /// EPP preferences exposed by the kernel policy.
    pub available_epp_preferences: Vec<String>,
    /// Current kernel EPP preference.
    pub current_epp_preference: String,
    /// Current kernel boost state.
    pub boost: bool,
}

/// Result of observing CPU frequency policy evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuFrequencyObservation {
    /// All requested values were read and validated.
    Value(CpuFrequencyDiagnostics),
    /// The source is known but currently unavailable.
    Unavailable,
    /// The attributes are absent or unsupported.
    Unsupported,
    /// The read was denied.
    PermissionDenied,
    /// The source returned malformed values.
    Malformed,
    /// The result could not be classified reliably.
    Unknown,
}

/// Typed sections carried by an immutable diagnostics snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsSnapshotSections {
    /// Application metadata.
    pub application: ApplicationDiagnostics,
    /// System/session metadata.
    pub system: SystemDiagnostics,
    /// Privacy-safe hardware metadata.
    pub hardware: HardwareDiagnostics,
    /// Independent service presence/health records.
    pub services: Vec<ServiceDiagnostics>,
    /// Canonical capability data plus registry metadata.
    pub capabilities: CapabilitySnapshotDiagnostics,
    /// Independent GPU primitive observations.
    pub gpu: GpuDiagnostics,
    /// Typed telemetry and freshness metadata.
    pub telemetry: TelemetryDiagnostics,
    /// Read-only display/output observations.
    pub display: DisplayDiagnostics,
    /// Read-only CPU package power-limit observation.
    pub cpu_package_power_limits: CpuPackagePowerLimitsObservation,
    /// Read-only CPU frequency policy evidence.
    pub cpu_frequency: CpuFrequencyObservation,
}

/// Immutable point-in-time production diagnostics domain snapshot.
///
/// Fields are private and there are no mutation APIs. A collector assembles a
/// complete set of typed sections and hands the frozen snapshot to future
/// presentation/export layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    schema_version: u32,
    generated_at: SystemTime,
    sections: DiagnosticsSnapshotSections,
}

impl DiagnosticsSnapshot {
    /// Freeze one point-in-time set of diagnostics sections.
    pub fn new(generated_at: SystemTime, sections: DiagnosticsSnapshotSections) -> Self {
        Self {
            schema_version: DIAGNOSTICS_SNAPSHOT_SCHEMA_VERSION,
            generated_at,
            sections,
        }
    }

    /// Diagnostics snapshot schema version.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Time at which this aggregate snapshot was generated.
    pub fn generated_at(&self) -> SystemTime {
        self.generated_at
    }

    /// Borrow all typed snapshot sections without exposing mutation.
    pub fn sections(&self) -> &DiagnosticsSnapshotSections {
        &self.sections
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::time::Duration;

    use crate::capability::{Capability, CapabilityStatus, FeatureId};

    use super::*;

    #[test]
    fn report_lookup() {
        let mut r = DiagnosticReport::default();
        r.entries.push(DiagnosticEntry::new("kernel", "7.1.6"));
        assert_eq!(r.get("kernel").unwrap().value, "7.1.6");
        assert!(r.get("missing").is_none());
    }

    #[test]
    fn service_state_variants_remain_distinct() {
        let states = [
            ServiceAvailability::Running,
            ServiceAvailability::Activatable,
            ServiceAvailability::Unavailable,
            ServiceAvailability::PermissionDenied,
            ServiceAvailability::Unknown,
        ];
        let unique: HashSet<_> = states.into_iter().collect();
        assert_eq!(unique.len(), states.len());
    }

    #[test]
    fn gpu_primitives_are_independent() {
        let gpu = GpuDiagnostics {
            mux: DiagnosticObservation::Value(GpuMuxState::Discrete),
            access_policy: DiagnosticObservation::Value(GpuAccessPolicy::Blocked),
            runtime_power: DiagnosticObservation::Unavailable,
            nvidia: DiagnosticObservation::Unknown,
        };

        assert_eq!(gpu.mux, DiagnosticObservation::Value(GpuMuxState::Discrete));
        assert_eq!(
            gpu.access_policy,
            DiagnosticObservation::Value(GpuAccessPolicy::Blocked)
        );
        assert_eq!(gpu.runtime_power, DiagnosticObservation::Unavailable);
    }

    #[test]
    fn absent_optional_build_and_compositor_fields_remain_absent() {
        let app = ApplicationDiagnostics {
            package_version: "0.1.0".into(),
            build_revision: None,
            build_channel: None,
        };
        let system = SystemDiagnostics {
            kernel_release: None,
            architecture: None,
            session_type: SessionType::Unknown,
            display_protocol: DisplayProtocol::Unknown,
            compositor: None,
        };

        assert!(app.build_revision.is_none());
        assert!(app.build_channel.is_none());
        assert!(system.compositor.is_none());
        assert!(system.kernel_release.is_none());
        assert!(system.architecture.is_none());
    }

    #[test]
    fn privacy_safe_hardware_identity_has_no_serial_field() {
        let hardware = HardwareDiagnostics {
            identity: Some(DeviceIdentity {
                vendor: "ASUSTeK COMPUTER INC.".into(),
                product: "Example".into(),
                board: "BOARD".into(),
                bios_version: "1.0".into(),
                bios_date: "2026-01-01".into(),
            }),
        };

        let json = serde_json::to_string(&hardware).unwrap();
        assert!(!json.contains("serial"));
        assert!(json.contains("vendor"));
        assert!(json.contains("bios_version"));
    }

    #[test]
    fn snapshot_preserves_capability_generation_and_timestamp() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        let mut capabilities = DeviceCapabilities::default();
        capabilities.features.insert(
            FeatureId::GpuMux,
            Capability::new(CapabilityStatus::Unknown),
        );
        let snapshot = CapabilitySnapshotDiagnostics {
            generation: 17,
            checked_at,
            capabilities,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let back: CapabilitySnapshotDiagnostics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.generation, 17);
        assert_eq!(back.checked_at, checked_at);
        assert_eq!(
            back.capabilities.status(FeatureId::GpuMux),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn telemetry_freshness_and_status_survive_roundtrip() {
        let last_attempt_at = SystemTime::UNIX_EPOCH + Duration::from_secs(80);
        let last_success_at = SystemTime::UNIX_EPOCH + Duration::from_secs(40);
        let telemetry = TelemetryDiagnostics {
            latest: None,
            status: TelemetryCollectionStatus::Degraded,
            last_attempt_at: Some(last_attempt_at),
            last_success_at: Some(last_success_at),
            freshness: TelemetryFreshness::Stale,
        };

        let json = serde_json::to_string(&telemetry).unwrap();
        let back: TelemetryDiagnostics = serde_json::from_str(&json).unwrap();
        assert_eq!(back, telemetry);
        assert!(back.latest.is_none());
        assert_eq!(back.status, TelemetryCollectionStatus::Degraded);
        assert_eq!(back.freshness, TelemetryFreshness::Stale);
    }

    #[test]
    fn unknown_and_unavailable_require_no_fake_defaults() {
        let no_evidence = DiagnosticObservation::<GpuMuxState>::Unknown;
        let unavailable = DiagnosticObservation::<GpuMuxState>::Unavailable;
        let observed_domain_unknown = DiagnosticObservation::Value(GpuMuxState::Unknown);

        assert_ne!(no_evidence, unavailable);
        assert_ne!(no_evidence, observed_domain_unknown);
        assert_ne!(unavailable, observed_domain_unknown);

        let display = DisplayDiagnostics {
            outputs: DiagnosticObservation::Unavailable,
            checked_at: None,
        };
        let telemetry = TelemetryDiagnostics {
            latest: None,
            status: TelemetryCollectionStatus::Unavailable,
            last_attempt_at: None,
            last_success_at: None,
            freshness: TelemetryFreshness::Unknown,
        };

        assert_eq!(display.outputs, DiagnosticObservation::Unavailable);
        assert!(telemetry.latest.is_none());
        assert_eq!(telemetry.freshness, TelemetryFreshness::Unknown);
    }

    #[test]
    fn cpu_frequency_observation_keeps_read_only_evidence_typed() {
        let observation = CpuFrequencyObservation::Value(CpuFrequencyDiagnostics {
            driver: "amd-pstate-epp".into(),
            available_epp_preferences: vec!["power".into(), "performance".into()],
            current_epp_preference: "power".into(),
            boost: false,
        });

        assert_eq!(
            observation,
            CpuFrequencyObservation::Value(CpuFrequencyDiagnostics {
                driver: "amd-pstate-epp".into(),
                available_epp_preferences: vec!["power".into(), "performance".into()],
                current_epp_preference: "power".into(),
                boost: false,
            })
        );
    }
}
