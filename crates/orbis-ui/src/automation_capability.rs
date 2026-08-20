//! Canonical capability metadata for the Automation shadow runtime.
//!
//! The lifecycle observer, policy planner and preflight stack are real read-side
//! functionality. Hardware execution is not production-enabled. Represent that
//! distinction explicitly as `ReadOnly` with read support and unsupported write
//! instead of leaving `FeatureId::Automation` semantically `Unknown`.

use std::time::SystemTime;

use orbis_capabilities::{CapabilityRegistryBuilder, RegistryError};
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityReason, CapabilityStatus,
    FeatureId, OperationCapability, RiskLevel,
};

/// Build capability evidence for the current hardware-inert Automation runtime.
///
/// This function performs no probing or I/O. `checked_at` must come from the
/// same registry-assembly instant as the surrounding immutable snapshot.
/// Production registry assembly may include this entry only while execution is
/// disabled. Promoting write to `Supported` requires an independently validated
/// serialized executor and authoritative read-back path.
pub fn automation_shadow_capability(checked_at: SystemTime) -> Capability {
    let read_only_reason = CapabilityReason {
        reason: "automation lifecycle observation and policy preflight are available; hardware execution is disabled"
            .into(),
        suggestion: "enable Automation writes only after the serialized executor and read-back path pass executable validation"
            .into(),
        backend: None,
        endpoint: None,
        requirement: None,
        risk: RiskLevel::Safe,
        checked_at: Some(checked_at),
    };
    let write_reason = CapabilityReason {
        reason: "Automation executor is not production-enabled".into(),
        suggestion: "keep unattended execution disabled until worker-owned serialization and read-back are validated"
            .into(),
        backend: None,
        endpoint: None,
        requirement: None,
        risk: RiskLevel::Safe,
        checked_at: Some(checked_at),
    };

    Capability::with_reason(CapabilityStatus::ReadOnly, read_only_reason)
        .with_operations(CapabilityOperations {
            read: OperationCapability::new(CapabilityStatus::Supported),
            write: OperationCapability::with_reason(CapabilityStatus::Unsupported, write_reason),
        })
        .with_constraints(CapabilityConstraints::None)
}

/// Add the canonical hardware-inert Automation entry to one registry builder.
///
/// Callers must pass the same `checked_at` used to construct the surrounding
/// registry generation. Duplicate insertion remains a `RegistryError`; this
/// helper never overwrites an independently assembled Automation capability.
pub fn add_automation_shadow_capability(
    builder: &mut CapabilityRegistryBuilder,
    checked_at: SystemTime,
) -> Result<(), RegistryError> {
    builder.add(
        FeatureId::Automation,
        automation_shadow_capability(checked_at),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_capability_is_explicitly_read_only() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(42);
        let capability = automation_shadow_capability(checked_at);

        assert_eq!(capability.status, CapabilityStatus::ReadOnly);
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(capability.constraints, CapabilityConstraints::None);
        assert_eq!(
            capability.reason.as_ref().and_then(|reason| reason.checked_at),
            Some(checked_at)
        );
        assert_eq!(
            capability
                .operations
                .write
                .reason
                .as_ref()
                .and_then(|reason| reason.checked_at),
            Some(checked_at)
        );
    }

    #[test]
    fn canonical_registry_accepts_shadow_capability_without_write_support() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(77);
        let mut builder = CapabilityRegistryBuilder::new(5, checked_at);
        add_automation_shadow_capability(&mut builder, checked_at)
            .expect("read-only Automation capability must satisfy registry invariants");
        let snapshot = builder.build().expect("snapshot");
        let capability = snapshot.capability(FeatureId::Automation).unwrap();

        assert_eq!(capability.status, CapabilityStatus::ReadOnly);
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn duplicate_automation_entry_is_rejected_instead_of_overwritten() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(91);
        let mut builder = CapabilityRegistryBuilder::new(6, checked_at);
        add_automation_shadow_capability(&mut builder, checked_at).unwrap();
        assert_eq!(
            add_automation_shadow_capability(&mut builder, checked_at),
            Err(RegistryError::DuplicateCapability {
                feature: FeatureId::Automation,
            })
        );
    }

    #[test]
    fn source_cannot_accidentally_advertise_supported_write() {
        let source = include_str!("automation_capability.rs");
        assert!(source.contains("CapabilityStatus::ReadOnly"));
        assert!(source.contains("CapabilityStatus::Unsupported"));
        assert!(!source.contains("write: OperationCapability::new(CapabilityStatus::Supported)"));
        assert!(!source.contains("write: OperationCapability::with_reason(CapabilityStatus::Supported"));
    }
}
