//! Pure product-promotion assessment for currently gated Hardware1 controls.
//!
//! Typed implementations are not sufficient evidence for release enablement.
//! This module keeps implementation existence, authoritative read-back,
//! authorization, non-mutating preflight, executable validation and explicit
//! product-policy approval as independent facts. It performs no I/O and cannot
//! enable a Hardware1 backend by itself.

/// Product mutation whose production composition is currently gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductMutation {
    PanelOverdrive,
    KeyboardBacklight,
    AuraStaticRgb,
}

/// Intended execution class. Unattended execution has stronger evidence
/// requirements than an explicitly user-requested interactive action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionScope {
    Interactive,
    Unattended,
}

/// Independent evidence collected for one mutation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProductMutationEvidence {
    /// A closed typed backend exists; no generic path/value writer is exposed.
    pub typed_backend: bool,
    /// A dedicated authorization boundary (for example a capability-specific
    /// polkit action) exists.
    pub authorization_boundary: bool,
    /// Startup/capability probing can prove structural readiness without
    /// performing a mutation or activating a competing owner.
    pub non_mutating_preflight: bool,
    /// Post-mutation hardware state can be read authoritatively and compared to
    /// the exact requested target.
    pub authoritative_hardware_readback: bool,
    /// A weaker configuration-level read-back exists. This may be sufficient
    /// for an explicitly interactive `Accepted` result but never substitutes for
    /// hardware confirmation in unattended execution.
    pub configuration_readback: bool,
    /// The exact production build passed executable Rust tests/check/clippy and
    /// relevant UI/transport validation.
    pub executable_validation: bool,
    /// Product/release policy explicitly approved enabling this capability.
    pub product_policy_approved: bool,
}

/// Why a mutation remains release-gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductMutationPromotionBlocker {
    TypedBackendMissing,
    AuthorizationBoundaryMissing,
    NonMutatingPreflightMissing,
    ReadBackMissing,
    HardwareReadBackRequiredForUnattended,
    ExecutableValidationMissing,
    ProductPolicyNotApproved,
}

/// Complete deterministic assessment. Empty blockers means the supplied
/// evidence is sufficient for the requested scope; it does not mutate runtime
/// composition or capability registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductMutationPromotionAssessment {
    pub mutation: ProductMutation,
    pub scope: PromotionScope,
    pub blockers: Vec<ProductMutationPromotionBlocker>,
}

impl ProductMutationPromotionAssessment {
    pub fn promotable(&self) -> bool {
        self.blockers.is_empty()
    }
}

pub fn assess_product_mutation_promotion(
    mutation: ProductMutation,
    scope: PromotionScope,
    evidence: ProductMutationEvidence,
) -> ProductMutationPromotionAssessment {
    let mut blockers = Vec::new();

    if !evidence.typed_backend {
        blockers.push(ProductMutationPromotionBlocker::TypedBackendMissing);
    }
    if !evidence.authorization_boundary {
        blockers.push(ProductMutationPromotionBlocker::AuthorizationBoundaryMissing);
    }
    if !evidence.non_mutating_preflight {
        blockers.push(ProductMutationPromotionBlocker::NonMutatingPreflightMissing);
    }

    match scope {
        PromotionScope::Interactive => {
            if !evidence.authoritative_hardware_readback && !evidence.configuration_readback {
                blockers.push(ProductMutationPromotionBlocker::ReadBackMissing);
            }
        }
        PromotionScope::Unattended => {
            if !evidence.authoritative_hardware_readback {
                blockers
                    .push(ProductMutationPromotionBlocker::HardwareReadBackRequiredForUnattended);
            }
        }
    }

    if !evidence.executable_validation {
        blockers.push(ProductMutationPromotionBlocker::ExecutableValidationMissing);
    }
    if !evidence.product_policy_approved {
        blockers.push(ProductMutationPromotionBlocker::ProductPolicyNotApproved);
    }

    ProductMutationPromotionAssessment {
        mutation,
        scope,
        blockers,
    }
}

/// Evidence already present in source for the three gated Hardware1 paths.
///
/// Panel and Aura now have explicit non-activating/read-only startup preflight
/// in `orbis-hardwared::product_preflight`; Keyboard has its structural LED ABI
/// probe. Executable validation and product approval deliberately remain false,
/// so none of these helpers can promote production mutation on this revision.
pub fn current_source_evidence(mutation: ProductMutation) -> ProductMutationEvidence {
    match mutation {
        ProductMutation::PanelOverdrive => ProductMutationEvidence {
            typed_backend: true,
            authorization_boundary: true,
            non_mutating_preflight: true,
            authoritative_hardware_readback: true,
            configuration_readback: true,
            executable_validation: false,
            product_policy_approved: false,
        },
        ProductMutation::KeyboardBacklight => ProductMutationEvidence {
            typed_backend: true,
            authorization_boundary: true,
            non_mutating_preflight: true,
            authoritative_hardware_readback: true,
            configuration_readback: false,
            executable_validation: false,
            product_policy_approved: false,
        },
        ProductMutation::AuraStaticRgb => ProductMutationEvidence {
            typed_backend: true,
            authorization_boundary: true,
            non_mutating_preflight: true,
            authoritative_hardware_readback: false,
            configuration_readback: true,
            executable_validation: false,
            product_policy_approved: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_interactive_paths_have_source_preflight_but_remain_release_blocked() {
        for mutation in [
            ProductMutation::PanelOverdrive,
            ProductMutation::KeyboardBacklight,
            ProductMutation::AuraStaticRgb,
        ] {
            let evidence = current_source_evidence(mutation);
            assert!(evidence.non_mutating_preflight);
            let assessment =
                assess_product_mutation_promotion(mutation, PromotionScope::Interactive, evidence);
            assert!(!assessment.promotable());
            assert!(
                assessment
                    .blockers
                    .contains(&ProductMutationPromotionBlocker::ExecutableValidationMissing)
            );
            assert!(
                assessment
                    .blockers
                    .contains(&ProductMutationPromotionBlocker::ProductPolicyNotApproved)
            );
            assert!(
                !assessment
                    .blockers
                    .contains(&ProductMutationPromotionBlocker::NonMutatingPreflightMissing)
            );
        }
    }

    #[test]
    fn aura_config_readback_can_never_satisfy_unattended_hardware_confirmation() {
        let assessment = assess_product_mutation_promotion(
            ProductMutation::AuraStaticRgb,
            PromotionScope::Unattended,
            current_source_evidence(ProductMutation::AuraStaticRgb),
        );
        assert!(
            assessment
                .blockers
                .contains(&ProductMutationPromotionBlocker::HardwareReadBackRequiredForUnattended)
        );
    }

    #[test]
    fn fully_proven_interactive_path_is_promotable() {
        let evidence = ProductMutationEvidence {
            typed_backend: true,
            authorization_boundary: true,
            non_mutating_preflight: true,
            authoritative_hardware_readback: true,
            configuration_readback: true,
            executable_validation: true,
            product_policy_approved: true,
        };
        let assessment = assess_product_mutation_promotion(
            ProductMutation::PanelOverdrive,
            PromotionScope::Interactive,
            evidence,
        );
        assert!(assessment.promotable());
    }

    #[test]
    fn module_has_no_runtime_mutation_or_capability_upgrade_surface() {
        let source = include_str!("product_mutation_promotion.rs");
        let forbidden = [
            ["set_", "panel_overdrive("].concat(),
            ["set_", "keyboard_backlight("].concat(),
            ["set_", "aura_static_rgb("].concat(),
            ["CapabilityStatus::", "Supported"].concat(),
            ["Command", "::new"].concat(),
        ];
        for token in forbidden {
            assert!(
                !source.contains(&token),
                "unexpected promotion side effect: {token}"
            );
        }
    }
}
