//! Deterministic capability-registry decoration for the hardware-inert
//! Automation runtime.
//!
//! Existing capability probing remains owned by `composition`. This module does
//! not probe hardware and does not alter generation/freshness metadata. It takes
//! one already-built immutable snapshot, copies every existing entry through the
//! strict registry builder, then appends the canonical Automation shadow/read
//! capability. Duplicate Automation evidence is rejected rather than overwritten.

use orbis_capabilities::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot, RegistryError};
use orbis_core::capability::FeatureId;

use crate::automation_capability::add_automation_shadow_capability;

/// Append canonical read-only Automation capability evidence to one immutable
/// snapshot while preserving its generation, timestamp and every existing entry.
pub fn with_automation_shadow_capability(
    snapshot: &CapabilityRegistrySnapshot,
) -> Result<CapabilityRegistrySnapshot, RegistryError> {
    let mut builder = CapabilityRegistryBuilder::new(snapshot.generation(), snapshot.checked_at());
    for (feature, capability) in snapshot.iter() {
        builder.add(*feature, capability.clone())?;
    }
    add_automation_shadow_capability(&mut builder, snapshot.checked_at())?;
    builder.build()
}

/// Whether a snapshot carries the exact fail-closed Automation operation shape
/// expected before production execution is enabled.
pub fn has_read_only_automation_shadow(snapshot: &CapabilityRegistrySnapshot) -> bool {
    let Some(capability) = snapshot.capability(FeatureId::Automation) else {
        return false;
    };
    capability.status == orbis_core::capability::CapabilityStatus::ReadOnly
        && capability.operations.read.status == orbis_core::capability::CapabilityStatus::Supported
        && capability.operations.write.status
            == orbis_core::capability::CapabilityStatus::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    use orbis_core::capability::{
        Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus,
        OperationCapability,
    };
    use orbis_core::profile::PerformanceProfile;

    fn performance() -> Capability {
        Capability::new(CapabilityStatus::Supported)
            .with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(CapabilityStatus::Supported),
            })
            .with_constraints(CapabilityConstraints::PerformanceProfiles(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
            ]))
    }

    #[test]
    fn decoration_preserves_snapshot_identity_and_existing_entries() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        let mut builder = CapabilityRegistryBuilder::new(7, checked_at);
        builder.add(FeatureId::Performance, performance()).unwrap();
        let original = builder.build().unwrap();

        let decorated = with_automation_shadow_capability(&original).unwrap();
        assert_eq!(decorated.generation(), original.generation());
        assert_eq!(decorated.checked_at(), original.checked_at());
        assert_eq!(
            decorated.capability(FeatureId::Performance),
            original.capability(FeatureId::Performance)
        );
        assert_eq!(decorated.len(), original.len() + 1);
        assert!(has_read_only_automation_shadow(&decorated));
    }

    #[test]
    fn duplicate_automation_evidence_is_rejected_not_overwritten() {
        let checked_at = SystemTime::UNIX_EPOCH + Duration::from_secs(9);
        let mut builder = CapabilityRegistryBuilder::new(3, checked_at);
        add_automation_shadow_capability(&mut builder, checked_at).unwrap();
        let original = builder.build().unwrap();

        assert_eq!(
            with_automation_shadow_capability(&original),
            Err(RegistryError::DuplicateCapability {
                feature: FeatureId::Automation,
            })
        );
    }

    #[test]
    fn decorator_cannot_advertise_automation_write() {
        let source = include_str!("automation_registry.rs");
        assert!(!source.contains(&["CapabilityStatus::Supported", ", write"].concat()));
        assert!(
            !source.contains(
                &[
                    "FeatureId::Automation, Capability::new(CapabilityStatus::",
                    "Supported",
                ]
                .concat()
            )
        );
        assert!(source.contains("add_automation_shadow_capability"));
    }
}
