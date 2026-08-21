//! Pure diagnostics adapter for the existing typed telemetry collection path.

use std::time::{Duration, SystemTime};

use orbis_core::{
    diagnostics::{TelemetryCollectionStatus, TelemetryDiagnostics, TelemetryFreshness},
    telemetry::Telemetry,
};

use crate::ProviderError;

/// Fold one already-completed telemetry collection attempt into diagnostics.
///
/// This adapter performs no hardware I/O and never calls a telemetry provider.
/// A successful typed sample replaces the previous sample. A failed attempt
/// preserves the last successful sample, updates only collection status/time,
/// and marks retained data stale so a failed refresh cannot present old values
/// as fresh. The freshness threshold is collector-owned and supplied by the
/// caller; no polling interval or timeout is hard-coded here.
pub fn telemetry_diagnostics_after_attempt(
    previous: Option<&TelemetryDiagnostics>,
    result: Result<Telemetry, ProviderError>,
    attempted_at: SystemTime,
    freshness_threshold: Duration,
) -> TelemetryDiagnostics {
    match result {
        Ok(sample) => {
            let last_success_at = sample.ts;
            let freshness = if matches!(sample.quality(), orbis_core::TelemetryQuality::Empty) {
                // A successful transport response with no useful fields is not
                // a fresh telemetry observation.
                TelemetryFreshness::Unknown
            } else {
                freshness_at(last_success_at, attempted_at, freshness_threshold)
            };
            TelemetryDiagnostics {
                latest: Some(sample),
                status: TelemetryCollectionStatus::Available,
                last_attempt_at: Some(attempted_at),
                last_success_at: Some(last_success_at),
                freshness,
            }
        }
        Err(error) => {
            let latest = previous.and_then(|state| state.latest.clone());
            let last_success_at = previous.and_then(|state| state.last_success_at);
            let freshness = if latest.is_some() {
                TelemetryFreshness::Stale
            } else {
                TelemetryFreshness::Unknown
            };
            TelemetryDiagnostics {
                latest,
                status: status_from_error(&error),
                last_attempt_at: Some(attempted_at),
                last_success_at,
                freshness,
            }
        }
    }
}

fn freshness_at(
    sampled_at: SystemTime,
    observed_at: SystemTime,
    freshness_threshold: Duration,
) -> TelemetryFreshness {
    match observed_at.duration_since(sampled_at) {
        Ok(age) if age <= freshness_threshold => TelemetryFreshness::Fresh,
        Ok(_) => TelemetryFreshness::Stale,
        Err(_) => TelemetryFreshness::Unknown,
    }
}

fn status_from_error(error: &ProviderError) -> TelemetryCollectionStatus {
    match error {
        ProviderError::PermissionDenied(_) => TelemetryCollectionStatus::PermissionDenied,
        ProviderError::BackendUnavailable(_)
        | ProviderError::Unsupported(_)
        | ProviderError::Timeout(_) => TelemetryCollectionStatus::Unavailable,
        ProviderError::InvalidRequest(_)
        | ProviderError::Io(_)
        | ProviderError::Dbus(_)
        | ProviderError::Internal(_)
        | ProviderError::Conflict(_) => TelemetryCollectionStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use orbis_core::{
        gpu::GpuPowerState,
        newtypes::{EnergyMWh, MilliWatt, Percent, Rpm, TemperatureC},
        telemetry::{BatteryTelemetry, FanTelemetry, PowerTelemetry, TelemetryQuality},
    };

    use super::*;

    fn sample(ts: SystemTime) -> Telemetry {
        Telemetry {
            cpu_temp: None,
            gpu_temp: None,
            fans: Vec::new(),
            power: PowerTelemetry::default(),
            ac_online: None,
            battery: None,
            gpu_power_state: GpuPowerState::Unknown,
            ts,
        }
    }

    fn complete_sample(ts: SystemTime) -> Telemetry {
        Telemetry {
            cpu_temp: Some(TemperatureC::new(50).unwrap()),
            gpu_temp: Some(TemperatureC::new(55).unwrap()),
            fans: vec![FanTelemetry {
                source: "test".into(),
                fan: orbis_core::fan::FanId::Cpu,
                label: "cpu_fan".into(),
                rpm: Rpm::new(2000).unwrap(),
                percent: Some(Percent::new(40).unwrap()),
                quality: orbis_core::telemetry::FanTelemetryQuality::Complete,
            }],
            power: PowerTelemetry {
                ac: Some(MilliWatt::new(50_000).unwrap()),
                battery: Some(MilliWatt::new(20_000).unwrap()),
                total: Some(MilliWatt::new(70_000).unwrap()),
                gpu: Some(MilliWatt::new(10_000).unwrap()),
            },
            ac_online: Some(true),
            battery: Some(BatteryTelemetry {
                percent: Percent::new(80).unwrap(),
                capacity: Some(Percent::new(95).unwrap()),
                energy_now: Some(EnergyMWh::new(40_000).unwrap()),
                energy_full: Some(EnergyMWh::new(50_000).unwrap()),
                charge_cycles: Some(10),
                state: "Charging".into(),
            }),
            gpu_power_state: GpuPowerState::Active,
            ts,
        }
    }

    #[test]
    fn successful_sample_is_preserved_exactly_and_marked_fresh_inside_threshold() {
        let sampled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let attempted_at = sampled_at + Duration::from_secs(2);
        let expected = sample(sampled_at);

        let diagnostics = telemetry_diagnostics_after_attempt(
            None,
            Ok(expected.clone()),
            attempted_at,
            Duration::from_secs(5),
        );

        assert_eq!(diagnostics.latest, Some(expected));
        assert_eq!(diagnostics.status, TelemetryCollectionStatus::Available);
        assert_eq!(diagnostics.last_attempt_at, Some(attempted_at));
        assert_eq!(diagnostics.last_success_at, Some(sampled_at));
        assert_eq!(diagnostics.freshness, TelemetryFreshness::Unknown);
        assert_eq!(diagnostics.quality(), TelemetryQuality::Empty);
    }

    #[test]
    fn complete_sample_is_fresh_and_complete() {
        let sampled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let diagnostics = telemetry_diagnostics_after_attempt(
            None,
            Ok(complete_sample(sampled_at)),
            sampled_at,
            Duration::from_secs(5),
        );

        assert_eq!(diagnostics.freshness, TelemetryFreshness::Fresh);
        assert_eq!(diagnostics.quality(), TelemetryQuality::Complete);
    }

    #[test]
    fn successful_sample_becomes_stale_only_by_caller_owned_threshold() {
        let sampled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let attempted_at = sampled_at + Duration::from_secs(6);

        let diagnostics = telemetry_diagnostics_after_attempt(
            None,
            Ok(complete_sample(sampled_at)),
            attempted_at,
            Duration::from_secs(5),
        );

        assert_eq!(diagnostics.status, TelemetryCollectionStatus::Available);
        assert_eq!(diagnostics.freshness, TelemetryFreshness::Stale);
        assert_eq!(diagnostics.quality(), TelemetryQuality::Stale);
    }

    #[test]
    fn failed_refresh_preserves_last_good_sample_and_marks_it_stale() {
        let sampled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let first_attempt = sampled_at + Duration::from_secs(1);
        let last_good = telemetry_diagnostics_after_attempt(
            None,
            Ok(sample(sampled_at)),
            first_attempt,
            Duration::from_secs(5),
        );
        let failed_at = sampled_at + Duration::from_secs(2);

        let diagnostics = telemetry_diagnostics_after_attempt(
            Some(&last_good),
            Err(ProviderError::BackendUnavailable(
                "telemetry unavailable".into(),
            )),
            failed_at,
            Duration::from_secs(5),
        );

        assert_eq!(diagnostics.latest, last_good.latest);
        assert_eq!(diagnostics.last_success_at, Some(sampled_at));
        assert_eq!(diagnostics.last_attempt_at, Some(failed_at));
        assert_eq!(diagnostics.status, TelemetryCollectionStatus::Unavailable);
        assert_eq!(diagnostics.freshness, TelemetryFreshness::Stale);
        assert_eq!(diagnostics.quality(), TelemetryQuality::Stale);
    }

    #[test]
    fn initial_failure_does_not_create_synthetic_sample() {
        let attempted_at = SystemTime::UNIX_EPOCH + Duration::from_secs(50);

        let diagnostics = telemetry_diagnostics_after_attempt(
            None,
            Err(ProviderError::PermissionDenied("denied".into())),
            attempted_at,
            Duration::from_secs(5),
        );

        assert!(diagnostics.latest.is_none());
        assert!(diagnostics.last_success_at.is_none());
        assert_eq!(diagnostics.last_attempt_at, Some(attempted_at));
        assert_eq!(
            diagnostics.status,
            TelemetryCollectionStatus::PermissionDenied
        );
        assert_eq!(diagnostics.freshness, TelemetryFreshness::Unknown);
        assert_eq!(diagnostics.quality(), TelemetryQuality::Failed);
    }

    #[test]
    fn partial_success_is_not_downgraded_or_filled_with_fake_values() {
        let sampled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let mut partial = sample(sampled_at);
        partial.cpu_temp = Some(TemperatureC::new(46).unwrap());

        let diagnostics = telemetry_diagnostics_after_attempt(
            None,
            Ok(partial.clone()),
            sampled_at,
            Duration::ZERO,
        );

        assert_eq!(diagnostics.status, TelemetryCollectionStatus::Available);
        assert_eq!(diagnostics.latest, Some(partial));
        assert_eq!(diagnostics.quality(), TelemetryQuality::Partial);
        let latest = diagnostics.latest.as_ref().unwrap();
        assert!(latest.cpu_temp.is_some());
        assert!(latest.gpu_temp.is_none());
        assert!(latest.fans.is_empty());
        assert!(latest.power.ac.is_none());
        assert!(latest.power.battery.is_none());
        assert!(latest.power.total.is_none());
        assert!(latest.power.gpu.is_none());
        assert!(latest.battery.is_none());
    }

    #[test]
    fn clock_regression_keeps_freshness_unknown() {
        let sampled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let attempted_at = sampled_at - Duration::from_secs(1);

        let diagnostics = telemetry_diagnostics_after_attempt(
            None,
            Ok(sample(sampled_at)),
            attempted_at,
            Duration::from_secs(5),
        );

        assert_eq!(diagnostics.freshness, TelemetryFreshness::Unknown);
    }

    #[test]
    fn provider_failures_map_without_fake_telemetry_status() {
        let cases = [
            (
                ProviderError::BackendUnavailable("offline".into()),
                TelemetryCollectionStatus::Unavailable,
            ),
            (
                ProviderError::Unsupported("unsupported".into()),
                TelemetryCollectionStatus::Unavailable,
            ),
            (
                ProviderError::Timeout("timeout".into()),
                TelemetryCollectionStatus::Unavailable,
            ),
            (
                ProviderError::PermissionDenied("denied".into()),
                TelemetryCollectionStatus::PermissionDenied,
            ),
            (
                ProviderError::Dbus("transport".into()),
                TelemetryCollectionStatus::Unknown,
            ),
            (
                ProviderError::Io(std::io::Error::other("io")),
                TelemetryCollectionStatus::Unknown,
            ),
            (
                ProviderError::Internal("internal".into()),
                TelemetryCollectionStatus::Unknown,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(status_from_error(&error), expected);
        }
    }
}
