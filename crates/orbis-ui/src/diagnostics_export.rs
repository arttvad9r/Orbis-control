//! Privacy-bounded diagnostics export projections.
//!
//! Exporters consume only `DiagnosticsUiDto`: no journal access, arbitrary file
//! reads, environment dumps, shell commands, serial/UUID/asset-tag fields, or
//! provider calls are available at this layer.

use std::fmt::Debug;

use orbis_core::diagnostics::{CpuPackagePowerLimitsObservation, DiagnosticObservation};
use orbis_core::limits::PowerLimitField;
use serde_json::{Value, json};

use crate::diagnostics_dto::{DiagnosticsUiDto, telemetry_field_gap_labels};

const UNKNOWN: &str = "unknown";

fn optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or(UNKNOWN)
}

fn observation<T: Debug>(value: &DiagnosticObservation<T>) -> String {
    match value {
        DiagnosticObservation::Value(value) => format!("{value:?}"),
        DiagnosticObservation::Unavailable => "Unavailable".into(),
        DiagnosticObservation::PermissionDenied => "PermissionDenied".into(),
        DiagnosticObservation::Unknown => "Unknown".into(),
    }
}

fn power_limit_field(field: &PowerLimitField) -> String {
    match field {
        PowerLimitField::Spl => "spl",
        PowerLimitField::Sppt => "sppt",
        PowerLimitField::Fppt => "fppt",
        PowerLimitField::CpuTempLimit => "cpu_temp_limit",
        PowerLimitField::GpuDynamicBoost => "gpu_dynamic_boost",
        PowerLimitField::GpuTempTarget => "gpu_temp_target",
        PowerLimitField::Other(_) => "other",
    }
    .into()
}

/// Stable, human-readable allowlisted summary suitable for clipboard export.
pub fn summary_text(dto: &DiagnosticsUiDto) -> String {
    let s = &dto.summary;
    let mut out = String::new();

    out.push_str("[application]\n");
    out.push_str(&format!("version={}\n", s.package_version));
    out.push_str(&format!("build_revision={}\n", optional(&s.build_revision)));
    out.push_str(&format!("build_channel={}\n", optional(&s.build_channel)));

    out.push_str("\n[system]\n");
    out.push_str(&format!("kernel={}\n", optional(&s.kernel_release)));
    out.push_str(&format!("architecture={}\n", optional(&s.architecture)));
    out.push_str(&format!("session={:?}\n", s.session_type));
    out.push_str(&format!("display_protocol={:?}\n", s.display_protocol));
    out.push_str(&format!("compositor={}\n", optional(&s.compositor)));

    out.push_str("\n[hardware]\n");
    out.push_str(&format!("vendor={}\n", optional(&s.hardware_vendor)));
    out.push_str(&format!("product={}\n", optional(&s.hardware_product)));
    out.push_str(&format!("board={}\n", optional(&s.hardware_board)));
    out.push_str(&format!("bios_version={}\n", optional(&s.bios_version)));
    out.push_str(&format!("bios_date={}\n", optional(&s.bios_date)));

    out.push_str("\n[capabilities]\n");
    if dto.capabilities.is_empty() {
        out.push_str("none\n");
    } else {
        for row in &dto.capabilities {
            out.push_str(&format!(
                "{} overall={} read={} write={}\n",
                row.feature.as_str(),
                row.status.as_str(),
                row.read_status.as_str(),
                row.write_status.as_str()
            ));
        }
    }

    out.push_str("\n[services]\n");
    if dto.services.is_empty() {
        out.push_str("none\n");
    } else {
        for row in &dto.services {
            out.push_str(&format!(
                "{:?} bus={:?} availability={:?} criticality={:?}\n",
                row.service, row.bus, row.availability, row.criticality
            ));
        }
    }

    out.push_str("\n[gpu]\n");
    out.push_str(&format!("mux={}\n", observation(&dto.gpu.mux)));
    out.push_str(&format!(
        "access_policy={}\n",
        observation(&dto.gpu.access_policy)
    ));
    out.push_str(&format!(
        "runtime_power={}\n",
        observation(&dto.gpu.runtime_power)
    ));
    match &dto.gpu.nvidia {
        DiagnosticObservation::Value(value) => {
            out.push_str(&format!(
                "nvidia_power_mw={} temperature_c={} write=ReadOnly\n",
                value
                    .power
                    .map_or_else(|| UNKNOWN.into(), |power| power.get().to_string()),
                value.temperature.map_or_else(
                    || UNKNOWN.into(),
                    |temperature| temperature.get().to_string()
                ),
            ));
            if let Some(limit) = &value.power_limit {
                out.push_str(&format!(
                    "nvidia_power_limit_w={} default_w={} min_w={} max_w={} write=ReadOnly\n",
                    limit.value,
                    limit
                        .default
                        .map_or_else(|| UNKNOWN.into(), |value| value.to_string()),
                    limit.min,
                    limit.max,
                ));
            }
        }
        other => out.push_str(&format!("nvidia={} write=ReadOnly\n", observation(other))),
    }

    out.push_str("\n[cpu_package_power_limits]\n");
    match &dto.cpu_package_power_limits {
        CpuPackagePowerLimitsObservation::Value(limits) => {
            for (field, value) in &limits.fields {
                out.push_str(&format!(
                    "{} current_w={} min_w={} max_w={} default_w={}\n",
                    power_limit_field(field),
                    value.value,
                    value.min,
                    value.max,
                    value
                        .default
                        .map_or_else(|| UNKNOWN.into(), |v| v.to_string()),
                ));
            }
        }
        CpuPackagePowerLimitsObservation::Unavailable => out.push_str("status=Unavailable\n"),
        CpuPackagePowerLimitsObservation::Unsupported => out.push_str("status=Unsupported\n"),
        CpuPackagePowerLimitsObservation::PermissionDenied => {
            out.push_str("status=PermissionDenied\n")
        }
        CpuPackagePowerLimitsObservation::Malformed => out.push_str("status=Malformed\n"),
        CpuPackagePowerLimitsObservation::Unknown => out.push_str("status=Unknown\n"),
    }

    out.push_str("\n[telemetry]\n");
    out.push_str(&format!("status={:?}\n", dto.telemetry.status));
    out.push_str(&format!("quality={:?}\n", dto.telemetry.quality()));
    out.push_str(&format!("freshness={:?}\n", dto.telemetry.freshness));
    out.push_str(&format!(
        "sample_present={}\n",
        dto.telemetry.latest.is_some()
    ));
    let gap_labels = telemetry_field_gap_labels(dto);
    if gap_labels.is_empty() {
        out.push_str("field_gaps=none\n");
    } else {
        for (field, gap) in gap_labels {
            out.push_str(&format!("field_gap_{field}={gap}\n"));
        }
    }

    out.push_str("\n[display]\n");
    match &dto.display.outputs {
        DiagnosticObservation::Value(snapshot) => {
            out.push_str(&format!("outputs_count={}\n", snapshot.outputs.len()));
        }
        other => out.push_str(&format!("outputs={}\n", observation(other))),
    }

    out
}

/// Stable JSON allowlist projection. It intentionally omits raw telemetry,
/// timestamps, arbitrary paths, environment contents, logs, and identifiers
/// outside the privacy-reviewed DTO summary fields.
pub fn report_json_value(dto: &DiagnosticsUiDto) -> Value {
    let s = &dto.summary;
    let capabilities = dto
        .capabilities
        .iter()
        .map(|row| {
            json!({
                "id": row.feature.as_str(),
                "overall": row.status.as_str(),
                "read": row.read_status.as_str(),
                "write": row.write_status.as_str(),
            })
        })
        .collect::<Vec<_>>();
    let services = dto
        .services
        .iter()
        .map(|row| {
            json!({
                "service": format!("{:?}", row.service),
                "bus": format!("{:?}", row.bus),
                "availability": format!("{:?}", row.availability),
                "criticality": format!("{:?}", row.criticality),
            })
        })
        .collect::<Vec<_>>();
    let display = match &dto.display.outputs {
        DiagnosticObservation::Value(snapshot) => json!({
            "status": "Value",
            "outputs_count": snapshot.outputs.len(),
        }),
        other => json!({ "status": observation(other) }),
    };
    let nvidia = match &dto.gpu.nvidia {
        DiagnosticObservation::Value(value) => json!({
            "status": "Value",
            "power_mw": value.power.map(|power| power.get()),
            "temperature_c": value.temperature.map(|temperature| temperature.get()),
            "power_limit": value.power_limit.as_ref().map(|limit| json!({
                "current_w": limit.value,
                "default_w": limit.default,
                "min_w": limit.min,
                "max_w": limit.max,
                "write": "ReadOnly",
            })),
        }),
        other => json!({ "status": observation(other), "write": "ReadOnly" }),
    };
    let cpu_package_power_limits = match &dto.cpu_package_power_limits {
        CpuPackagePowerLimitsObservation::Value(limits) => json!({
            "status": "Value",
            "fields": limits.fields.iter().map(|(field, value)| json!({
                "field": power_limit_field(field),
                "current_w": value.value,
                "min_w": value.min,
                "max_w": value.max,
                "default_w": value.default,
            })).collect::<Vec<_>>(),
        }),
        CpuPackagePowerLimitsObservation::Unavailable => json!({ "status": "Unavailable" }),
        CpuPackagePowerLimitsObservation::Unsupported => json!({ "status": "Unsupported" }),
        CpuPackagePowerLimitsObservation::PermissionDenied => {
            json!({ "status": "PermissionDenied" })
        }
        CpuPackagePowerLimitsObservation::Malformed => json!({ "status": "Malformed" }),
        CpuPackagePowerLimitsObservation::Unknown => json!({ "status": "Unknown" }),
    };

    json!({
        "schema_version": 1,
        "application": {
            "version": s.package_version,
            "build_revision": s.build_revision,
            "build_channel": s.build_channel,
        },
        "system": {
            "kernel": s.kernel_release,
            "architecture": s.architecture,
            "session": format!("{:?}", s.session_type),
            "display_protocol": format!("{:?}", s.display_protocol),
            "compositor": s.compositor,
        },
        "hardware": {
            "vendor": s.hardware_vendor,
            "product": s.hardware_product,
            "board": s.hardware_board,
            "bios_version": s.bios_version,
            "bios_date": s.bios_date,
        },
        "capability_generation": dto.capability_generation,
        "capabilities": capabilities,
        "services": services,
        "gpu": {
            "mux": observation(&dto.gpu.mux),
            "access_policy": observation(&dto.gpu.access_policy),
            "runtime_power": observation(&dto.gpu.runtime_power),
            "nvidia": nvidia,
        },
        "cpu_package_power_limits": cpu_package_power_limits,
        "telemetry": {
            "status": format!("{:?}", dto.telemetry.status),
            "quality": format!("{:?}", dto.telemetry.quality()),
            "freshness": format!("{:?}", dto.telemetry.freshness),
            "sample_present": dto.telemetry.latest.is_some(),
            "field_gaps": telemetry_field_gap_labels(dto)
                .iter()
                .map(|(field, gap)| json!({ "field": field, "gap": gap }))
                .collect::<Vec<_>>(),
        },
        "display": display,
    })
}

/// Pretty JSON form for clipboard/file consumers.
pub fn report_json(dto: &DiagnosticsUiDto) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&report_json_value(dto))
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use orbis_core::capability::{CapabilityStatus, FeatureId};
    use orbis_core::diagnostics::{
        DiagnosticObservation, DisplayDiagnostics, DisplayProtocol, GpuDiagnostics, SessionType,
        TelemetryCollectionStatus, TelemetryDiagnostics, TelemetryFreshness,
    };

    use super::*;
    use crate::diagnostics_dto::{CapabilityRowDto, DiagnosticsSummaryDto};

    fn dto() -> DiagnosticsUiDto {
        DiagnosticsUiDto {
            generated_at: SystemTime::UNIX_EPOCH,
            summary: DiagnosticsSummaryDto {
                package_version: "0.1.0-test".into(),
                build_revision: Some("deadbeef".into()),
                build_channel: Some("beta".into()),
                kernel_release: Some("6.12-test".into()),
                architecture: Some("x86_64".into()),
                session_type: SessionType::Wayland,
                display_protocol: DisplayProtocol::Wayland,
                compositor: None,
                hardware_vendor: Some("ASUSTeK COMPUTER INC.".into()),
                hardware_product: Some("Example Model".into()),
                hardware_board: Some("EXAMPLE-BOARD".into()),
                bios_version: Some("1.2.3".into()),
                bios_date: None,
            },
            capability_generation: 17,
            capability_checked_at: SystemTime::UNIX_EPOCH,
            capabilities: vec![CapabilityRowDto {
                feature: FeatureId::Performance,
                status: CapabilityStatus::ReadOnly,
                read_status: CapabilityStatus::Supported,
                write_status: CapabilityStatus::ReadOnly,
            }],
            services: Vec::new(),
            gpu: GpuDiagnostics {
                mux: DiagnosticObservation::Unknown,
                access_policy: DiagnosticObservation::PermissionDenied,
                runtime_power: DiagnosticObservation::Unavailable,
                nvidia: DiagnosticObservation::Unknown,
            },
            cpu_package_power_limits: CpuPackagePowerLimitsObservation::Unknown,
            telemetry: TelemetryDiagnostics {
                latest: None,
                status: TelemetryCollectionStatus::Unavailable,
                last_attempt_at: None,
                last_success_at: None,
                freshness: TelemetryFreshness::Unknown,
            },
            display: DisplayDiagnostics {
                outputs: DiagnosticObservation::Unknown,
                checked_at: None,
            },
        }
    }

    #[test]
    fn text_export_has_stable_allowlisted_sections() {
        let text = summary_text(&dto());
        for section in [
            "[application]",
            "[system]",
            "[hardware]",
            "[capabilities]",
            "[services]",
            "[gpu]",
            "[telemetry]",
            "[display]",
        ] {
            assert!(text.contains(section));
        }
        assert!(text.contains("performance overall=read_only read=supported write=read_only"));
        for forbidden in ["serial", "uuid", "asset_tag", "journal", "environment"] {
            assert!(!text.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn json_export_is_versioned_and_omits_raw_collection_surfaces() {
        let value = report_json_value(&dto());
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["hardware"]["product"], "Example Model");
        assert_eq!(value["capabilities"][0]["read"], "supported");
        assert_eq!(value["capabilities"][0]["write"], "read_only");
        let rendered = report_json(&dto()).unwrap().to_ascii_lowercase();
        for forbidden in ["serial", "uuid", "asset_tag", "journal", "environment"] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[test]
    fn unknown_and_permission_states_survive_export() {
        let text = summary_text(&dto());
        assert!(text.contains("mux=Unknown"));
        assert!(text.contains("access_policy=PermissionDenied"));
        assert!(text.contains("runtime_power=Unavailable"));
        assert!(text.contains("freshness=Unknown"));
    }

    #[test]
    fn telemetry_gap_absence_and_evidence_are_exported() {
        // No sample → explicit absence of gap evidence, stable schema.
        let text = summary_text(&dto());
        assert!(text.contains("field_gaps=none"));
        let value = report_json_value(&dto());
        assert_eq!(value["telemetry"]["field_gaps"], serde_json::json!([]));

        // A sample with failed discovered sources exports each gap.
        let mut with_gaps = dto();
        let mut sample = orbis_core::telemetry::Telemetry::empty();
        sample.field_gaps = vec![(
            orbis_core::telemetry::TelemetryField::CpuTemp,
            orbis_core::telemetry::TelemetryFieldGap::Denied,
        )];
        with_gaps.telemetry.latest = Some(sample);
        let text = summary_text(&with_gaps);
        assert!(text.contains("field_gap_CpuTemp=Denied"));
        let value = report_json_value(&with_gaps);
        assert_eq!(value["telemetry"]["field_gaps"][0]["field"], "CpuTemp");
        assert_eq!(value["telemetry"]["field_gaps"][0]["gap"], "Denied");
    }
}
