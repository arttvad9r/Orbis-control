//! Pure formatting model for the production Diagnostics window.
//!
//! The model consumes only `DiagnosticsUiDto`; it never performs I/O or calls a
//! provider. Status styling/UX refinement remains a later layer, but all text
//! here comes from the real typed snapshot rather than preview literals.

use std::fmt::Debug;

use orbis_core::diagnostics::DiagnosticObservation;

use crate::diagnostics_dto::DiagnosticsUiDto;

/// String properties consumed by `DiagnosticsWindow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsWindowModel {
    /// Exact kernel release or explicit absence marker.
    pub kernel: String,
    /// Privacy-safe vendor/product identity or explicit absence marker.
    pub platform: String,
    /// Exact running package version.
    pub version: String,
    /// Optional build metadata only when embedded.
    pub build: String,
    /// Architecture/session/display protocol summary.
    pub system: String,
    /// Canonical capability rows including separate read/write states.
    pub capabilities: String,
    /// D-Bus service-presence observations.
    pub services: String,
    /// Independent GPU primitive observations.
    pub gpu: String,
    /// Telemetry collection/freshness state.
    pub telemetry: String,
    /// Read-only display-output observation.
    pub display: String,
    /// Capability generation shown as snapshot metadata.
    pub snapshot_meta: String,
}

impl DiagnosticsWindowModel {
    /// Format one immutable presentation DTO for Slint string properties.
    pub fn from_dto(dto: &DiagnosticsUiDto) -> Self {
        let summary = &dto.summary;
        let kernel = summary
            .kernel_release
            .clone()
            .unwrap_or_else(|| "Unknown".into());

        let platform = match (&summary.hardware_vendor, &summary.hardware_product) {
            (Some(vendor), Some(product)) => format!("{vendor} · {product}"),
            (Some(vendor), None) => vendor.clone(),
            (None, Some(product)) => product.clone(),
            (None, None) => "Unknown".into(),
        };

        let mut build_parts = Vec::new();
        if let Some(channel) = &summary.build_channel {
            build_parts.push(format!("channel={channel}"));
        }
        if let Some(revision) = &summary.build_revision {
            build_parts.push(format!("revision={revision}"));
        }
        let build = if build_parts.is_empty() {
            "Build metadata not embedded".into()
        } else {
            build_parts.join(" · ")
        };

        let mut system_parts = Vec::new();
        if let Some(architecture) = &summary.architecture {
            system_parts.push(architecture.clone());
        }
        system_parts.push(format!("session={:?}", summary.session_type));
        system_parts.push(format!("display={:?}", summary.display_protocol));
        if let Some(compositor) = &summary.compositor {
            system_parts.push(format!("compositor={compositor}"));
        }
        if let Some(board) = &summary.hardware_board {
            system_parts.push(format!("board={board}"));
        }
        if let Some(bios) = &summary.bios_version {
            system_parts.push(format!("BIOS={bios}"));
        }
        let system = system_parts.join(" · ");

        let capabilities = if dto.capabilities.is_empty() {
            "No capability entries".into()
        } else {
            dto.capabilities
                .iter()
                .map(|row| {
                    format!(
                        "{}: {:?} · read={:?} · write={:?}",
                        row.feature.as_str(),
                        row.status,
                        row.read_status,
                        row.write_status
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let services = if dto.services.is_empty() {
            "No service observations".into()
        } else {
            dto.services
                .iter()
                .map(|row| format!("{:?} ({:?}): {:?}", row.service, row.bus, row.availability))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let gpu = format!(
            "MUX: {}\nAccess: {}\nRuntime power: {}\nNVIDIA: {}",
            observation_label(&dto.gpu.mux),
            observation_label(&dto.gpu.access_policy),
            observation_label(&dto.gpu.runtime_power),
            nvidia_label(&dto.gpu.nvidia),
        );

        let mut telemetry = format!(
            "status={:?} · quality={:?} · freshness={:?} · sample={}",
            dto.telemetry.status,
            dto.telemetry.quality(),
            dto.telemetry.freshness,
            if dto.telemetry.latest.is_some() {
                "available"
            } else {
                "none"
            }
        );
        let gap_labels = crate::diagnostics_dto::telemetry_field_gap_labels(dto);
        if gap_labels.is_empty() {
            telemetry.push_str(" · gaps: none");
        } else {
            telemetry.push_str(&format!(
                "\ngaps: {}",
                gap_labels
                    .iter()
                    .map(|(field, gap)| format!("{field}={gap}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
        }

        let display = match &dto.display.outputs {
            DiagnosticObservation::Value(outputs) => {
                format!("Observed outputs: {}", outputs.outputs.len())
            }
            other => format!("Outputs: {}", observation_label(other)),
        };

        Self {
            kernel,
            platform,
            version: summary.package_version.clone(),
            build,
            system,
            capabilities,
            services,
            gpu,
            telemetry,
            display,
            snapshot_meta: format!("capability generation {}", dto.capability_generation),
        }
    }
}

fn observation_label<T: Debug>(observation: &DiagnosticObservation<T>) -> String {
    match observation {
        DiagnosticObservation::Value(value) => format!("{value:?}"),
        DiagnosticObservation::Unavailable => "Unavailable".into(),
        DiagnosticObservation::PermissionDenied => "PermissionDenied".into(),
        DiagnosticObservation::Unknown => "Unknown".into(),
    }
}

fn nvidia_label(
    observation: &DiagnosticObservation<orbis_core::diagnostics::NvidiaGpuDiagnostics>,
) -> String {
    match observation {
        DiagnosticObservation::Value(value) => format!(
            "power={} temp={} limit={}",
            value
                .power
                .map_or_else(|| "Unknown".into(), |v| format!("{} mW", v.get())),
            value
                .temperature
                .map_or_else(|| "Unknown".into(), |v| format!("{} C", v.get())),
            value.power_limit.map_or_else(
                || "Unknown".into(),
                |v| {
                    format!(
                        "{} W (default {:?}, {}..{} W)",
                        v.value, v.default, v.min, v.max
                    )
                }
            ),
        ),
        other => observation_label(other),
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use orbis_core::capability::{CapabilityStatus, FeatureId};
    use orbis_core::diagnostics::{
        DiagnosticsServiceId, DisplayDiagnostics, DisplayProtocol, GpuDiagnostics,
        ServiceAvailability, ServiceBusScope, ServiceCriticality, SessionType,
        TelemetryCollectionStatus, TelemetryDiagnostics, TelemetryFreshness,
    };
    use orbis_core::gpu::{GpuMuxState, GpuPowerState};

    use super::*;
    use crate::diagnostics_dto::{
        CapabilityRowDto, DiagnosticsSummaryDto, DiagnosticsUiDto, ServiceRowDto,
    };

    fn dto() -> DiagnosticsUiDto {
        DiagnosticsUiDto {
            generated_at: SystemTime::UNIX_EPOCH,
            summary: DiagnosticsSummaryDto {
                package_version: "0.9.7".into(),
                build_revision: None,
                build_channel: Some("beta".into()),
                kernel_release: Some("6.12.9-orbis".into()),
                architecture: Some("x86_64".into()),
                session_type: SessionType::Wayland,
                display_protocol: DisplayProtocol::Wayland,
                compositor: None,
                hardware_vendor: Some("ASUSTeK COMPUTER INC.".into()),
                hardware_product: Some("GA403UI".into()),
                hardware_board: Some("GA403UI".into()),
                bios_version: Some("310".into()),
                bios_date: None,
            },
            capability_generation: 8,
            capability_checked_at: SystemTime::UNIX_EPOCH,
            capabilities: vec![CapabilityRowDto {
                feature: FeatureId::ChargeLimit,
                status: CapabilityStatus::Supported,
                read_status: CapabilityStatus::Supported,
                write_status: CapabilityStatus::PermissionDenied,
            }],
            services: vec![ServiceRowDto {
                service: DiagnosticsServiceId::OrbisHardwared,
                bus: ServiceBusScope::System,
                availability: ServiceAvailability::Unavailable,
                criticality: ServiceCriticality::CapabilityLocal,
                checked_at: Some(SystemTime::UNIX_EPOCH),
            }],
            gpu: GpuDiagnostics {
                mux: DiagnosticObservation::Value(GpuMuxState::Integrated),
                access_policy: DiagnosticObservation::PermissionDenied,
                runtime_power: DiagnosticObservation::Value(GpuPowerState::Unknown),
                nvidia: DiagnosticObservation::Unknown,
            },
            telemetry: TelemetryDiagnostics {
                latest: None,
                status: TelemetryCollectionStatus::Unavailable,
                last_attempt_at: Some(SystemTime::UNIX_EPOCH),
                last_success_at: None,
                freshness: TelemetryFreshness::Unknown,
            },
            display: DisplayDiagnostics {
                outputs: DiagnosticObservation::Unknown,
                checked_at: Some(SystemTime::UNIX_EPOCH),
            },
        }
    }

    #[test]
    fn production_values_replace_preview_literals() {
        let model = DiagnosticsWindowModel::from_dto(&dto());
        assert_eq!(model.kernel, "6.12.9-orbis");
        assert_eq!(model.platform, "ASUSTeK COMPUTER INC. · GA403UI");
        assert_eq!(model.version, "0.9.7");
        assert!(model.build.contains("beta"));
        assert!(!model.capabilities.contains("Preview"));
        assert!(!model.services.contains("Preview"));
    }

    #[test]
    fn capability_and_service_states_are_not_collapsed() {
        let model = DiagnosticsWindowModel::from_dto(&dto());
        assert!(model.capabilities.contains("Supported"));
        assert!(model.capabilities.contains("PermissionDenied"));
        assert!(model.services.contains("Unavailable"));
        assert!(model.gpu.contains("PermissionDenied"));
        assert!(model.gpu.contains("Unknown"));
    }

    #[test]
    fn nvidia_observation_explicitly_remains_read_only() {
        let mut value = dto();
        value.gpu.nvidia =
            DiagnosticObservation::Value(orbis_core::diagnostics::NvidiaGpuDiagnostics {
                power: Some(orbis_core::newtypes::MilliWatt::new(42_000).unwrap()),
                temperature: Some(orbis_core::newtypes::TemperatureC::new(61).unwrap()),
                power_limit: Some(
                    orbis_core::limits::PowerLimitValue::new(
                        80,
                        35,
                        115,
                        1,
                        Some(80),
                        orbis_core::limits::Unit::Watts,
                    )
                    .unwrap(),
                ),
            });
        let model = DiagnosticsWindowModel::from_dto(&value);
        assert!(model.gpu.contains("42000 mW"));
        assert!(model.gpu.contains("61 C"));
        assert!(model.gpu.contains("35..115 W"));
    }

    #[test]
    fn absent_identity_never_synthesizes_asus() {
        let mut value = dto();
        value.summary.hardware_vendor = None;
        value.summary.hardware_product = None;
        value.summary.kernel_release = None;
        let model = DiagnosticsWindowModel::from_dto(&value);
        assert_eq!(model.platform, "Unknown");
        assert_eq!(model.kernel, "Unknown");
        assert!(!model.platform.contains("ASUS"));
    }

    #[test]
    fn telemetry_without_sample_reports_explicit_gap_absence() {
        let model = DiagnosticsWindowModel::from_dto(&dto());
        assert!(model.telemetry.contains("gaps: none"));
    }

    #[test]
    fn telemetry_field_gaps_are_rendered_field_by_field() {
        let mut value = dto();
        let mut sample = orbis_core::telemetry::Telemetry::empty();
        sample.field_gaps = vec![
            (
                orbis_core::telemetry::TelemetryField::CpuTemp,
                orbis_core::telemetry::TelemetryFieldGap::Denied,
            ),
            (
                orbis_core::telemetry::TelemetryField::Battery,
                orbis_core::telemetry::TelemetryFieldGap::Malformed,
            ),
        ];
        value.telemetry.latest = Some(sample);
        let model = DiagnosticsWindowModel::from_dto(&value);
        assert!(model.telemetry.contains("CpuTemp=Denied"));
        assert!(model.telemetry.contains("Battery=Malformed"));
        assert!(!model.telemetry.contains("gaps: none"));
    }
}
