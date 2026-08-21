//! Hardware support validation workflow.
//!
//! The workflow encodes NBFC-style read-only-first onboarding. It does not
//! perform probes or writes; it only states which evidence must exist before a
//! future controlled write validation can even become eligible.

use serde::{Deserialize, Serialize};

/// Stage of a hardware validation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareValidationStage {
    /// No trustworthy read contract has been established.
    Discovery,
    /// Read-only contract is proven; dynamic correlation still required.
    ReadOnlyValidated,
    /// Sensors/state react consistently to controlled system changes/load.
    Correlated,
    /// A typed mutation backend is proven and controlled write validation may
    /// be scheduled explicitly.
    WriteEligible,
    /// Controlled write + authoritative restoration were validated.
    WriteValidated,
    /// Evidence became inconsistent or a validation step failed.
    Failed,
}

/// Evidence accumulated during hardware onboarding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HardwareValidationEvidence {
    /// Backend identity/path/service is discovered.
    pub backend_discovered: bool,
    /// Repeated authoritative reads are structurally valid.
    pub read_contract_valid: bool,
    /// Observed sensor/state reacts to a controlled external change/load.
    pub dynamic_correlation_valid: bool,
    /// Distinct devices/fans were identified independently where required.
    pub independent_channels_valid: bool,
    /// Typed mutation ownership/authorization/semantics are proven.
    pub mutation_backend_proven: bool,
    /// A controlled mutation was authoritatively read back.
    pub mutation_readback_valid: bool,
    /// Previous state was restored and authoritatively verified.
    pub restoration_valid: bool,
    /// Any hard validation failure.
    pub failed: bool,
}

impl HardwareValidationEvidence {
    fn correlated_prerequisites(&self) -> bool {
        self.backend_discovered
            && self.read_contract_valid
            && self.dynamic_correlation_valid
            && self.independent_channels_valid
    }

    fn write_prerequisites(&self) -> bool {
        self.correlated_prerequisites() && self.mutation_backend_proven
    }

    /// Derive the strongest justified stage from available evidence.
    pub fn stage(&self) -> HardwareValidationStage {
        if self.failed {
            return HardwareValidationStage::Failed;
        }
        if self.write_prerequisites() && self.mutation_readback_valid && self.restoration_valid {
            return HardwareValidationStage::WriteValidated;
        }
        if self.write_prerequisites() {
            return HardwareValidationStage::WriteEligible;
        }
        if self.correlated_prerequisites() {
            return HardwareValidationStage::Correlated;
        }
        if self.backend_discovered && self.read_contract_valid {
            return HardwareValidationStage::ReadOnlyValidated;
        }
        HardwareValidationStage::Discovery
    }

    /// Whether a future controlled write validation may be attempted.
    ///
    /// This is only an eligibility predicate. It does not authorize or execute
    /// a hardware write.
    pub fn write_validation_eligible(&self) -> bool {
        matches!(
            self.stage(),
            HardwareValidationStage::WriteEligible | HardwareValidationStage::WriteValidated
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_never_jumps_from_discovery_to_write_eligible() {
        let evidence = HardwareValidationEvidence {
            backend_discovered: true,
            mutation_backend_proven: true,
            ..Default::default()
        };
        assert_eq!(evidence.stage(), HardwareValidationStage::Discovery);
        assert!(!evidence.write_validation_eligible());
    }

    #[test]
    fn read_only_validation_requires_discovery_and_valid_reads() {
        let evidence = HardwareValidationEvidence {
            backend_discovered: true,
            read_contract_valid: true,
            ..Default::default()
        };
        assert_eq!(
            evidence.stage(),
            HardwareValidationStage::ReadOnlyValidated
        );
    }

    #[test]
    fn write_eligibility_requires_correlation_and_independent_channels() {
        let evidence = HardwareValidationEvidence {
            backend_discovered: true,
            read_contract_valid: true,
            dynamic_correlation_valid: true,
            independent_channels_valid: true,
            mutation_backend_proven: true,
            ..Default::default()
        };
        assert_eq!(evidence.stage(), HardwareValidationStage::WriteEligible);
        assert!(evidence.write_validation_eligible());
    }

    #[test]
    fn write_validated_requires_all_prior_evidence_readback_and_restoration() {
        let incomplete = HardwareValidationEvidence {
            mutation_readback_valid: true,
            restoration_valid: true,
            ..Default::default()
        };
        assert_eq!(incomplete.stage(), HardwareValidationStage::Discovery);

        let complete = HardwareValidationEvidence {
            backend_discovered: true,
            read_contract_valid: true,
            dynamic_correlation_valid: true,
            independent_channels_valid: true,
            mutation_backend_proven: true,
            mutation_readback_valid: true,
            restoration_valid: true,
            failed: false,
        };
        assert_eq!(complete.stage(), HardwareValidationStage::WriteValidated);
    }

    #[test]
    fn hard_failure_overrides_positive_evidence() {
        let evidence = HardwareValidationEvidence {
            backend_discovered: true,
            read_contract_valid: true,
            dynamic_correlation_valid: true,
            independent_channels_valid: true,
            mutation_backend_proven: true,
            mutation_readback_valid: true,
            restoration_valid: true,
            failed: true,
        };
        assert_eq!(evidence.stage(), HardwareValidationStage::Failed);
        assert!(!evidence.write_validation_eligible());
    }
}
