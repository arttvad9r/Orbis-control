//! Read-only capability adapters for Performance and Battery.
//!
//! These functions execute provider reads but never execute mutation methods.
//! They return capability metadata only; observed values are deliberately
//! discarded after the provider contract has been established.

use orbis_capabilities::{
    ProbeClassification, ProbeContext, ProbeError, ProbeOperationResult, capability_from_operations,
};
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, OperationCapability,
};

use crate::error::ProviderError;
use crate::traits::{BatteryProvider, PerformanceProvider};

fn operation_from_error(
    error: &ProviderError,
    context: ProbeContext,
) -> Result<OperationCapability, ProbeError> {
    error
        .into_probe_result(context)
        .map(ProbeOperationResult::into_operation)
}

fn unsupported_write() -> OperationCapability {
    ProbeOperationResult::with_detail(
        ProbeClassification::Unsupported,
        "read-only capability probe does not establish mutation support",
    )
    .into_operation()
}

fn capability_from_read(
    read: OperationCapability,
    constraints: CapabilityConstraints,
) -> Capability {
    capability_from_operations(
        CapabilityOperations {
            read,
            write: unsupported_write(),
        },
        constraints,
    )
}

/// Probe Performance support from current and available profile reads.
///
/// The current profile is read only to establish that the read contract is
/// usable; it is not stored in the returned capability metadata.
pub async fn probe_performance<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: PerformanceProvider + ?Sized,
{
    let profiles = match provider.profiles().await {
        Ok(profiles) => profiles,
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            return Ok(capability_from_read(read, CapabilityConstraints::Unknown));
        }
    };

    if profiles.is_empty() {
        let read = ProbeOperationResult::with_detail(
            ProbeClassification::Unsupported,
            "performance backend reported no profiles",
        )
        .into_operation();
        return Ok(capability_from_read(read, CapabilityConstraints::Unknown));
    }

    if let Err(error) = provider.current_profile().await {
        let read = operation_from_error(&error, ProbeContext::EstablishedBackend)?;
        return Ok(capability_from_read(read, CapabilityConstraints::Unknown));
    }

    let read = ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
    Ok(capability_from_read(
        read,
        CapabilityConstraints::PerformanceProfiles(profiles),
    ))
}

/// Probe Battery charge-limit support from an authoritative read.
///
/// The returned `ChargeLimit` is used only for support/bounds metadata. Its
/// enabled/configured/effective values never enter the capability result.
pub async fn probe_charge_limit<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: BatteryProvider + ?Sized,
{
    let charge_limit = match provider.charge_limit().await {
        Ok(charge_limit) => charge_limit,
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            return Ok(capability_from_read(read, CapabilityConstraints::Unknown));
        }
    };

    let constraints = charge_limit
        .bounds
        .map(CapabilityConstraints::ChargeLimit)
        .unwrap_or(CapabilityConstraints::Unknown);
    let read = ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
    Ok(capability_from_read(read, constraints))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_core::action::ApplyResult;
    use orbis_core::battery::{ChargeLimit, ChargeLimitBounds};
    use orbis_core::capability::{CapabilityConstraints, CapabilityStatus};
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::identity::BackendIdentity;
    use orbis_core::newtypes::Percent;
    use orbis_core::profile::PerformanceProfile;

    use super::*;
    use crate::error::ValidationResult;
    use crate::traits::{Provider, ProviderHealth};

    #[derive(Debug, Clone)]
    enum ScriptedError {
        BackendMissing,
        Unsupported,
        PermissionDenied,
    }

    impl ScriptedError {
        fn provider_error(&self) -> ProviderError {
            match self {
                Self::BackendMissing => ProviderError::BackendUnavailable("missing".into()),
                Self::Unsupported => ProviderError::Unsupported("unsupported".into()),
                Self::PermissionDenied => ProviderError::PermissionDenied("denied".into()),
            }
        }
    }

    #[derive(Debug, Clone)]
    enum Scripted<T> {
        Value(T),
        Error(ScriptedError),
    }

    impl<T: Clone> Scripted<T> {
        fn result(&self) -> Result<T, ProviderError> {
            match self {
                Self::Value(value) => Ok(value.clone()),
                Self::Error(error) => Err(error.provider_error()),
            }
        }
    }

    #[derive(Debug, Clone)]
    struct ScriptedProvider {
        profiles: Scripted<Vec<PerformanceProfile>>,
        current: Scripted<PerformanceProfile>,
        charge_limit: Scripted<ChargeLimit>,
    }

    impl ScriptedProvider {
        fn performance(profiles: Scripted<Vec<PerformanceProfile>>) -> Self {
            Self {
                profiles,
                current: Scripted::Value(PerformanceProfile::Balanced),
                charge_limit: Scripted::Error(ScriptedError::Unsupported),
            }
        }

        fn battery(charge_limit: Scripted<ChargeLimit>) -> Self {
            Self {
                profiles: Scripted::Error(ScriptedError::Unsupported),
                current: Scripted::Error(ScriptedError::Unsupported),
                charge_limit,
            }
        }
    }

    impl Provider for ScriptedProvider {
        fn id(&self) -> &'static str {
            "scripted"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("unsupported: {feature}")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl PerformanceProvider for ScriptedProvider {
        async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
            self.profiles.result()
        }

        async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
            self.current.result()
        }

        async fn set_profile(
            &self,
            _profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("probe must not write".into()))
        }

        async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Err(ProviderError::Unsupported("not part of probe".into()))
        }

        async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Err(ProviderError::Unsupported("not part of probe".into()))
        }

        fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
            ValidationResult::invalid("probe provider")
        }
    }

    #[async_trait]
    impl BatteryProvider for ScriptedProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            self.charge_limit.result()
        }

        async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("probe must not write".into()))
        }

        async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("probe must not write".into()))
        }

        fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
            ValidationResult::invalid("probe provider")
        }
    }

    fn charge_limit(bounds: Option<ChargeLimitBounds>) -> ChargeLimit {
        ChargeLimit::new(
            false,
            Some(Percent::new(80).unwrap()),
            Some(Percent::new(80).unwrap()),
            bounds,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn performance_probe_reports_profiles_without_current_value() {
        let provider = ScriptedProvider::performance(Scripted::Value(vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ]));
        let capability = probe_performance(&provider).await.unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            capability.constraints,
            CapabilityConstraints::PerformanceProfiles(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])
        );
        assert_eq!(capability.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn performance_probe_preserves_backend_missing_and_unsupported() {
        for (error, expected) in [
            (
                ScriptedError::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (ScriptedError::Unsupported, CapabilityStatus::Unsupported),
        ] {
            let capability =
                probe_performance(&ScriptedProvider::performance(Scripted::Error(error)))
                    .await
                    .unwrap();
            assert_eq!(capability.operations.read.status, expected);
        }
    }

    #[tokio::test]
    async fn battery_probe_preserves_known_and_unknown_bounds() {
        let bounds =
            ChargeLimitBounds::new(Percent::new(40).unwrap(), Percent::new(100).unwrap(), 1)
                .unwrap();
        let known = probe_charge_limit(&ScriptedProvider::battery(Scripted::Value(charge_limit(
            Some(bounds),
        ))))
        .await
        .unwrap();
        assert_eq!(known.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(
            known.constraints,
            CapabilityConstraints::ChargeLimit(bounds)
        );

        let unknown = probe_charge_limit(&ScriptedProvider::battery(Scripted::Value(
            charge_limit(None),
        )))
        .await
        .unwrap();
        assert_eq!(unknown.constraints, CapabilityConstraints::Unknown);
    }

    #[tokio::test]
    async fn battery_probe_preserves_backend_and_permission_errors() {
        let missing = probe_charge_limit(&ScriptedProvider::battery(Scripted::Error(
            ScriptedError::BackendMissing,
        )))
        .await
        .unwrap();
        assert_eq!(
            missing.operations.read.status,
            CapabilityStatus::BackendMissing
        );

        let denied = probe_charge_limit(&ScriptedProvider::battery(Scripted::Error(
            ScriptedError::PermissionDenied,
        )))
        .await
        .unwrap();
        assert_eq!(
            denied.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
    }

    #[tokio::test]
    async fn probes_assemble_through_registry_builder() {
        let performance = probe_performance(&ScriptedProvider::performance(Scripted::Value(vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
        ])))
        .await
        .unwrap();
        let battery = probe_charge_limit(&ScriptedProvider::battery(Scripted::Value(
            charge_limit(None),
        )))
        .await
        .unwrap();

        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        builder
            .add(orbis_core::FeatureId::Performance, performance)
            .unwrap();
        builder
            .add(orbis_core::FeatureId::ChargeLimit, battery)
            .unwrap();
        let snapshot = builder.build().unwrap();
        assert!(snapshot.contains(orbis_core::FeatureId::Performance));
        assert!(snapshot.contains(orbis_core::FeatureId::ChargeLimit));
    }
}
