//! Lossless diagnostics projection of immutable capability registry snapshots.

use orbis_core::diagnostics::CapabilitySnapshotDiagnostics;

use crate::registry::CapabilityRegistrySnapshot;

/// Project one immutable capability registry snapshot into diagnostics.
///
/// This adapter performs no probing and makes no support inference. Registry
/// generation, checked-at timestamp, and canonical `DeviceCapabilities` are
/// copied exactly from the supplied snapshot. Service presence/health is not an
/// input and therefore cannot alter capability status here.
pub fn capability_snapshot_diagnostics(
    snapshot: &CapabilityRegistrySnapshot,
) -> CapabilitySnapshotDiagnostics {
    CapabilitySnapshotDiagnostics {
        generation: snapshot.generation(),
        checked_at: snapshot.checked_at(),
        capabilities: snapshot.device_capabilities().clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use orbis_core::capability::{CapabilityStatus, FeatureId};

    use super::*;
    use crate::{CapabilityPart, CapabilityRegistryBuilder};

    #[test]
    fn preserves_registry_metadata_and_canonical_capabilities_exactly() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(4_242);
        let mut builder = CapabilityRegistryBuilder::new(77, checked_at);
        builder
            .add_part(CapabilityPart {
                feature: FeatureId::GpuPower,
                status: CapabilityStatus::BackendMissing,
                reason: Some("backend not present".into()),
            })
            .unwrap();
        let snapshot = builder.build().unwrap();

        let diagnostics = capability_snapshot_diagnostics(&snapshot);

        assert_eq!(diagnostics.generation, snapshot.generation());
        assert_eq!(diagnostics.checked_at, snapshot.checked_at());
        assert_eq!(diagnostics.capabilities, *snapshot.device_capabilities());
        assert_eq!(
            diagnostics
                .capabilities
                .features
                .get(&FeatureId::GpuPower)
                .unwrap()
                .status,
            CapabilityStatus::BackendMissing
        );
        assert_eq!(
            diagnostics
                .capabilities
                .features
                .get(&FeatureId::GpuPower)
                .unwrap()
                .reason
                .as_deref(),
            Some("backend not present")
        );
    }

    #[test]
    fn empty_registry_remains_empty_without_synthetic_entries() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(9);
        let snapshot = CapabilityRegistryBuilder::new(3, checked_at)
            .build()
            .unwrap();

        let diagnostics = capability_snapshot_diagnostics(&snapshot);

        assert_eq!(diagnostics.generation, 3);
        assert_eq!(diagnostics.checked_at, checked_at);
        assert!(diagnostics.capabilities.features.is_empty());
    }

    #[test]
    fn adapting_does_not_mutate_the_registry_snapshot() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(12);
        let mut builder = CapabilityRegistryBuilder::new(5, checked_at);
        builder
            .add_part(CapabilityPart {
                feature: FeatureId::Performance,
                status: CapabilityStatus::Unknown,
                reason: None,
            })
            .unwrap();
        let snapshot = builder.build().unwrap();
        let before = snapshot.clone();

        let first = capability_snapshot_diagnostics(&snapshot);
        let second = capability_snapshot_diagnostics(&snapshot);

        assert_eq!(snapshot, before);
        assert_eq!(first, second);
    }
}
