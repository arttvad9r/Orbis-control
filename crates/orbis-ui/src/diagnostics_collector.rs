//! Application-owned diagnostics snapshot aggregation.
//!
//! This module is deliberately Slint-free and I/O-free. Source-specific code
//! (metadata providers, service-presence checks, capability projection, GPU /
//! telemetry / display adapters) produces typed sections first. The collector
//! then freezes those independently classified results into one immutable
//! `DiagnosticsSnapshot`.

use std::time::SystemTime;

use orbis_core::diagnostics::{
    ApplicationDiagnostics, CapabilitySnapshotDiagnostics, DiagnosticsSnapshot,
    DiagnosticsSnapshotSections, DisplayDiagnostics, GpuDiagnostics, HardwareDiagnostics,
    ServiceDiagnostics, SystemDiagnostics, TelemetryDiagnostics,
};

/// Freeze one point-in-time diagnostics aggregate from already-typed sources.
///
/// This function intentionally has no `Result` return: source-local failures
/// must already be represented by the typed diagnostics inputs (`Unknown`,
/// `Unavailable`, `PermissionDenied`, capability-specific statuses, telemetry
/// status/freshness, and so on). Therefore one unavailable optional source does
/// not prevent publication of unrelated diagnostics sections.
///
/// No provider calls, D-Bus calls, filesystem access, hardware access, polling,
/// or UI formatting occur here.
#[allow(clippy::too_many_arguments)]
pub fn collect_diagnostics_snapshot(
    generated_at: SystemTime,
    application: ApplicationDiagnostics,
    system: SystemDiagnostics,
    hardware: HardwareDiagnostics,
    services: Vec<ServiceDiagnostics>,
    capabilities: CapabilitySnapshotDiagnostics,
    gpu: GpuDiagnostics,
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
            telemetry,
            display,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use orbis_core::{
        capability::{Capability, CapabilityStatus, DeviceCapabilities, FeatureId},
        diagnostics::{
            DiagnosticObservation, DiagnosticsServiceId, DisplayProtocol, ServiceAvailability,
            ServiceBusScope, ServiceCriticality, SessionType, TelemetryCollectionStatus,
            TelemetryFreshness,
        },
        gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState},
    };

    use super::*;

    fn application() -> ApplicationDiagnostics {
        ApplicationDiagnostics {
            package_version: "1.2.3".into(),
            build_revision: None,
            build_channel: Some("beta".into()),
        }
    }

    fn system() -> SystemDiagnostics {
        SystemDiagnostics {
            kernel_release: Some("6.12.7-orbis".into()),
            architecture: Some("x86_64".into()),
            session_type: SessionType::Wayland,
            display_protocol: DisplayProtocol::Wayland,
            compositor: None,
        }
    }

    fn capabilities(checked_at: SystemTime) -> CapabilitySnapshotDiagnostics {
        let mut device = DeviceCapabilities::default();
        device.features.insert(
            FeatureId::PanelOverdrive,
            Capability::new(CapabilityStatus::Unsupported),
        );
        CapabilitySnapshotDiagnostics {
            generation: 17,
            checked_at,
            capabilities: device,
        }
    }

    #[test]
    fn freezes_all_typed_sections_without_reclassification() {
        let generated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let capability_checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);
        let service_checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(95);

        let services = vec![ServiceDiagnostics {
            service: DiagnosticsServiceId::OrbisSessiond,
            bus: ServiceBusScope::Session,
            availability: ServiceAvailability::Running,
            criticality: ServiceCriticality::CoreReadPath,
            checked_at: Some(service_checked_at),
        }];
        let gpu = GpuDiagnostics {
            mux: DiagnosticObservation::Value(GpuMuxState::Discrete),
            access_policy: DiagnosticObservation::Value(GpuAccessPolicy::Unblocked),
            runtime_power: DiagnosticObservation::Value(GpuPowerState::Suspended),
        };
        let telemetry = TelemetryDiagnostics {
            latest: None,
            status: TelemetryCollectionStatus::Unknown,
            last_attempt_at: None,
            last_success_at: None,
            freshness: TelemetryFreshness::Unknown,
        };
        let display = DisplayDiagnostics {
            outputs: DiagnosticObservation::Unknown,
            checked_at: None,
        };

        let snapshot = collect_diagnostics_snapshot(
            generated_at,
            application(),
            system(),
            HardwareDiagnostics { identity: None },
            services.clone(),
            capabilities(capability_checked_at),
            gpu.clone(),
            telemetry.clone(),
            display.clone(),
        );

        assert_eq!(snapshot.generated_at(), generated_at);
        assert_eq!(snapshot.schema_version(), 1);
        let sections = snapshot.sections();
        assert_eq!(sections.application, application());
        assert_eq!(sections.system, system());
        assert_eq!(sections.hardware, HardwareDiagnostics { identity: None });
        assert_eq!(sections.services, services);
        assert_eq!(sections.capabilities.generation, 17);
        assert_eq!(sections.capabilities.checked_at, capability_checked_at);
        assert_eq!(sections.gpu, gpu);
        assert_eq!(sections.telemetry, telemetry);
        assert_eq!(sections.display, display);
    }

    #[test]
    fn source_local_failures_do_not_block_unrelated_sections() {
        let generated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(180);

        let services = vec![
            ServiceDiagnostics {
                service: DiagnosticsServiceId::Asusd,
                bus: ServiceBusScope::System,
                availability: ServiceAvailability::Unavailable,
                criticality: ServiceCriticality::CapabilityLocal,
                checked_at: Some(checked_at),
            },
            ServiceDiagnostics {
                service: DiagnosticsServiceId::Supergfxd,
                bus: ServiceBusScope::System,
                availability: ServiceAvailability::PermissionDenied,
                criticality: ServiceCriticality::Optional,
                checked_at: Some(checked_at),
            },
        ];
        let gpu = GpuDiagnostics {
            mux: DiagnosticObservation::PermissionDenied,
            access_policy: DiagnosticObservation::Unavailable,
            runtime_power: DiagnosticObservation::Value(GpuPowerState::Unknown),
        };
        let telemetry = TelemetryDiagnostics {
            latest: None,
            status: TelemetryCollectionStatus::Unavailable,
            last_attempt_at: Some(checked_at),
            last_success_at: None,
            freshness: TelemetryFreshness::Unknown,
        };
        let display = DisplayDiagnostics {
            outputs: DiagnosticObservation::Unavailable,
            checked_at: Some(checked_at),
        };

        let snapshot = collect_diagnostics_snapshot(
            generated_at,
            application(),
            system(),
            HardwareDiagnostics { identity: None },
            services,
            capabilities(checked_at),
            gpu,
            telemetry,
            display,
        );
        let sections = snapshot.sections();

        assert_eq!(
            sections.services[0].availability,
            ServiceAvailability::Unavailable
        );
        assert_eq!(
            sections.services[1].availability,
            ServiceAvailability::PermissionDenied
        );
        assert_eq!(sections.gpu.mux, DiagnosticObservation::PermissionDenied);
        assert_eq!(
            sections.gpu.access_policy,
            DiagnosticObservation::Unavailable
        );
        assert_eq!(
            sections.gpu.runtime_power,
            DiagnosticObservation::Value(GpuPowerState::Unknown)
        );
        assert_eq!(
            sections.telemetry.status,
            TelemetryCollectionStatus::Unavailable
        );
        assert_eq!(
            sections.display.outputs,
            DiagnosticObservation::Unavailable
        );
        // An unrelated capability remains independently classified rather than
        // being replaced by a global backend state.
        assert_eq!(
            sections.capabilities.capabilities.status(FeatureId::PanelOverdrive),
            CapabilityStatus::Unsupported
        );
        assert_eq!(sections.application.package_version, "1.2.3");
    }

    #[test]
    fn collector_does_not_synthesize_missing_sources() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let snapshot = collect_diagnostics_snapshot(
            checked_at,
            application(),
            SystemDiagnostics {
                kernel_release: None,
                architecture: None,
                session_type: SessionType::Unknown,
                display_protocol: DisplayProtocol::Unknown,
                compositor: None,
            },
            HardwareDiagnostics { identity: None },
            Vec::new(),
            CapabilitySnapshotDiagnostics {
                generation: 0,
                checked_at,
                capabilities: DeviceCapabilities::default(),
            },
            GpuDiagnostics {
                mux: DiagnosticObservation::Unknown,
                access_policy: DiagnosticObservation::Unknown,
                runtime_power: DiagnosticObservation::Unknown,
            },
            TelemetryDiagnostics {
                latest: None,
                status: TelemetryCollectionStatus::Unknown,
                last_attempt_at: None,
                last_success_at: None,
                freshness: TelemetryFreshness::Unknown,
            },
            DisplayDiagnostics {
                outputs: DiagnosticObservation::Unknown,
                checked_at: None,
            },
        );

        let sections = snapshot.sections();
        assert!(sections.system.kernel_release.is_none());
        assert!(sections.system.architecture.is_none());
        assert!(sections.hardware.identity.is_none());
        assert!(sections.services.is_empty());
        assert!(sections.capabilities.capabilities.features.is_empty());
        assert!(sections.telemetry.latest.is_none());
        assert_eq!(sections.display.outputs, DiagnosticObservation::Unknown);
    }
}
