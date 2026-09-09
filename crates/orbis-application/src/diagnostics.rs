//! Application-owned immutable diagnostics snapshot assembly.
//!
//! Source-specific reads, probes, freshness decisions, and failure
//! classification happen before this boundary. The collector accepts
//! only typed diagnostics sections and freezes them into one immutable
//! `DiagnosticsSnapshot`. Because source failures are already encoded in
//! section-local enums/statuses, collection itself is intentionally
//! infallible and cannot turn one backend failure into a global failure.

use std::time::SystemTime;

use orbis_core::diagnostics::{
    ApplicationDiagnostics, CapabilitySnapshotDiagnostics, CpuPackagePowerLimitsObservation,
    DiagnosticsSnapshot, DiagnosticsSnapshotSections, DisplayDiagnostics, GpuDiagnostics,
    HardwareDiagnostics, ServiceDiagnostics, SystemDiagnostics, TelemetryDiagnostics,
};

/// Application-layer boundary that freezes already-collected typed
/// diagnostics into one immutable point-in-time snapshot.
///
/// The collector owns no provider, D-Bus connection, runtime, polling
/// loop, or mutation API. Callers must first translate source outcomes
/// into the typed diagnostics sections. This keeps capability-local
/// failures local and makes snapshot assembly itself infallible.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiagnosticsCollector;

impl DiagnosticsCollector {
    /// Create a stateless diagnostics collector.
    pub const fn new() -> Self {
        Self
    }

    /// Freeze one point-in-time diagnostics aggregate.
    ///
    /// No source is queried here. `Unavailable`, `PermissionDenied`,
    /// successful domain `Unknown`, stale telemetry, and per-capability
    /// statuses are preserved exactly as supplied.
    #[allow(clippy::too_many_arguments)]
    pub fn collect(
        &self,
        generated_at: SystemTime,
        application: ApplicationDiagnostics,
        system: SystemDiagnostics,
        hardware: HardwareDiagnostics,
        services: Vec<ServiceDiagnostics>,
        capabilities: CapabilitySnapshotDiagnostics,
        gpu: GpuDiagnostics,
        cpu_package_power_limits: CpuPackagePowerLimitsObservation,
        telemetry: TelemetryDiagnostics,
        display: DisplayDiagnostics,
    ) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot::new(
            generated_at,
            DiagnosticsSnapshotSections {
                application,
                system,
                hardware,
                services,
                capabilities,
                gpu,
                cpu_package_power_limits,
                telemetry,
                display,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use orbis_core::capability::DeviceCapabilities;
    use orbis_core::diagnostics::{
        DIAGNOSTICS_SNAPSHOT_SCHEMA_VERSION, DiagnosticObservation, DiagnosticsServiceId,
        DisplayProtocol, ServiceAvailability, ServiceBusScope, ServiceCriticality, SessionType,
        TelemetryCollectionStatus, TelemetryFreshness,
    };
    use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
    use orbis_core::telemetry::Telemetry;

    use super::*;

    fn snapshot_with_service(availability: ServiceAvailability) -> DiagnosticsSnapshot {
        let generated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);

        DiagnosticsCollector::new().collect(
            generated_at,
            ApplicationDiagnostics {
                package_version: "0.1.0-test".into(),
                build_revision: None,
                build_channel: None,
            },
            SystemDiagnostics {
                kernel_release: Some("6.12-test".into()),
                architecture: Some("x86_64".into()),
                session_type: SessionType::Wayland,
                display_protocol: DisplayProtocol::Wayland,
                compositor: None,
            },
            HardwareDiagnostics { identity: None },
            vec![ServiceDiagnostics {
                service: DiagnosticsServiceId::OrbisHardwared,
                bus: ServiceBusScope::System,
                availability,
                criticality: ServiceCriticality::CapabilityLocal,
                checked_at: Some(checked_at),
            }],
            CapabilitySnapshotDiagnostics {
                generation: 7,
                checked_at,
                capabilities: DeviceCapabilities::default(),
            },
            GpuDiagnostics {
                mux: DiagnosticObservation::Value(GpuMuxState::Unknown),
                access_policy: DiagnosticObservation::PermissionDenied,
                runtime_power: DiagnosticObservation::Unavailable,
                nvidia: DiagnosticObservation::Unknown,
            },
            CpuPackagePowerLimitsObservation::Unknown,
            TelemetryDiagnostics {
                latest: None,
                status: TelemetryCollectionStatus::Unavailable,
                last_attempt_at: Some(checked_at),
                last_success_at: None,
                freshness: TelemetryFreshness::Unknown,
            },
            DisplayDiagnostics {
                outputs: DiagnosticObservation::Unknown,
                checked_at: Some(checked_at),
            },
        )
    }

    #[test]
    fn freezes_exact_generated_at_schema_and_source_metadata() {
        let snapshot = snapshot_with_service(ServiceAvailability::Running);
        let expected_generated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let expected_checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);

        assert_eq!(
            snapshot.schema_version(),
            DIAGNOSTICS_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(snapshot.generated_at(), expected_generated_at);
        assert_eq!(
            snapshot.sections().application.package_version,
            "0.1.0-test"
        );
        assert_eq!(snapshot.sections().capabilities.generation, 7);
        assert_eq!(
            snapshot.sections().capabilities.checked_at,
            expected_checked_at
        );
        assert_eq!(
            snapshot.sections().services[0].checked_at,
            Some(expected_checked_at)
        );
    }

    #[test]
    fn capability_local_failure_states_do_not_collapse_snapshot() {
        let mut last_good = Telemetry::empty();
        last_good.ts = SystemTime::UNIX_EPOCH + Duration::from_secs(40);
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);

        let snapshot = DiagnosticsCollector::new().collect(
            SystemTime::UNIX_EPOCH + Duration::from_secs(100),
            ApplicationDiagnostics {
                package_version: "0.1.0-test".into(),
                build_revision: None,
                build_channel: None,
            },
            SystemDiagnostics {
                kernel_release: None,
                architecture: Some("x86_64".into()),
                session_type: SessionType::Unknown,
                display_protocol: DisplayProtocol::Unknown,
                compositor: None,
            },
            HardwareDiagnostics { identity: None },
            vec![ServiceDiagnostics {
                service: DiagnosticsServiceId::Asusd,
                bus: ServiceBusScope::System,
                availability: ServiceAvailability::Unavailable,
                criticality: ServiceCriticality::CapabilityLocal,
                checked_at: Some(checked_at),
            }],
            CapabilitySnapshotDiagnostics {
                generation: 11,
                checked_at,
                capabilities: DeviceCapabilities::default(),
            },
            GpuDiagnostics {
                mux: DiagnosticObservation::Value(GpuMuxState::Unknown),
                access_policy: DiagnosticObservation::PermissionDenied,
                runtime_power: DiagnosticObservation::Unavailable,
                nvidia: DiagnosticObservation::Unknown,
            },
            CpuPackagePowerLimitsObservation::Unknown,
            TelemetryDiagnostics {
                latest: Some(last_good.clone()),
                status: TelemetryCollectionStatus::Degraded,
                last_attempt_at: Some(checked_at),
                last_success_at: Some(last_good.ts),
                freshness: TelemetryFreshness::Stale,
            },
            DisplayDiagnostics {
                outputs: DiagnosticObservation::Unknown,
                checked_at: Some(checked_at),
            },
        );

        let sections = snapshot.sections();
        assert_eq!(
            sections.services[0].availability,
            ServiceAvailability::Unavailable
        );
        assert_eq!(sections.capabilities.generation, 11);
        assert_eq!(
            sections.gpu.mux,
            DiagnosticObservation::Value(GpuMuxState::Unknown)
        );
        assert_eq!(
            sections.gpu.access_policy,
            DiagnosticObservation::PermissionDenied
        );
        assert_eq!(
            sections.gpu.runtime_power,
            DiagnosticObservation::Unavailable
        );
        assert_eq!(sections.telemetry.latest, Some(last_good));
        assert_eq!(
            sections.telemetry.status,
            TelemetryCollectionStatus::Degraded
        );
        assert_eq!(sections.telemetry.freshness, TelemetryFreshness::Stale);
        assert_eq!(sections.display.outputs, DiagnosticObservation::Unknown);
    }

    #[test]
    fn successful_domain_unknown_remains_distinct_from_failed_read() {
        let snapshot = snapshot_with_service(ServiceAvailability::Running);

        assert_eq!(
            snapshot.sections().gpu.mux,
            DiagnosticObservation::Value(GpuMuxState::Unknown)
        );
        assert_eq!(
            snapshot.sections().gpu.access_policy,
            DiagnosticObservation::<GpuAccessPolicy>::PermissionDenied
        );
        assert_eq!(
            snapshot.sections().gpu.runtime_power,
            DiagnosticObservation::<GpuPowerState>::Unavailable
        );
    }

    #[test]
    fn later_collection_cannot_mutate_an_earlier_snapshot() {
        let first = snapshot_with_service(ServiceAvailability::Unavailable);
        let second = snapshot_with_service(ServiceAvailability::Running);

        assert_eq!(
            first.sections().services[0].availability,
            ServiceAvailability::Unavailable
        );
        assert_eq!(
            second.sections().services[0].availability,
            ServiceAvailability::Running
        );
    }
}
