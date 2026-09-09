//! Pure presentation DTO projection for Diagnostics UI consumers.
//!
//! This module never queries providers, D-Bus, sysfs, workers, or Slint. It
//! projects one immutable `DiagnosticsSnapshot` into owned presentation data so
//! later UI wiring can remain a consumer of a point-in-time snapshot only.

use std::time::SystemTime;

use orbis_core::capability::{CapabilityStatus, FeatureId};
use orbis_core::diagnostics::{
    CpuPackagePowerLimitsObservation, DiagnosticsServiceId, DiagnosticsSnapshot,
    DisplayDiagnostics, DisplayProtocol, GpuDiagnostics, ServiceAvailability, ServiceBusScope,
    ServiceCriticality, SessionType, TelemetryDiagnostics,
};

/// Compact application/system/hardware values intended for the Diagnostics
/// summary area. Optional source values remain optional; no placeholder such as
/// `Unknown`, `Linux`, or `ASUS` is synthesized here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsSummaryDto {
    /// Exact running package version from the diagnostics snapshot.
    pub package_version: String,
    /// Embedded build revision only when the source supplied one.
    pub build_revision: Option<String>,
    /// Embedded build channel only when the source supplied one.
    pub build_channel: Option<String>,
    /// Exact kernel release when available.
    pub kernel_release: Option<String>,
    /// Running architecture when available.
    pub architecture: Option<String>,
    /// Normalized session type.
    pub session_type: SessionType,
    /// Normalized display protocol.
    pub display_protocol: DisplayProtocol,
    /// Reliable compositor identity only when present in the source snapshot.
    pub compositor: Option<String>,
    /// Privacy-safe DMI vendor.
    pub hardware_vendor: Option<String>,
    /// Privacy-safe DMI product/model.
    pub hardware_product: Option<String>,
    /// Privacy-safe DMI board/model identifier.
    pub hardware_board: Option<String>,
    /// BIOS version when present.
    pub bios_version: Option<String>,
    /// BIOS date when present.
    pub bios_date: Option<String>,
}

/// One canonical capability row for presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRowDto {
    /// Stable core-owned feature identifier.
    pub feature: FeatureId,
    /// Canonical feature-level status.
    pub status: CapabilityStatus,
    /// Independent authoritative read-operation status.
    pub read_status: CapabilityStatus,
    /// Independent write-operation status.
    pub write_status: CapabilityStatus,
}

/// One normalized service-presence row for presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRowDto {
    /// Stable Orbis-owned service identifier.
    pub service: DiagnosticsServiceId,
    /// Bus on which presence was checked.
    pub bus: ServiceBusScope,
    /// Presence/availability classification.
    pub availability: ServiceAvailability,
    /// Architecture-owned criticality classification.
    pub criticality: ServiceCriticality,
    /// Source-owned check timestamp.
    pub checked_at: Option<SystemTime>,
}

/// Owned presentation projection of one immutable diagnostics snapshot.
///
/// GPU, telemetry, and display sections remain typed because their domain
/// `Unknown` values and observation failures must stay distinguishable until a
/// later explicit UI-status formatting layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsUiDto {
    /// Aggregate generation timestamp.
    pub generated_at: SystemTime,
    /// Compact summary fields.
    pub summary: DiagnosticsSummaryDto,
    /// Capability registry generation.
    pub capability_generation: u64,
    /// Capability registry check timestamp.
    pub capability_checked_at: SystemTime,
    /// Canonical capability rows in `DeviceCapabilities` deterministic order.
    pub capabilities: Vec<CapabilityRowDto>,
    /// Service rows in snapshot order.
    pub services: Vec<ServiceRowDto>,
    /// Independent GPU primitive observations.
    pub gpu: GpuDiagnostics,
    /// Read-only CPU package power-limit observation.
    pub cpu_package_power_limits: CpuPackagePowerLimitsObservation,
    /// Telemetry values and collection/freshness metadata.
    pub telemetry: TelemetryDiagnostics,
    /// Read-only display/output observation.
    pub display: DisplayDiagnostics,
}

/// Field-local telemetry gap evidence as stable display labels (#117).
///
/// Reads the typed `Telemetry.field_gaps` carried by the latest sample; a
/// missing sample yields an empty list. Labels use the existing `{:?}` style
/// of the surrounding diagnostics text; no paths or identifiers are involved.
pub fn telemetry_field_gap_labels(dto: &DiagnosticsUiDto) -> Vec<(String, String)> {
    match &dto.telemetry.latest {
        Some(sample) => sample
            .field_gaps
            .iter()
            .map(|(field, gap)| (format!("{field:?}"), format!("{gap:?}")))
            .collect(),
        None => Vec::new(),
    }
}

impl DiagnosticsUiDto {
    /// Project a frozen diagnostics snapshot into owned presentation data.
    ///
    /// This is a pure clone/projection operation. It performs no I/O, probing,
    /// refresh, capability inference, or hardware interaction.
    pub fn from_snapshot(snapshot: &DiagnosticsSnapshot) -> Self {
        let sections = snapshot.sections();
        let identity = sections.hardware.identity.as_ref();

        let capabilities = sections
            .capabilities
            .capabilities
            .features
            .iter()
            .map(|(feature, capability)| CapabilityRowDto {
                feature: *feature,
                status: capability.status,
                read_status: capability.operations.read.status,
                write_status: capability.operations.write.status,
            })
            .collect();

        let services = sections
            .services
            .iter()
            .map(|service| ServiceRowDto {
                service: service.service,
                bus: service.bus,
                availability: service.availability,
                criticality: service.criticality,
                checked_at: service.checked_at,
            })
            .collect();

        Self {
            generated_at: snapshot.generated_at(),
            summary: DiagnosticsSummaryDto {
                package_version: sections.application.package_version.clone(),
                build_revision: sections.application.build_revision.clone(),
                build_channel: sections.application.build_channel.clone(),
                kernel_release: sections.system.kernel_release.clone(),
                architecture: sections.system.architecture.clone(),
                session_type: sections.system.session_type,
                display_protocol: sections.system.display_protocol,
                compositor: sections.system.compositor.clone(),
                hardware_vendor: identity.map(|value| value.vendor.clone()),
                hardware_product: identity.map(|value| value.product.clone()),
                hardware_board: identity.map(|value| value.board.clone()),
                bios_version: identity.map(|value| value.bios_version.clone()),
                bios_date: identity.map(|value| value.bios_date.clone()),
            },
            capability_generation: sections.capabilities.generation,
            capability_checked_at: sections.capabilities.checked_at,
            capabilities,
            services,
            gpu: sections.gpu.clone(),
            cpu_package_power_limits: sections.cpu_package_power_limits.clone(),
            telemetry: sections.telemetry.clone(),
            display: sections.display.clone(),
        }
    }
}

impl From<&DiagnosticsSnapshot> for DiagnosticsUiDto {
    fn from(snapshot: &DiagnosticsSnapshot) -> Self {
        Self::from_snapshot(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use orbis_application::diagnostics::DiagnosticsCollector;
    use orbis_core::capability::{
        Capability, CapabilityOperations, DeviceCapabilities, OperationCapability,
    };
    use orbis_core::diagnostics::{
        ApplicationDiagnostics, CapabilitySnapshotDiagnostics, DiagnosticObservation,
        DisplayDiagnostics, GpuDiagnostics, HardwareDiagnostics, ServiceDiagnostics,
        SystemDiagnostics, TelemetryCollectionStatus, TelemetryFreshness,
    };
    use orbis_core::display_output::DisplayOutputSnapshot;
    use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
    use orbis_core::identity::DeviceIdentity;
    use orbis_core::telemetry::Telemetry;

    use super::*;

    fn capability(
        status: CapabilityStatus,
        read: CapabilityStatus,
        write: CapabilityStatus,
    ) -> Capability {
        Capability::new(status).with_operations(CapabilityOperations {
            read: OperationCapability::new(read),
            write: OperationCapability::new(write),
        })
    }

    fn sample_snapshot(with_identity: bool) -> DiagnosticsSnapshot {
        let generated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);
        let mut capabilities = DeviceCapabilities::default();
        // Insert out of enum order to prove the DTO follows the deterministic
        // BTreeMap order rather than insertion order.
        capabilities.features.insert(
            FeatureId::Aura,
            capability(
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
        );
        capabilities.features.insert(
            FeatureId::Performance,
            capability(
                CapabilityStatus::ReadOnly,
                CapabilityStatus::Supported,
                CapabilityStatus::ReadOnly,
            ),
        );

        let identity = with_identity.then(|| DeviceIdentity {
            vendor: "ASUSTeK COMPUTER INC.".into(),
            product: "Example Model".into(),
            board: "EXAMPLE-BOARD".into(),
            bios_version: "1.2.3".into(),
            bios_date: "2026-01-01".into(),
        });

        DiagnosticsCollector::new().collect(
            generated_at,
            ApplicationDiagnostics {
                package_version: "0.1.0-test".into(),
                build_revision: Some("deadbeef".into()),
                build_channel: None,
            },
            SystemDiagnostics {
                kernel_release: Some("6.12-test".into()),
                architecture: Some("x86_64".into()),
                session_type: SessionType::Wayland,
                display_protocol: DisplayProtocol::Wayland,
                compositor: None,
            },
            HardwareDiagnostics { identity },
            vec![
                ServiceDiagnostics {
                    service: DiagnosticsServiceId::Asusd,
                    bus: ServiceBusScope::System,
                    availability: ServiceAvailability::Unavailable,
                    criticality: ServiceCriticality::CapabilityLocal,
                    checked_at: Some(checked_at),
                },
                ServiceDiagnostics {
                    service: DiagnosticsServiceId::OrbisSessiond,
                    bus: ServiceBusScope::Session,
                    availability: ServiceAvailability::Running,
                    criticality: ServiceCriticality::CoreReadPath,
                    checked_at: Some(checked_at),
                },
            ],
            CapabilitySnapshotDiagnostics {
                generation: 17,
                checked_at,
                capabilities,
            },
            GpuDiagnostics {
                mux: DiagnosticObservation::Value(GpuMuxState::Unknown),
                access_policy: DiagnosticObservation::PermissionDenied,
                runtime_power: DiagnosticObservation::Value(GpuPowerState::Stale),
                nvidia: DiagnosticObservation::Unknown,
            },
            CpuPackagePowerLimitsObservation::Unknown,
            TelemetryDiagnostics {
                latest: Some(Telemetry::empty()),
                status: TelemetryCollectionStatus::Degraded,
                last_attempt_at: Some(checked_at),
                last_success_at: Some(checked_at),
                freshness: TelemetryFreshness::Stale,
            },
            DisplayDiagnostics {
                outputs: DiagnosticObservation::Value(DisplayOutputSnapshot {
                    outputs: Vec::new(),
                }),
                checked_at: Some(checked_at),
            },
        )
    }

    #[test]
    fn projects_real_summary_values_without_preview_fallbacks() {
        let dto = DiagnosticsUiDto::from_snapshot(&sample_snapshot(true));

        assert_eq!(dto.summary.package_version, "0.1.0-test");
        assert_eq!(dto.summary.kernel_release.as_deref(), Some("6.12-test"));
        assert_eq!(dto.summary.architecture.as_deref(), Some("x86_64"));
        assert_eq!(
            dto.summary.hardware_vendor.as_deref(),
            Some("ASUSTeK COMPUTER INC.")
        );
        assert_eq!(
            dto.summary.hardware_product.as_deref(),
            Some("Example Model")
        );
        assert_eq!(dto.summary.compositor, None);
    }

    #[test]
    fn missing_optional_source_values_remain_none() {
        let dto = DiagnosticsUiDto::from_snapshot(&sample_snapshot(false));

        assert!(dto.summary.hardware_vendor.is_none());
        assert!(dto.summary.hardware_product.is_none());
        assert!(dto.summary.hardware_board.is_none());
        assert!(dto.summary.bios_version.is_none());
        assert!(dto.summary.bios_date.is_none());
        assert!(dto.summary.compositor.is_none());
    }

    #[test]
    fn capability_rows_preserve_overall_read_and_write_statuses_separately() {
        let dto = DiagnosticsUiDto::from_snapshot(&sample_snapshot(true));

        assert_eq!(dto.capability_generation, 17);
        assert_eq!(dto.capabilities.len(), 2);
        assert_eq!(dto.capabilities[0].feature, FeatureId::Performance);
        assert_eq!(dto.capabilities[0].status, CapabilityStatus::ReadOnly);
        assert_eq!(dto.capabilities[0].read_status, CapabilityStatus::Supported);
        assert_eq!(dto.capabilities[0].write_status, CapabilityStatus::ReadOnly);
        assert_eq!(dto.capabilities[1].feature, FeatureId::Aura);
        assert_eq!(dto.capabilities[1].status, CapabilityStatus::BackendMissing);
    }

    #[test]
    fn service_rows_preserve_snapshot_order_and_distinct_availability() {
        let dto = DiagnosticsUiDto::from_snapshot(&sample_snapshot(true));

        assert_eq!(dto.services.len(), 2);
        assert_eq!(dto.services[0].service, DiagnosticsServiceId::Asusd);
        assert_eq!(
            dto.services[0].availability,
            ServiceAvailability::Unavailable
        );
        assert_eq!(dto.services[1].service, DiagnosticsServiceId::OrbisSessiond);
        assert_eq!(dto.services[1].availability, ServiceAvailability::Running);
    }

    #[test]
    fn typed_gpu_telemetry_and_display_states_are_not_collapsed_to_labels() {
        let dto = DiagnosticsUiDto::from_snapshot(&sample_snapshot(true));

        assert_eq!(
            dto.gpu.mux,
            DiagnosticObservation::Value(GpuMuxState::Unknown)
        );
        assert_eq!(
            dto.gpu.access_policy,
            DiagnosticObservation::<GpuAccessPolicy>::PermissionDenied
        );
        assert_eq!(
            dto.gpu.runtime_power,
            DiagnosticObservation::Value(GpuPowerState::Stale)
        );
        assert_eq!(dto.telemetry.status, TelemetryCollectionStatus::Degraded);
        assert_eq!(dto.telemetry.freshness, TelemetryFreshness::Stale);
        assert!(matches!(
            dto.display.outputs,
            DiagnosticObservation::Value(DisplayOutputSnapshot { .. })
        ));
    }

    #[test]
    fn projection_is_owned_and_does_not_mutate_source_snapshot() {
        let snapshot = sample_snapshot(true);
        let mut dto = DiagnosticsUiDto::from_snapshot(&snapshot);
        dto.summary.package_version = "changed-for-ui".into();
        dto.services.clear();

        assert_eq!(
            snapshot.sections().application.package_version,
            "0.1.0-test"
        );
        assert_eq!(snapshot.sections().services.len(), 2);
    }
}
