//! Typed result contract for future read-only capability probes.
//!
//! This module classifies evidence only. It does not execute I/O, inspect
//! hardware or own a runtime registry.

use thiserror::Error;

use orbis_core::action::ActionRequirement;
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityReason, CapabilityStatus,
    OperationCapability,
};

/// Context needed to distinguish an absent backend from an unavailable one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeContext {
    /// The probe is establishing whether the required backend exists.
    BackendDiscovery,
    /// The backend is known/owned, but the current operation may be transiently unavailable.
    EstablishedBackend,
}

/// Evidence classification produced by a capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeClassification {
    /// The operation is supported.
    Supported,
    /// The operation is supported with a lifecycle/action requirement.
    SupportedWithRequirement(ActionRequirement),
    /// The operation is proven unsupported by the backend contract.
    Unsupported,
    /// The required backend/service is structurally absent.
    BackendMissing,
    /// A known backend is temporarily unavailable.
    TemporarilyUnavailable,
    /// The operation exists but current authorization evidence denies it.
    PermissionDenied,
    /// Evidence is insufficient to classify the operation.
    Unknown,
}

impl ProbeClassification {
    /// Convert evidence classification to the canonical domain status.
    pub fn status(self) -> CapabilityStatus {
        match self {
            Self::Supported => CapabilityStatus::Supported,
            Self::SupportedWithRequirement(_) => CapabilityStatus::SupportedWithRequirement,
            Self::Unsupported => CapabilityStatus::Unsupported,
            Self::BackendMissing => CapabilityStatus::BackendMissing,
            Self::TemporarilyUnavailable => CapabilityStatus::TemporarilyUnavailable,
            Self::PermissionDenied => CapabilityStatus::PermissionDenied,
            Self::Unknown => CapabilityStatus::Unknown,
        }
    }

    /// Requirement carried by a supported-with-requirement classification.
    pub fn requirement(self) -> Option<ActionRequirement> {
        match self {
            Self::SupportedWithRequirement(requirement) => Some(requirement),
            _ => None,
        }
    }
}

/// Result of probing one operation, without any observed hardware value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOperationResult {
    /// Evidence classification.
    pub classification: ProbeClassification,
    /// Optional diagnostic reason for the classification.
    pub reason: Option<CapabilityReason>,
}

impl ProbeOperationResult {
    /// Create a classified operation result without a diagnostic reason.
    pub fn classified(classification: ProbeClassification) -> Self {
        Self {
            classification,
            reason: None,
        }
    }

    /// Create a result with a technical diagnostic reason.
    pub fn with_detail(classification: ProbeClassification, detail: impl Into<String>) -> Self {
        Self {
            classification,
            reason: Some(CapabilityReason {
                reason: detail.into(),
                suggestion: String::new(),
                backend: None,
                endpoint: None,
                requirement: classification.requirement(),
                risk: orbis_core::capability::RiskLevel::Safe,
                checked_at: None,
            }),
        }
    }

    /// Convert probe evidence to the existing operation-level domain model.
    pub fn into_operation(self) -> OperationCapability {
        OperationCapability {
            status: self.classification.status(),
            reason: self.reason,
        }
    }
}

/// Probe execution/contract failures which must not be silently reported as
/// ordinary unsupported hardware.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProbeError {
    /// Provider returned a request/invariant violation while probing.
    #[error("probe contract violation: {0}")]
    ContractViolation(String),
    /// Provider or protocol failed internally while producing evidence.
    #[error("probe internal failure: {0}")]
    Internal(String),
}

/// Convert a generic D-Bus diagnostic string to a conservative classification.
///
/// Providers should prefer typed D-Bus errors when available. This helper only
/// handles stable well-known error names/messages and returns `Unknown` for
/// generic/unrecognized D-Bus failures.
pub fn classify_dbus_detail(detail: &str, _context: ProbeContext) -> ProbeClassification {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("accessdenied") || lower.contains("permission denied") {
        ProbeClassification::PermissionDenied
    } else if lower.contains("unknownmethod") || lower.contains("not supported") {
        ProbeClassification::Unsupported
    } else if lower.contains("serviceunknown")
        || lower.contains("namehasnoowner")
        || lower.contains("service not found")
    {
        ProbeClassification::BackendMissing
    } else if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("noreply")
        || lower.contains("disconnected")
    {
        ProbeClassification::TemporarilyUnavailable
    } else {
        ProbeClassification::Unknown
    }
}

/// Resolve one overall capability status from independently classified
/// operations. This is the shared policy used by all capability adapters.
pub fn resolve_overall_status(operations: &CapabilityOperations) -> CapabilityStatus {
    if operations.read.status == CapabilityStatus::SupportedWithRequirement
        || operations.write.status == CapabilityStatus::SupportedWithRequirement
    {
        return CapabilityStatus::SupportedWithRequirement;
    }
    if operations.read.status == CapabilityStatus::Supported
        || operations.write.status == CapabilityStatus::Supported
    {
        return CapabilityStatus::Supported;
    }
    if operations.read.status == CapabilityStatus::ReadOnly
        || operations.write.status == CapabilityStatus::ReadOnly
    {
        return CapabilityStatus::ReadOnly;
    }
    if operations.read.status == CapabilityStatus::PermissionDenied
        || operations.write.status == CapabilityStatus::PermissionDenied
    {
        return CapabilityStatus::PermissionDenied;
    }
    if operations.read.status == CapabilityStatus::BackendMissing
        || operations.write.status == CapabilityStatus::BackendMissing
    {
        return CapabilityStatus::BackendMissing;
    }
    if operations.read.status == CapabilityStatus::TemporarilyUnavailable
        || operations.write.status == CapabilityStatus::TemporarilyUnavailable
    {
        return CapabilityStatus::TemporarilyUnavailable;
    }
    if operations.read.status == CapabilityStatus::Unsupported
        || operations.write.status == CapabilityStatus::Unsupported
    {
        return CapabilityStatus::Unsupported;
    }
    CapabilityStatus::Unknown
}

/// Build a capability from operation results and typed constraints.
///
/// The caller still submits the returned value to
/// `CapabilityRegistryBuilder`, which performs final consistency validation.
pub fn capability_from_operations(
    operations: CapabilityOperations,
    constraints: CapabilityConstraints,
) -> Capability {
    Capability::new(resolve_overall_status(&operations))
        .with_operations(operations)
        .with_constraints(constraints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::capability::{CapabilityOperations, CapabilityStatus};

    #[test]
    fn classifications_map_to_canonical_statuses() {
        assert_eq!(
            ProbeClassification::Supported.status(),
            CapabilityStatus::Supported
        );
        assert_eq!(
            ProbeClassification::SupportedWithRequirement(ActionRequirement::Reboot).status(),
            CapabilityStatus::SupportedWithRequirement
        );
        assert_eq!(
            ProbeClassification::BackendMissing.status(),
            CapabilityStatus::BackendMissing
        );
        assert_eq!(
            ProbeClassification::TemporarilyUnavailable.status(),
            CapabilityStatus::TemporarilyUnavailable
        );
        assert_eq!(
            ProbeClassification::PermissionDenied.status(),
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            ProbeClassification::Unknown.status(),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn probe_result_converts_to_operation_without_observed_value() {
        let operation = ProbeOperationResult::with_detail(
            ProbeClassification::PermissionDenied,
            "authorization evidence denied",
        )
        .into_operation();
        assert_eq!(operation.status, CapabilityStatus::PermissionDenied);
        assert!(operation.reason.is_some());

        let operations = CapabilityOperations {
            read: operation,
            write: ProbeOperationResult::classified(ProbeClassification::Unsupported)
                .into_operation(),
        };
        assert_eq!(operations.write.status, CapabilityStatus::Unsupported);
    }

    #[test]
    fn dbus_classification_is_conservative_and_context_aware() {
        assert_eq!(
            classify_dbus_detail(
                "org.freedesktop.DBus.Error.ServiceUnknown",
                ProbeContext::BackendDiscovery
            ),
            ProbeClassification::BackendMissing
        );
        assert_eq!(
            classify_dbus_detail(
                "org.freedesktop.DBus.Error.AccessDenied",
                ProbeContext::EstablishedBackend
            ),
            ProbeClassification::PermissionDenied
        );
        assert_eq!(
            classify_dbus_detail(
                "org.freedesktop.DBus.Error.NoReply timeout",
                ProbeContext::EstablishedBackend
            ),
            ProbeClassification::TemporarilyUnavailable
        );
        assert_eq!(
            classify_dbus_detail(
                "org.freedesktop.DBus.Error.UnknownMethod",
                ProbeContext::EstablishedBackend
            ),
            ProbeClassification::Unsupported
        );
        assert_eq!(
            classify_dbus_detail(
                "unexpected protocol failure",
                ProbeContext::EstablishedBackend
            ),
            ProbeClassification::Unknown
        );
    }

    #[test]
    fn probe_errors_are_not_operation_statuses() {
        let error = ProbeError::Internal("malformed protocol reply".into());
        assert_eq!(
            error.to_string(),
            "probe internal failure: malformed protocol reply"
        );
    }
}
