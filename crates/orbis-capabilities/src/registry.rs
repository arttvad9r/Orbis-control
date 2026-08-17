//! Immutable capability registry snapshots and deterministic assembly.
//!
//! This module contains no probing, I/O, runtime state or backend access. It
//! only validates and freezes capability metadata supplied by a future probe
//! layer.

use std::collections::BTreeMap;
use std::time::SystemTime;

use thiserror::Error;

use orbis_core::capability::{
    Capability, CapabilityOperations, CapabilityStatus, DeviceCapabilities, FeatureId,
};

use crate::engine::CapabilityPart;

/// Errors produced while assembling a registry snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    /// A capability ID was supplied more than once.
    #[error("duplicate capability: {feature:?}")]
    DuplicateCapability {
        /// Capability ID supplied more than once.
        feature: FeatureId,
    },
    /// Overall and operation-level statuses contradict each other.
    #[error("inconsistent capability {feature:?}: {message}")]
    InconsistentCapability {
        /// Capability ID.
        feature: FeatureId,
        /// Explanation of the violated invariant.
        message: String,
    },
}

/// Immutable capability metadata snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CapabilityRegistrySnapshot {
    generation: u64,
    checked_at: SystemTime,
    capabilities: DeviceCapabilities,
}

impl CapabilityRegistrySnapshot {
    /// Snapshot generation, representing a runtime snapshot version.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Time at which this snapshot was assembled.
    pub fn checked_at(&self) -> SystemTime {
        self.checked_at
    }

    /// Look up one canonical capability ID.
    pub fn capability(&self, feature: FeatureId) -> Option<&Capability> {
        self.capabilities.features.get(&feature)
    }

    /// Whether the snapshot contains the capability ID.
    pub fn contains(&self, feature: FeatureId) -> bool {
        self.capabilities.features.contains_key(&feature)
    }

    /// Deterministically ordered capability iterator.
    pub fn iter(&self) -> impl Iterator<Item = (&FeatureId, &Capability)> {
        self.capabilities.features.iter()
    }

    /// Number of entries in this partial or complete snapshot.
    pub fn len(&self) -> usize {
        self.capabilities.features.len()
    }

    /// Whether this snapshot contains no capability entries.
    pub fn is_empty(&self) -> bool {
        self.capabilities.features.is_empty()
    }

    /// Borrow the existing domain collection without exposing mutability.
    pub fn device_capabilities(&self) -> &DeviceCapabilities {
        &self.capabilities
    }
}

/// Mutable assembly object which produces an immutable snapshot on `build`.
#[derive(Debug)]
pub struct CapabilityRegistryBuilder {
    generation: u64,
    checked_at: SystemTime,
    capabilities: BTreeMap<FeatureId, Capability>,
}

impl CapabilityRegistryBuilder {
    /// Create a builder for an explicitly numbered runtime snapshot.
    pub fn new(generation: u64, checked_at: SystemTime) -> Self {
        Self {
            generation,
            checked_at,
            capabilities: BTreeMap::new(),
        }
    }

    /// Add a fully described capability entry.
    ///
    /// New runtime entries use strict operation consistency validation.
    pub fn add(&mut self, feature: FeatureId, capability: Capability) -> Result<(), RegistryError> {
        validate_capability(feature, &capability, false)?;
        self.insert(feature, capability)
    }

    /// Add a legacy report part without inventing operation support metadata.
    ///
    /// `CapabilityPart` predates operation-level metadata. Its resulting
    /// operations and constraints remain `Unknown` by design.
    pub fn add_part(&mut self, part: CapabilityPart) -> Result<(), RegistryError> {
        let capability = match part.reason {
            Some(reason) => Capability::with_reason(part.status, reason),
            None => Capability::new(part.status),
        };
        validate_capability(part.feature, &capability, true)?;
        self.insert(part.feature, capability)
    }

    fn insert(&mut self, feature: FeatureId, capability: Capability) -> Result<(), RegistryError> {
        if self.capabilities.contains_key(&feature) {
            return Err(RegistryError::DuplicateCapability { feature });
        }
        self.capabilities.insert(feature, capability);
        Ok(())
    }

    /// Freeze all assembled entries into an immutable snapshot.
    pub fn build(self) -> Result<CapabilityRegistrySnapshot, RegistryError> {
        Ok(CapabilityRegistrySnapshot {
            generation: self.generation,
            checked_at: self.checked_at,
            capabilities: DeviceCapabilities {
                features: self.capabilities,
            },
        })
    }
}

fn operation_is_supported(status: CapabilityStatus) -> bool {
    matches!(
        status,
        CapabilityStatus::Supported | CapabilityStatus::SupportedWithRequirement
    )
}

fn operation_indicates_read_support(status: CapabilityStatus) -> bool {
    operation_is_supported(status) || status == CapabilityStatus::ReadOnly
}

fn all_operations_unknown(operations: &CapabilityOperations) -> bool {
    operations.read.status == CapabilityStatus::Unknown
        && operations.write.status == CapabilityStatus::Unknown
}

fn validate_capability(
    feature: FeatureId,
    capability: &Capability,
    allow_legacy_unknown: bool,
) -> Result<(), RegistryError> {
    let operations = &capability.operations;
    let read = operations.read.status;
    let write = operations.write.status;

    // Old Capability/CapabilityPart records have no operation metadata. Keep
    // them representable only through the explicit legacy assembly path.
    if allow_legacy_unknown && all_operations_unknown(operations) {
        return Ok(());
    }

    let invalid = |message: &str| RegistryError::InconsistentCapability {
        feature,
        message: message.to_string(),
    };

    match capability.status {
        CapabilityStatus::Supported => {
            if !operation_is_supported(read) && !operation_is_supported(write) {
                return Err(invalid(
                    "overall Supported requires at least one supported operation",
                ));
            }
        }
        CapabilityStatus::SupportedWithRequirement => {
            if !operation_is_supported(read) && !operation_is_supported(write) {
                return Err(invalid(
                    "overall SupportedWithRequirement requires a supported operation",
                ));
            }
        }
        CapabilityStatus::ReadOnly => {
            if !operation_indicates_read_support(read) {
                return Err(invalid(
                    "overall ReadOnly requires a supported read operation",
                ));
            }
            if operation_is_supported(write) {
                return Err(invalid(
                    "overall ReadOnly cannot have a supported write operation",
                ));
            }
        }
        CapabilityStatus::Unsupported | CapabilityStatus::BackendMissing => {
            if operation_is_supported(read) || operation_is_supported(write) {
                return Err(invalid(
                    "overall unsupported/backend-missing cannot have a supported operation",
                ));
            }
        }
        CapabilityStatus::TemporarilyUnavailable => {
            if operation_is_supported(read) || operation_is_supported(write) {
                return Err(invalid(
                    "overall temporarily-unavailable cannot have a currently supported operation",
                ));
            }
        }
        CapabilityStatus::PermissionDenied => {
            if read != CapabilityStatus::PermissionDenied
                && write != CapabilityStatus::PermissionDenied
            {
                return Err(invalid(
                    "overall PermissionDenied requires a denied operation",
                ));
            }
        }
        CapabilityStatus::Unknown
        | CapabilityStatus::Conflicted
        | CapabilityStatus::Experimental => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::battery::ChargeLimitBounds;
    use orbis_core::capability::{
        CapabilityConstraints, CapabilityOperations, OperationCapability,
    };
    use orbis_core::newtypes::Percent;
    use orbis_core::profile::PerformanceProfile;

    fn at(generation: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(generation)
    }

    fn operations(read: CapabilityStatus, write: CapabilityStatus) -> CapabilityOperations {
        CapabilityOperations {
            read: OperationCapability::new(read),
            write: OperationCapability::new(write),
        }
    }

    fn capability(
        status: CapabilityStatus,
        read: CapabilityStatus,
        write: CapabilityStatus,
    ) -> Capability {
        Capability::new(status).with_operations(operations(read, write))
    }

    #[test]
    fn empty_snapshot_builds() {
        let snapshot = CapabilityRegistryBuilder::new(1, at(1)).build().unwrap();
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.generation(), 1);
    }

    #[test]
    fn ordering_and_lookup_are_deterministic() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        builder
            .add(
                FeatureId::GpuPower,
                capability(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::ReadOnly,
                ),
            )
            .unwrap();
        builder
            .add(
                FeatureId::Performance,
                capability(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                ),
            )
            .unwrap();
        let snapshot = builder.build().unwrap();
        let ids: Vec<_> = snapshot.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![FeatureId::Performance, FeatureId::GpuPower]);
        assert!(snapshot.contains(FeatureId::GpuPower));
        assert_eq!(
            snapshot.capability(FeatureId::GpuPower).unwrap().status,
            CapabilityStatus::Supported
        );
    }

    #[test]
    fn duplicate_feature_is_rejected() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        let entry = capability(
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
            CapabilityStatus::ReadOnly,
        );
        builder.add(FeatureId::GpuPower, entry.clone()).unwrap();
        assert_eq!(
            builder.add(FeatureId::GpuPower, entry),
            Err(RegistryError::DuplicateCapability {
                feature: FeatureId::GpuPower
            })
        );
    }

    #[test]
    fn generation_and_timestamp_are_preserved() {
        let checked_at = at(42);
        let snapshot = CapabilityRegistryBuilder::new(7, checked_at)
            .build()
            .unwrap();
        assert_eq!(snapshot.generation(), 7);
        assert_eq!(snapshot.checked_at(), checked_at);
    }

    #[test]
    fn supported_read_and_unsupported_write_are_valid() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        builder
            .add(
                FeatureId::GpuPower,
                capability(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::Unsupported,
                ),
            )
            .unwrap();
    }

    #[test]
    fn supported_read_and_permission_denied_write_are_valid() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        builder
            .add(
                FeatureId::ChargeLimit,
                capability(
                    CapabilityStatus::Supported,
                    CapabilityStatus::Supported,
                    CapabilityStatus::PermissionDenied,
                ),
            )
            .unwrap();
    }

    #[test]
    fn contradictory_status_is_rejected() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        let unsupported = capability(
            CapabilityStatus::Unsupported,
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
        );
        assert!(matches!(
            builder.add(FeatureId::GpuProductPolicy, unsupported),
            Err(RegistryError::InconsistentCapability { .. })
        ));

        let readonly_write = capability(
            CapabilityStatus::ReadOnly,
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
        );
        assert!(matches!(
            builder.add(FeatureId::GpuMux, readonly_write),
            Err(RegistryError::InconsistentCapability { .. })
        ));
    }

    #[test]
    fn unknown_operations_are_accepted_only_as_legacy_parts() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        assert!(
            builder
                .add_part(CapabilityPart {
                    feature: FeatureId::Performance,
                    status: CapabilityStatus::Supported,
                    reason: None,
                })
                .is_ok()
        );
        let snapshot = builder.build().unwrap();
        let entry = snapshot.capability(FeatureId::Performance).unwrap();
        assert_eq!(entry.operations.read.status, CapabilityStatus::Unknown);
        assert_eq!(entry.operations.write.status, CapabilityStatus::Unknown);
        assert_eq!(entry.constraints, CapabilityConstraints::Unknown);
    }

    #[test]
    fn unknown_constraints_are_preserved() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        let entry = capability(
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
        )
        .with_constraints(CapabilityConstraints::Unknown);
        builder.add(FeatureId::GpuPower, entry).unwrap();
        let snapshot = builder.build().unwrap();
        assert_eq!(
            snapshot
                .capability(FeatureId::GpuPower)
                .unwrap()
                .constraints,
            CapabilityConstraints::Unknown
        );
        assert_ne!(CapabilityConstraints::Unknown, CapabilityConstraints::None);
    }

    #[test]
    fn battery_constraints_do_not_store_observed_state() {
        let bounds =
            ChargeLimitBounds::new(Percent::new(40).unwrap(), Percent::new(100).unwrap(), 1)
                .unwrap();
        let entry = capability(
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
        )
        .with_constraints(CapabilityConstraints::ChargeLimit(bounds));
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        builder.add(FeatureId::ChargeLimit, entry).unwrap();
        let snapshot = builder.build().unwrap();
        let stored = snapshot.capability(FeatureId::ChargeLimit).unwrap();
        assert_eq!(
            stored.constraints,
            CapabilityConstraints::ChargeLimit(bounds)
        );
        assert!(stored.reason.is_none());
    }

    #[test]
    fn gpu_primitives_are_independent_and_product_is_not_synthesized() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        for feature in [FeatureId::GpuPower, FeatureId::GpuMux, FeatureId::GpuAccess] {
            builder
                .add(
                    feature,
                    capability(
                        CapabilityStatus::Supported,
                        CapabilityStatus::Supported,
                        CapabilityStatus::ReadOnly,
                    ),
                )
                .unwrap();
        }
        let snapshot = builder.build().unwrap();
        assert_eq!(snapshot.len(), 3);
        assert!(!snapshot.contains(FeatureId::GpuProductPolicy));
    }

    #[test]
    fn gpu_product_policy_requires_explicit_entry() {
        let mut builder = CapabilityRegistryBuilder::new(1, at(1));
        builder
            .add(
                FeatureId::GpuProductPolicy,
                capability(
                    CapabilityStatus::Unsupported,
                    CapabilityStatus::Unsupported,
                    CapabilityStatus::Unsupported,
                ),
            )
            .unwrap();
        assert_eq!(
            builder
                .build()
                .unwrap()
                .capability(FeatureId::GpuProductPolicy)
                .unwrap()
                .status,
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn performance_constraints_exclude_current_profile() {
        let entry = capability(
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
        )
        .with_constraints(CapabilityConstraints::PerformanceProfiles(vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ]));
        assert!(matches!(
            entry.constraints,
            CapabilityConstraints::PerformanceProfiles(_)
        ));
    }
}
