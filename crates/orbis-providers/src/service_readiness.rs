//! Adapter from existing service-presence diagnostics into structured
//! readiness evidence.
//!
//! This module does not perform D-Bus I/O. It prevents creation of a second
//! service-discovery source of truth by translating already-observed
//! `ServiceDiagnostics` into the readiness model.

use orbis_core::FeatureId;
use orbis_core::diagnostics::{ServiceAvailability, ServiceDiagnostics};
use orbis_core::readiness::{PermissionState, ReadinessItem, ReadinessState};

/// Convert one already-observed service diagnostic into readiness evidence.
///
/// `Activatable` is classified as `Inactive`: a stopped service must not be
/// activated by diagnostics. `PermissionDenied` is kept separate from service
/// reachability through `PermissionState::Denied`.
pub fn readiness_from_service_diagnostics(
    diagnostics: &ServiceDiagnostics,
    id: impl Into<String>,
    required_for: Vec<FeatureId>,
) -> ReadinessItem {
    let (state, permission) = match diagnostics.availability {
        ServiceAvailability::Running => (ReadinessState::Ready, PermissionState::NotRequired),
        ServiceAvailability::Activatable => {
            (ReadinessState::Inactive, PermissionState::NotRequired)
        }
        ServiceAvailability::Unavailable => {
            (ReadinessState::NotAvailable, PermissionState::NotRequired)
        }
        ServiceAvailability::PermissionDenied => (ReadinessState::Unknown, PermissionState::Denied),
        ServiceAvailability::Unknown => (ReadinessState::Unknown, PermissionState::Unknown),
    };

    ReadinessItem {
        id: id.into(),
        state,
        required_for,
        evidence: vec![format!(
            "service={:?} bus={:?} availability={:?}",
            diagnostics.service, diagnostics.bus, diagnostics.availability
        )],
        permission,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::diagnostics::{DiagnosticsServiceId, ServiceBusScope, ServiceCriticality};

    fn diagnostics(availability: ServiceAvailability) -> ServiceDiagnostics {
        ServiceDiagnostics {
            service: DiagnosticsServiceId::Asusd,
            bus: ServiceBusScope::System,
            availability,
            criticality: ServiceCriticality::CapabilityLocal,
            checked_at: None,
        }
    }

    #[test]
    fn activatable_service_is_inactive_and_never_auto_started() {
        let item = readiness_from_service_diagnostics(
            &diagnostics(ServiceAvailability::Activatable),
            "asusd",
            vec![FeatureId::FanCurves],
        );
        assert_eq!(item.state, ReadinessState::Inactive);
        assert_eq!(item.permission, PermissionState::NotRequired);
    }

    #[test]
    fn permission_denial_remains_separate_evidence() {
        let item = readiness_from_service_diagnostics(
            &diagnostics(ServiceAvailability::PermissionDenied),
            "asusd",
            vec![FeatureId::FanCurves],
        );
        assert_eq!(item.state, ReadinessState::Unknown);
        assert_eq!(item.permission, PermissionState::Denied);
        assert!(item.blocks(FeatureId::FanCurves));
    }

    #[test]
    fn running_service_is_ready() {
        let item = readiness_from_service_diagnostics(
            &diagnostics(ServiceAvailability::Running),
            "asusd",
            vec![FeatureId::FanCurves],
        );
        assert_eq!(item.state, ReadinessState::Ready);
        assert!(!item.blocks(FeatureId::FanCurves));
    }
}
