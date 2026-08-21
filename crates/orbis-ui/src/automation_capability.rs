//! Canonical capability metadata and promotion evidence for Automation.
//!
//! Lifecycle observation, policy planning and preflight are real read-side
//! functionality. Hardware execution is not production-enabled, so the
//! canonical capability remains `ReadOnly` with write `Unsupported`.

use std::time::SystemTime;

use orbis_capabilities::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot, RegistryError};
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityReason, CapabilityStatus,
    FeatureId, OperationCapability, RiskLevel,
};

/// Independent evidence required before a future Performance-only Automation
/// executor may even be considered for production promotion.
///
/// This type is diagnostic/policy input only. Supplying all `true` values does
/// not itself change any capability and cannot enable a mutation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AutomationPromotionEvidence {
    /// Lifecycle observation/revision state is owned by the same sequential
    /// worker that owns application mutations.
    pub lifecycle_worker_owned: bool,
    /// Serialization admission and capability-generation replacement share the
    /// same worker owner through the mutation/read-back critical section.
    pub serialization_worker_owned: bool,
    /// Unknown post-mutation outcomes are wired to the typed recovery barrier
    /// and block further unattended admission until reconciliation.
    pub recovery_wired: bool,
    /// The first Performance-only executor has passed its executable failure-path
    /// tests against the existing application owner contract.
    pub performance_executor_validated: bool,
    /// Locked workspace check/tests/clippy and Slint validation passed on the
    /// exact revision being considered for promotion.
    pub executable_validation_passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationPromotionBlock {
    LifecycleNotWorkerOwned,
    SerializationNotWorkerOwned,
    RecoveryNotWired,
    PerformanceExecutorNotValidated,
    ExecutableValidationMissing,
}

/// Pure promotion assessment. It deliberately returns blockers rather than a
/// capability object so this helper cannot accidentally advertise write support.
pub fn assess_automation_promotion(
    evidence: AutomationPromotionEvidence,
) -> Result<(), Vec<AutomationPromotionBlock>> {
    let mut blocks = Vec::new();
    if !evidence.lifecycle_worker_owned {
        blocks.push(AutomationPromotionBlock::LifecycleNotWorkerOwned);
    }
    if !evidence.serialization_worker_owned {
        blocks.push(AutomationPromotionBlock::SerializationNotWorkerOwned);
    }
    if !evidence.recovery_wired {
        blocks.push(AutomationPromotionBlock::RecoveryNotWired);
    }
    if !evidence.performance_executor_validated {
        blocks.push(AutomationPromotionBlock::PerformanceExecutorNotValidated);
    }
    if !evidence.executable_validation_passed {
        blocks.push(AutomationPromotionBlock::ExecutableValidationMissing);
    }

    if blocks.is_empty() {
        Ok(())
    } else {
        Err(blocks)
    }
}

/// Build capability evidence for the current hardware-inert Automation runtime.
pub fn automation_shadow_capability(checked_at: SystemTime) -> Capability {
    let read_only_reason = CapabilityReason {
        reason: "automation lifecycle observation and policy preflight are available; hardware execution is disabled"
            .into(),
        suggestion: "enable Automation writes only after worker ownership, recovery and executable validation are proven"
            .into(),
        backend: None,
        endpoint: None,
        requirement: None,
        risk: RiskLevel::Safe,
        checked_at: Some(checked_at),
    };
    let write_reason = CapabilityReason {
        reason: "Automation executor is not production-enabled".into(),
        suggestion: "keep unattended execution disabled until the promotion evidence is complete on the exact build revision"
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
/// Duplicate insertion is an error; this helper never overwrites evidence.
pub fn add_automation_shadow_capability(
    builder: &mut CapabilityRegistryBuilder,
    checked_at: SystemTime,
) -> Result<(), RegistryError> {
    builder.add(
        FeatureId::Automation,
        automation_shadow_capability(checked_at),
    )
}

/// Return an immutable snapshot that contains explicit Automation shadow
/// evidence while preserving every authoritative entry, generation and
/// `checked_at` value from the source snapshot.
///
/// This is an adapter for consumers that need to reason about the already-real
/// Automation read/shadow stack before the global production probe owns that
/// feature. It never upgrades write support. If the source snapshot already
/// contains `FeatureId::Automation`, that authoritative entry is preserved
/// verbatim instead of being overwritten.
pub fn augment_snapshot_with_automation_shadow(
    snapshot: &CapabilityRegistrySnapshot,
) -> Result<CapabilityRegistrySnapshot, RegistryError> {
    if snapshot.contains(FeatureId::Automation) {
        return Ok(snapshot.clone());
    }

    let mut builder = CapabilityRegistryBuilder::new(snapshot.generation(), snapshot.checked_at());
    for (feature, capability) in snapshot.iter() {
        builder.add(*feature, capability.clone())?;
    }
    add_automation_shadow_capability(&mut builder, snapshot.checked_at())?;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_read_only_capability() -> Capability {
        Capability::new(CapabilityStatus::ReadOnly)
            .with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(CapabilityStatus::Unsupported),
            })
            .with_constraints(CapabilityConstraints::None)
    }

    #[test]
    fn default_promotion_evidence_fails_every_gate() {
        assert_eq!(
            assess_automation_promotion(AutomationPromotionEvidence::default()),
            Err(vec![
                AutomationPromotionBlock::LifecycleNotWorkerOwned,
                AutomationPromotionBlock::SerializationNotWorkerOwned,
                AutomationPromotionBlock::RecoveryNotWired,
                AutomationPromotionBlock::PerformanceExecutorNotValidated,
                AutomationPromotionBlock::ExecutableValidationMissing,
            ])
        );
    }

    #[test]
    fn executable_validation_is_independent_and_mandatory() {
        let evidence = AutomationPromotionEvidence {
            lifecycle_worker_owned: true,
            serialization_worker_owned: true,
            recovery_wired: true,
            performance_executor_validated: true,
            executable_validation_passed: false,
        };
        assert_eq!(
            assess_automation_promotion(evidence),
            Err(vec![AutomationPromotionBlock::ExecutableValidationMissing])
        );
    }

    #[test]
    fn complete_evidence_only_passes_assessment_and_does_not_build_write_capability() {
        let evidence = AutomationPromotionEvidence {
            lifecycle_worker_owned: true,
            serialization_worker_owned: true,
            recovery_wired: true,
            performance_executor_validated: true,
            executable_validation_passed: true,
        };
        assert_eq!(assess_automation_promotion(evidence), Ok(()));

        let capability = automation_shadow_capability(SystemTime::UNIX_EPOCH);
        assert_eq!(capability.status, CapabilityStatus::ReadOnly);
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

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
            capability
                .reason
                .as_ref()
                .and_then(|reason| reason.checked_at),
            Some(checked_at)
        );
    }

    #[test]
    fn registry_accepts_shadow_capability_without_write_support() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(77);
        let mut builder = CapabilityRegistryBuilder::new(5, checked_at);
        add_automation_shadow_capability(&mut builder, checked_at).unwrap();
        let snapshot = builder.build().unwrap();
        let capability = snapshot.capability(FeatureId::Automation).unwrap();
        assert_eq!(capability.status, CapabilityStatus::ReadOnly);
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn augmentation_preserves_generation_time_and_existing_entries() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(88);
        let mut builder = CapabilityRegistryBuilder::new(17, checked_at);
        builder
            .add(FeatureId::DisplayOutput, supported_read_only_capability())
            .unwrap();
        let source = builder.build().unwrap();

        let augmented = augment_snapshot_with_automation_shadow(&source).unwrap();
        assert_eq!(augmented.generation(), 17);
        assert_eq!(augmented.checked_at(), checked_at);
        assert_eq!(augmented.len(), source.len() + 1);
        assert_eq!(
            augmented.capability(FeatureId::DisplayOutput),
            source.capability(FeatureId::DisplayOutput)
        );
        let automation = augmented.capability(FeatureId::Automation).unwrap();
        assert_eq!(automation.status, CapabilityStatus::ReadOnly);
        assert_eq!(
            automation.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn augmentation_never_overwrites_existing_automation_evidence() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(90);
        let mut builder = CapabilityRegistryBuilder::new(18, checked_at);
        let existing = supported_read_only_capability();
        builder
            .add(FeatureId::Automation, existing.clone())
            .unwrap();
        let source = builder.build().unwrap();

        let augmented = augment_snapshot_with_automation_shadow(&source).unwrap();
        assert_eq!(augmented, source);
        assert_eq!(augmented.capability(FeatureId::Automation), Some(&existing));
    }

    #[test]
    fn duplicate_entry_is_rejected_instead_of_overwritten() {
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
        let risky = [
            [
                "write: OperationCapability::new(CapabilityStatus::",
                "Supported)",
            ]
            .concat(),
            [
                "write: OperationCapability::with_reason(CapabilityStatus::",
                "Supported",
            ]
            .concat(),
        ];
        for token in risky {
            assert!(
                !source.contains(&token),
                "unexpected write promotion token: {token}"
            );
        }
    }
}
