use std::time::{Duration, SystemTime};

use orbis_application::diagnostics::DiagnosticsCollector;
use orbis_core::capability::DeviceCapabilities;
use orbis_core::diagnostics::{
    ApplicationDiagnostics, CapabilitySnapshotDiagnostics, DiagnosticObservation,
    DisplayDiagnostics, DisplayProtocol, GpuDiagnostics, HardwareDiagnostics, SessionType,
    SystemDiagnostics, TelemetryCollectionStatus, TelemetryDiagnostics, TelemetryFreshness,
};
use orbis_ui::diagnostics_dto::DiagnosticsUiDto;
use orbis_ui::diagnostics_export::{report_json, summary_text};

#[test]
fn typed_snapshot_projects_through_privacy_bounded_exports() {
    let generated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);
    let snapshot = DiagnosticsCollector::new().collect(
        generated_at,
        ApplicationDiagnostics {
            package_version: "0.1.0-integration".into(),
            build_revision: Some("deadbeef".into()),
            build_channel: Some("test".into()),
        },
        SystemDiagnostics {
            kernel_release: Some("6.12-test".into()),
            architecture: Some("x86_64".into()),
            session_type: SessionType::Wayland,
            display_protocol: DisplayProtocol::Wayland,
            compositor: None,
        },
        HardwareDiagnostics { identity: None },
        Vec::new(),
        CapabilitySnapshotDiagnostics {
            generation: 4,
            checked_at,
            capabilities: DeviceCapabilities::default(),
        },
        GpuDiagnostics {
            mux: DiagnosticObservation::Unknown,
            access_policy: DiagnosticObservation::PermissionDenied,
            runtime_power: DiagnosticObservation::Unavailable,
            nvidia: DiagnosticObservation::Unknown,
        },
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
    );

    let dto = DiagnosticsUiDto::from_snapshot(&snapshot);
    let text = summary_text(&dto);
    let json = report_json(&dto).expect("allowlisted diagnostics JSON");

    assert_eq!(dto.summary.package_version, "0.1.0-integration");
    assert!(dto.summary.hardware_vendor.is_none());
    assert!(text.contains("vendor=unknown"));
    assert!(text.contains("mux=Unknown"));
    assert!(text.contains("access_policy=PermissionDenied"));
    assert!(text.contains("runtime_power=Unavailable"));
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"capability_generation\": 4"));

    for forbidden in ["serial", "uuid", "asset_tag", "journal", "environment"] {
        assert!(!text.to_ascii_lowercase().contains(forbidden));
        assert!(!json.to_ascii_lowercase().contains(forbidden));
    }
}
