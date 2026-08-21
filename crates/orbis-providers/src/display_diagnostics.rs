//! Read-only diagnostics adapter for existing display output providers.

use std::time::SystemTime;

use orbis_core::diagnostics::{DiagnosticObservation, DisplayDiagnostics};
use orbis_core::display_output::DisplayOutputSnapshot;

use crate::{DisplayOutputProvider, ProviderError, bounded_provider_call};

/// Read one authoritative display-output snapshot and classify the observation.
///
/// This adapter uses the existing read-only `DisplayOutputProvider`; it does not
/// configure outputs, perform modesets, or infer capability/writability from a
/// successful observation. `checked_at` remains caller-owned.
pub async fn display_diagnostics_snapshot<P>(
    provider: &P,
    checked_at: Option<SystemTime>,
) -> DisplayDiagnostics
where
    P: DisplayOutputProvider + ?Sized,
{
    DisplayDiagnostics {
        outputs: observation_from_result(
            bounded_provider_call(
                provider,
                "display.output_snapshot",
                provider.display_output_snapshot(),
            )
            .await,
        ),
        checked_at,
    }
}

fn observation_from_result(
    result: Result<DisplayOutputSnapshot, ProviderError>,
) -> DiagnosticObservation<DisplayOutputSnapshot> {
    match result {
        Ok(snapshot) => DiagnosticObservation::Value(snapshot),
        Err(ProviderError::PermissionDenied(_)) => DiagnosticObservation::PermissionDenied,
        Err(
            ProviderError::BackendUnavailable(_)
            | ProviderError::Unsupported(_)
            | ProviderError::Timeout(_),
        ) => DiagnosticObservation::Unavailable,
        Err(
            ProviderError::InvalidRequest(_)
            | ProviderError::Io(_)
            | ProviderError::Dbus(_)
            | ProviderError::Internal(_)
            | ProviderError::Conflict(_),
        ) => DiagnosticObservation::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn successful_empty_snapshot_is_still_a_successful_observation() {
        let snapshot = DisplayOutputSnapshot {
            outputs: Vec::new(),
        };
        assert_eq!(
            observation_from_result(Ok(snapshot.clone())),
            DiagnosticObservation::Value(snapshot)
        );
    }

    #[test]
    fn permission_denied_is_preserved() {
        assert_eq!(
            observation_from_result(Err(ProviderError::PermissionDenied("denied".into()))),
            DiagnosticObservation::PermissionDenied
        );
    }

    #[test]
    fn unavailable_provider_failures_are_not_fake_output_snapshots() {
        for error in [
            ProviderError::BackendUnavailable("no compositor".into()),
            ProviderError::Unsupported("not supported".into()),
            ProviderError::Timeout("timeout".into()),
        ] {
            assert_eq!(
                observation_from_result(Err(error)),
                DiagnosticObservation::Unavailable
            );
        }
    }

    #[test]
    fn transport_or_internal_failures_remain_unknown() {
        let errors = [
            ProviderError::InvalidRequest("invalid".into()),
            ProviderError::Io(std::io::Error::other("io")),
            ProviderError::Dbus("dbus".into()),
            ProviderError::Internal("internal".into()),
        ];
        for error in errors {
            assert_eq!(
                observation_from_result(Err(error)),
                DiagnosticObservation::Unknown
            );
        }
    }

    #[test]
    fn caller_owned_timestamp_can_remain_absent() {
        let diagnostics = DisplayDiagnostics {
            outputs: observation_from_result(Ok(DisplayOutputSnapshot {
                outputs: Vec::new(),
            })),
            checked_at: None,
        };
        assert!(diagnostics.checked_at.is_none());
    }

    #[test]
    fn caller_owned_timestamp_is_preserved_exactly() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        let diagnostics = DisplayDiagnostics {
            outputs: observation_from_result(Ok(DisplayOutputSnapshot {
                outputs: Vec::new(),
            })),
            checked_at: Some(checked_at),
        };
        assert_eq!(diagnostics.checked_at, Some(checked_at));
    }
}
