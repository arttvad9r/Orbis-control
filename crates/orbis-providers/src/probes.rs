//! Read-only capability adapters for Performance, Battery, and GPU primitives.
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
use crate::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};

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

/// Probe GPU runtime power capability.
///
/// The resulting power value is read only to confirm the read contract; the
/// observed `GpuPowerState` (e.g. `Active`/`Suspended`/`Off`/`Stale`) does
/// not enter the capability metadata.
pub async fn probe_gpu_power<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: GpuPowerProvider + ?Sized,
{
    match provider.power_state().await {
        Ok(_) => Ok(supported_read_only()),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            Ok(capability_from_read(read, CapabilityConstraints::Unknown))
        }
    }
}

/// Probe physical GPU MUX capability.
///
/// The resulting MUX value is read only to confirm the read contract; the
/// observed `GpuMuxState` (e.g. `Integrated`/`Discrete`) does not enter the
/// capability metadata. Provider-level `Unsupported` for an absent firmware
/// attribute is classified by the existing `ProviderError → ProbeClassification`
/// adapter rather than escalated to a separate `BackendMissing`.
pub async fn probe_gpu_mux<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: GpuMuxProvider + ?Sized,
{
    match provider.mux_state().await {
        Ok(_) => Ok(supported_read_only()),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            Ok(capability_from_read(read, CapabilityConstraints::Unknown))
        }
    }
}

/// Probe dGPU access policy capability.
///
/// The resulting `GpuAccessPolicy` is read only to confirm the read contract;
/// the observed `Blocked`/`Unblocked`/`Pending` value does not enter the
/// capability metadata.
pub async fn probe_gpu_access<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: GpuAccessProvider + ?Sized,
{
    match provider.access_policy().await {
        Ok(_) => Ok(supported_read_only()),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            Ok(capability_from_read(read, CapabilityConstraints::Unknown))
        }
    }
}

fn supported_read_only() -> Capability {
    let read = ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
    let write = ProbeOperationResult::with_detail(
        ProbeClassification::Unsupported,
        "GPU primitive read-only probe does not establish write support",
    )
    .into_operation();
    capability_from_operations(
        CapabilityOperations { read, write },
        CapabilityConstraints::Unknown,
    )
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
    use crate::traits::{
        GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, Provider, ProviderHealth,
    };
    use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};

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
        gpu_power: Scripted<GpuPowerState>,
        gpu_mux: Scripted<GpuMuxState>,
        gpu_access: Scripted<GpuAccessPolicy>,
    }

    impl ScriptedProvider {
        fn performance(profiles: Scripted<Vec<PerformanceProfile>>) -> Self {
            Self {
                profiles,
                current: Scripted::Value(PerformanceProfile::Balanced),
                charge_limit: Scripted::Error(ScriptedError::Unsupported),
                gpu_power: Scripted::Error(ScriptedError::Unsupported),
                gpu_mux: Scripted::Error(ScriptedError::Unsupported),
                gpu_access: Scripted::Error(ScriptedError::Unsupported),
            }
        }

        fn battery(charge_limit: Scripted<ChargeLimit>) -> Self {
            Self {
                profiles: Scripted::Error(ScriptedError::Unsupported),
                current: Scripted::Error(ScriptedError::Unsupported),
                charge_limit,
                gpu_power: Scripted::Error(ScriptedError::Unsupported),
                gpu_mux: Scripted::Error(ScriptedError::Unsupported),
                gpu_access: Scripted::Error(ScriptedError::Unsupported),
            }
        }

        fn gpu(
            power: Scripted<GpuPowerState>,
            mux: Scripted<GpuMuxState>,
            access: Scripted<GpuAccessPolicy>,
        ) -> Self {
            Self {
                profiles: Scripted::Error(ScriptedError::Unsupported),
                current: Scripted::Error(ScriptedError::Unsupported),
                charge_limit: Scripted::Error(ScriptedError::Unsupported),
                gpu_power: power,
                gpu_mux: mux,
                gpu_access: access,
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

    #[async_trait]
    impl GpuPowerProvider for ScriptedProvider {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            self.gpu_power.result()
        }
    }

    #[async_trait]
    impl GpuMuxProvider for ScriptedProvider {
        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            self.gpu_mux.result()
        }
    }

    #[async_trait]
    impl GpuAccessProvider for ScriptedProvider {
        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            self.gpu_access.result()
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

    #[tokio::test]
    async fn gpu_power_probe_reports_supported_without_storing_state() {
        for state in [
            GpuPowerState::Active,
            GpuPowerState::Suspended,
            GpuPowerState::Off,
            GpuPowerState::Stale,
            GpuPowerState::Unknown,
        ] {
            let capability = probe_gpu_power(&ScriptedProvider::gpu(
                Scripted::Value(state),
                Scripted::Error(ScriptedError::Unsupported),
                Scripted::Error(ScriptedError::Unsupported),
            ))
            .await
            .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported
            );
            assert_eq!(
                capability.operations.write.status,
                CapabilityStatus::Unsupported
            );
            assert_eq!(capability.constraints, CapabilityConstraints::Unknown);
            assert!(capability.reason.is_none());
        }
    }

    #[tokio::test]
    async fn gpu_power_probe_propagates_backend_missing_for_power_read() {
        let missing = probe_gpu_power(&ScriptedProvider::gpu(
            Scripted::Error(ScriptedError::BackendMissing),
            Scripted::Error(ScriptedError::Unsupported),
            Scripted::Error(ScriptedError::Unsupported),
        ))
        .await
        .unwrap();
        assert_eq!(
            missing.operations.read.status,
            CapabilityStatus::BackendMissing
        );
    }

    #[tokio::test]
    async fn gpu_mux_probe_supported_for_proven_values_and_unsupported_when_absent() {
        for state in [
            GpuMuxState::Integrated,
            GpuMuxState::Discrete,
            GpuMuxState::Unknown,
        ] {
            let capability = probe_gpu_mux(&ScriptedProvider::gpu(
                Scripted::Error(ScriptedError::Unsupported),
                Scripted::Value(state),
                Scripted::Error(ScriptedError::Unsupported),
            ))
            .await
            .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported
            );
            assert_eq!(
                capability.operations.write.status,
                CapabilityStatus::Unsupported
            );
            assert_eq!(capability.constraints, CapabilityConstraints::Unknown);
        }

        let absent = probe_gpu_mux(&ScriptedProvider::gpu(
            Scripted::Error(ScriptedError::Unsupported),
            Scripted::Error(ScriptedError::Unsupported),
            Scripted::Error(ScriptedError::Unsupported),
        ))
        .await
        .unwrap();
        assert_eq!(absent.operations.read.status, CapabilityStatus::Unsupported);
    }

    #[tokio::test]
    async fn gpu_access_probe_supported_and_writes_remain_unsupported() {
        for state in [
            GpuAccessPolicy::Unblocked,
            GpuAccessPolicy::Blocked,
            GpuAccessPolicy::Pending,
            GpuAccessPolicy::Unknown,
        ] {
            let capability = probe_gpu_access(&ScriptedProvider::gpu(
                Scripted::Error(ScriptedError::Unsupported),
                Scripted::Error(ScriptedError::Unsupported),
                Scripted::Value(state),
            ))
            .await
            .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported
            );
            assert_eq!(
                capability.operations.write.status,
                CapabilityStatus::Unsupported
            );
            assert_eq!(capability.constraints, CapabilityConstraints::Unknown);
        }

        let absent = probe_gpu_access(&ScriptedProvider::gpu(
            Scripted::Error(ScriptedError::Unsupported),
            Scripted::Error(ScriptedError::Unsupported),
            Scripted::Error(ScriptedError::Unsupported),
        ))
        .await
        .unwrap();
        assert_eq!(absent.operations.read.status, CapabilityStatus::Unsupported);
    }

    #[tokio::test]
    async fn gpu_primitive_probes_fail_independently_without_dropping_others() {
        let provider = ScriptedProvider::gpu(
            Scripted::Value(GpuPowerState::Suspended),
            Scripted::Error(ScriptedError::BackendMissing),
            Scripted::Value(GpuAccessPolicy::Blocked),
        );
        let power = probe_gpu_power(&provider).await.unwrap();
        let mux = probe_gpu_mux(&provider).await.unwrap();
        let access = probe_gpu_access(&provider).await.unwrap();
        assert_eq!(power.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(mux.operations.read.status, CapabilityStatus::BackendMissing);
        assert_eq!(access.operations.read.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn registry_does_not_synthesise_gpu_product_policy() {
        let provider = ScriptedProvider::gpu(
            Scripted::Value(GpuPowerState::Suspended),
            Scripted::Value(GpuMuxState::Integrated),
            Scripted::Value(GpuAccessPolicy::Unblocked),
        );
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        builder
            .add(
                orbis_core::FeatureId::GpuPower,
                probe_gpu_power(&provider).await.unwrap(),
            )
            .unwrap();
        builder
            .add(
                orbis_core::FeatureId::GpuMux,
                probe_gpu_mux(&provider).await.unwrap(),
            )
            .unwrap();
        builder
            .add(
                orbis_core::FeatureId::GpuAccess,
                probe_gpu_access(&provider).await.unwrap(),
            )
            .unwrap();
        let snapshot = builder.build().unwrap();
        assert!(!snapshot.contains(orbis_core::FeatureId::GpuProductPolicy));
        assert!(snapshot.contains(orbis_core::FeatureId::GpuPower));
        assert!(snapshot.contains(orbis_core::FeatureId::GpuMux));
        assert!(snapshot.contains(orbis_core::FeatureId::GpuAccess));
    }
}
