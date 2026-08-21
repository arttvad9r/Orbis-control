//! Read-only capability adapters for Performance, Battery, and GPU primitives.
//!
//! These functions execute provider reads but never execute mutation methods.
//! They return capability metadata only; observed values are deliberately
//! discarded after the provider contract has been established.
//!
//! Write capability for Performance and ChargeLimit is established
//! declaratively through the provider's `validate_*` method with a
//! representative valid value: a `Valid` result proves that the production
//! mutation path exists and is available, while an `Invalid` result (e.g. the
//! read-only session contract) reports `Unsupported`. No mutation method is
//! ever invoked by a probe.

use orbis_capabilities::{
    ProbeClassification, ProbeContext, ProbeError, ProbeOperationResult, capability_from_operations,
};
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, OperationCapability,
};

use crate::error::ProviderError;
use crate::traits::{
    BatteryProvider, DisplayOutputProvider, FanProvider, GpuAccessProvider, GpuMuxProvider,
    GpuPowerProvider, MiniLedModeProvider, PanelOverdriveProvider, PerformanceProvider,
    ScreenAutoBrightnessProvider,
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

/// Conservative write operation for a failed read probe.
///
/// When the read contract itself is broken, write support cannot be
/// established. The canonical status is mirrored for structural failures
/// (`BackendMissing`/`Unsupported`/`TemporarilyUnavailable`/`Unknown`);
/// `PermissionDenied` on read is not evidence about writes, so write stays
/// `Unsupported` rather than inventing a denied status.
fn write_from_read_failure(read: &OperationCapability) -> OperationCapability {
    let status = match read.status {
        CapabilityStatus::BackendMissing => CapabilityStatus::BackendMissing,
        CapabilityStatus::Unsupported => CapabilityStatus::Unsupported,
        CapabilityStatus::TemporarilyUnavailable => CapabilityStatus::TemporarilyUnavailable,
        CapabilityStatus::Conflicted => CapabilityStatus::Conflicted,
        CapabilityStatus::Unknown => CapabilityStatus::Unknown,
        // PermissionDenied proves read-only access, not write denial.
        _ => CapabilityStatus::Unsupported,
    };
    OperationCapability {
        status,
        reason: Some(orbis_core::capability::CapabilityReason {
            reason: "write capability cannot be established while the read probe failed".into(),
            suggestion: String::new(),
            backend: None,
            endpoint: None,
            requirement: None,
            risk: orbis_core::capability::RiskLevel::Safe,
            checked_at: None,
        }),
    }
}

fn capability_from_read(
    read: OperationCapability,
    write: OperationCapability,
    constraints: CapabilityConstraints,
) -> Capability {
    capability_from_operations(CapabilityOperations { read, write }, constraints)
}

/// Probe Performance support from current and available profile reads.
///
/// The current profile is read only to establish that the read contract is
/// usable; it is not stored in the returned capability metadata.
///
/// Write capability comes from typed runtime evidence about the mutation
/// backend (`mutation_status`), NOT from `validate_set_profile`: validation
/// only answers whether a profile is a valid input if mutation were
/// available; it never proves the mutation path exists or is operational.
pub async fn probe_performance<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: PerformanceProvider + ?Sized,
{
    let profiles = match provider.profiles().await {
        Ok(profiles) => profiles,
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            return Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ));
        }
    };

    if profiles.is_empty() {
        let read = ProbeOperationResult::with_detail(
            ProbeClassification::Unsupported,
            "performance backend reported no profiles",
        )
        .into_operation();
        let write = write_from_read_failure(&read);
        return Ok(capability_from_read(
            read,
            write,
            CapabilityConstraints::Unknown,
        ));
    }

    if let Err(error) = provider.current_profile().await {
        let read = operation_from_error(&error, ProbeContext::EstablishedBackend)?;
        let write = write_from_read_failure(&read);
        return Ok(capability_from_read(
            read,
            write,
            CapabilityConstraints::Unknown,
        ));
    }

    let read = ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
    let write = write_from_mutation_status(mutation_status);
    Ok(capability_from_read(
        read,
        write,
        CapabilityConstraints::PerformanceProfiles(profiles),
    ))
}

/// Probe Battery charge-limit support from an authoritative read.
///
/// The returned `ChargeLimit` is used only for support/bounds metadata. Its
/// enabled/configured/effective values never enter the capability result.
///
/// Write capability comes from typed runtime evidence about the mutation
/// backend (`mutation_status`), NOT from `validate_charge_limit`: validation
/// only answers whether an input could be sent if mutation were available; it
/// never proves the mutation path exists or is currently available.
pub async fn probe_charge_limit<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: BatteryProvider + ?Sized,
{
    let charge_limit = match provider.charge_limit().await {
        Ok(charge_limit) => charge_limit,
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            return Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ));
        }
    };

    let constraints = charge_limit
        .bounds
        .map(CapabilityConstraints::ChargeLimit)
        .unwrap_or(CapabilityConstraints::Unknown);
    let read = ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
    let write = write_from_mutation_status(mutation_status);
    Ok(capability_from_read(read, write, constraints))
}

/// Derive write operation capability from typed runtime mutation-backend
/// evidence.
///
/// The evidence status is preserved exactly — `Supported`, `Unsupported`,
/// `TemporarilyUnavailable`/`BackendMissing`, `PermissionDenied` and
/// `Unknown` stay distinguishable. Validation results are never used here.
fn write_from_mutation_status(status: CapabilityStatus) -> OperationCapability {
    OperationCapability {
        status,
        reason: Some(orbis_core::capability::CapabilityReason {
            reason: "Hardware1 mutation backend runtime evidence".into(),
            suggestion: String::new(),
            backend: None,
            endpoint: None,
            requirement: None,
            risk: orbis_core::capability::RiskLevel::Safe,
            checked_at: None,
        }),
    }
}

/// Probe fan curve read capability for a specific fan.
///
/// Reads the active curve (read-only) to establish the read contract; the
/// curve points are discarded and never enter the capability metadata.
///
/// Write capability comes from typed runtime evidence about the mutation
/// backend (`mutation_status`), NOT from `validate_curve` or readable curve
/// points: those prove reads, not the mutation path. Никаких пробных writes —
/// write status определяется по runtime evidence.
pub async fn probe_fan_curve<P>(
    provider: &P,
    fan: &orbis_core::fan::FanId,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: FanProvider + ?Sized,
{
    match provider.active_curve(fan).await {
        Ok(_) => Ok(capability_from_operations(
            CapabilityOperations {
                read: ProbeOperationResult::classified(ProbeClassification::Supported)
                    .into_operation(),
                write: write_from_mutation_status(mutation_status),
            },
            CapabilityConstraints::Unknown,
        )),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            // Write не фальсифицируется: при структурном отказе read write
            // определяется по статусу read (BackendMissing/Unsupported/...),
            // даже если Hardware1 mutation backend доказан.
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ))
        }
    }
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
            Ok(capability_from_read(
                read,
                unsupported_write(),
                CapabilityConstraints::Unknown,
            ))
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
            Ok(capability_from_read(
                read,
                unsupported_write(),
                CapabilityConstraints::Unknown,
            ))
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
            Ok(capability_from_read(
                read,
                unsupported_write(),
                CapabilityConstraints::Unknown,
            ))
        }
    }
}

/// Probe Panel Overdrive read capability from an authoritative read.
///
/// The observed state is read only to confirm the read contract; the
/// `Disabled`/`Enabled`/`Unknown` value never enters the capability metadata.
///
/// Write capability comes from typed runtime evidence about the mutation
/// backend (`mutation_status`), NOT from validation: validation only answers
/// whether an input could be sent if mutation were available.
pub async fn probe_panel_overdrive<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: PanelOverdriveProvider + ?Sized,
{
    match provider.panel_overdrive_state().await {
        Ok(_) => Ok(capability_from_operations(
            CapabilityOperations {
                read: ProbeOperationResult::classified(ProbeClassification::Supported)
                    .into_operation(),
                write: write_from_mutation_status(mutation_status),
            },
            CapabilityConstraints::Unknown,
        )),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ))
        }
    }
}

/// Fixed `ReadOnly` write operation for capabilities whose mutation backend is
/// intentionally absent in this slice.
///
/// `CapabilityStatus::ReadOnly` — read exists, write absent; this is honest
/// even though the sysfs attribute is root-writable: Orbis has no production
/// MiniLED mutation backend yet.
fn read_only_write() -> OperationCapability {
    OperationCapability {
        status: CapabilityStatus::ReadOnly,
        reason: Some(orbis_core::capability::CapabilityReason {
            reason:
                "MiniLED mutation intentionally not implemented; no production mutation backend"
                    .into(),
            suggestion: String::new(),
            backend: None,
            endpoint: None,
            requirement: None,
            risk: orbis_core::capability::RiskLevel::Safe,
            checked_at: None,
        }),
    }
}

/// Probe MiniLED mode read capability from an authoritative snapshot.
///
/// The observed state is read only to confirm the read contract; the raw
/// current/allowed values never enter the capability metadata.
///
/// Write capability is fixed at `ReadOnly`: this slice has no production
/// MiniLED mutation backend, so write must never be presented as Supported
/// just because the sysfs attribute is root-writable.
pub async fn probe_mini_led_mode<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: MiniLedModeProvider + ?Sized,
{
    match provider.mini_led_mode_state().await {
        Ok(_) => Ok(capability_from_operations(
            CapabilityOperations {
                read: ProbeOperationResult::classified(ProbeClassification::Supported)
                    .into_operation(),
                write: read_only_write(),
            },
            CapabilityConstraints::Unknown,
        )),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ))
        }
    }
}

/// Probe Screen Auto Brightness read capability from an authoritative read.
///
/// The observed state is read only to confirm the read contract; the
/// `Disabled`/`Enabled`/`Unknown` value never enters the capability metadata.
///
/// Write capability is fixed at `ReadOnly`: this slice has no production
/// Screen Auto Brightness mutation backend, so write must never be presented
/// as Supported just because the sysfs attribute is root-writable.
pub async fn probe_screen_auto_brightness<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: ScreenAutoBrightnessProvider + ?Sized,
{
    match provider.screen_auto_brightness_state().await {
        Ok(_) => Ok(capability_from_operations(
            CapabilityOperations {
                read: ProbeOperationResult::classified(ProbeClassification::Supported)
                    .into_operation(),
                write: read_only_write(),
            },
            CapabilityConstraints::Unknown,
        )),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ))
        }
    }
}

/// Probe Display Output read capability from an authoritative read.
///
/// The observed state is read only to confirm the read contract; the
/// `DisplayOutputSnapshot` value never enters the capability metadata.
///
/// Write capability is fixed at `ReadOnly`: this slice has no production
/// Display Output mutation backend, so write must never be presented
/// as Supported.
pub async fn probe_display_output<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: DisplayOutputProvider + ?Sized,
{
    match provider.display_output_snapshot().await {
        Ok(_) => Ok(capability_from_operations(
            CapabilityOperations {
                read: ProbeOperationResult::classified(ProbeClassification::Supported)
                    .into_operation(),
                write: read_only_write(),
            },
            CapabilityConstraints::Unknown,
        )),
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(
                read,
                write,
                CapabilityConstraints::Unknown,
            ))
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
        Internal,
        Conflict,
    }

    impl ScriptedError {
        fn provider_error(&self) -> ProviderError {
            match self {
                Self::BackendMissing => ProviderError::BackendUnavailable("missing".into()),
                Self::Unsupported => ProviderError::Unsupported("unsupported".into()),
                Self::PermissionDenied => ProviderError::PermissionDenied("denied".into()),
                Self::Internal => ProviderError::Internal("malformed".into()),
                Self::Conflict => ProviderError::Conflict("sources disagree".into()),
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
        write_supported: bool,
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
                write_supported: false,
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
                write_supported: false,
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
                write_supported: false,
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
            if self.write_supported {
                ValidationResult::ok()
            } else {
                ValidationResult::invalid("probe provider")
            }
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
            if self.write_supported {
                ValidationResult::ok()
            } else {
                ValidationResult::invalid("probe provider")
            }
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
        // Mutation evidence = Unsupported (no proven backend).
        let capability = probe_performance(&provider, CapabilityStatus::Unsupported)
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
            let capability = probe_performance(
                &ScriptedProvider::performance(Scripted::Error(error)),
                // Even positive mutation evidence must not override a read
                // failure: write mirrors the read classification.
                CapabilityStatus::Supported,
            )
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
        let known = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Value(charge_limit(Some(bounds)))),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        assert_eq!(known.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(
            known.constraints,
            CapabilityConstraints::ChargeLimit(bounds)
        );

        let unknown = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Value(charge_limit(None))),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        assert_eq!(unknown.constraints, CapabilityConstraints::Unknown);
    }

    #[tokio::test]
    async fn battery_probe_preserves_backend_and_permission_errors() {
        let missing = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::BackendMissing)),
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            missing.operations.read.status,
            CapabilityStatus::BackendMissing
        );

        let denied = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::PermissionDenied)),
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            denied.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
    }

    #[tokio::test]
    async fn battery_source_conflict_never_promotes_support_or_write() {
        let capability = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::Conflict)),
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Conflicted
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Conflicted
        );
        assert_eq!(capability.status, CapabilityStatus::Conflicted);
    }

    #[tokio::test]
    async fn backend_presence_without_mutation_evidence_never_promotes_write() {
        let performance = probe_performance(
            &ScriptedProvider::performance(Scripted::Value(vec![PerformanceProfile::Balanced])),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        assert_eq!(
            performance.operations.write.status,
            CapabilityStatus::Unsupported
        );

        let battery = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Value(charge_limit(None))),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        assert_eq!(
            battery.operations.write.status,
            CapabilityStatus::Unsupported
        );

        let fan = probe_fan_curve(
            &ScriptedFanProvider::ok(),
            &orbis_core::fan::FanId::Cpu,
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        assert_eq!(fan.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(fan.operations.write.status, CapabilityStatus::Unsupported);
    }

    #[tokio::test]
    async fn performance_probe_reports_write_supported_when_mutation_path_proven() {
        let provider = ScriptedProvider::performance(Scripted::Value(vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ]));
        // Mutation backend evidence = Supported (Hardware1 reports a proven
        // performance mutation backend).
        let capability = probe_performance(&provider, CapabilityStatus::Supported)
            .await
            .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Supported
        );
        assert_eq!(capability.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn performance_probe_write_status_matches_runtime_evidence() {
        // Read is Supported and profiles are known in all cases; only the
        // mutation evidence varies. Each evidence class must be preserved
        // exactly — never collapsed to a bool or a generic error.
        for (evidence, expected_write) in [
            (CapabilityStatus::Supported, CapabilityStatus::Supported),
            (CapabilityStatus::Unsupported, CapabilityStatus::Unsupported),
            (
                CapabilityStatus::TemporarilyUnavailable,
                CapabilityStatus::TemporarilyUnavailable,
            ),
            (
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (
                CapabilityStatus::PermissionDenied,
                CapabilityStatus::PermissionDenied,
            ),
            (CapabilityStatus::Unknown, CapabilityStatus::Unknown),
        ] {
            let provider = ScriptedProvider::performance(Scripted::Value(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
            ]));
            let capability = probe_performance(&provider, evidence).await.unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported,
                "read must stay Supported for evidence {evidence:?}"
            );
            assert_eq!(
                capability.operations.write.status, expected_write,
                "write must mirror evidence {evidence:?}"
            );
            // Constraints (known profiles) are data, not write proof.
            assert!(matches!(
                capability.constraints,
                CapabilityConstraints::PerformanceProfiles(_)
            ));
        }
    }

    #[tokio::test]
    async fn performance_probe_validation_alone_does_not_make_write_supported() {
        // validate_set_profile accepts the profile (write_supported=true),
        // but the runtime mutation evidence says Unsupported. Validation alone
        // must never make write Supported.
        let mut provider = ScriptedProvider::performance(Scripted::Value(vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
        ]));
        provider.write_supported = true;
        let capability = probe_performance(&provider, CapabilityStatus::Unsupported)
            .await
            .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported,
            "validate_set_profile must not prove mutation availability"
        );
    }

    #[tokio::test]
    async fn charge_limit_probe_reports_write_supported_when_mutation_path_proven() {
        let bounds =
            ChargeLimitBounds::new(Percent::new(40).unwrap(), Percent::new(100).unwrap(), 1)
                .unwrap();
        let provider = ScriptedProvider::battery(Scripted::Value(charge_limit(Some(bounds))));
        // Mutation backend evidence = Supported (Hardware1 reports a proven
        // production battery mutation backend).
        let capability = probe_charge_limit(&provider, CapabilityStatus::Supported)
            .await
            .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Supported
        );
        assert_eq!(capability.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn write_operation_mirrors_backend_missing_when_read_fails() {
        // Structural read failure: write must be reported as BackendMissing,
        // not Unsupported-as-if-proven or invented PermissionDenied.
        let performance = probe_performance(
            &ScriptedProvider::performance(Scripted::Error(ScriptedError::BackendMissing)),
            // Even positive mutation evidence must not override a structural
            // read failure: write mirrors the read classification.
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            performance.operations.read.status,
            CapabilityStatus::BackendMissing
        );
        assert_eq!(
            performance.operations.write.status,
            CapabilityStatus::BackendMissing
        );

        let battery = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::BackendMissing)),
            // Even positive mutation evidence must not override a structural
            // read failure: write mirrors the read classification.
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            battery.operations.read.status,
            CapabilityStatus::BackendMissing
        );
        assert_eq!(
            battery.operations.write.status,
            CapabilityStatus::BackendMissing
        );
    }

    #[tokio::test]
    async fn write_operation_does_not_invent_permission_denied() {
        // Read PermissionDenied is evidence about reads only. Write must stay
        // Unsupported instead of claiming a denied write without evidence.
        let performance = probe_performance(
            &ScriptedProvider::performance(Scripted::Error(ScriptedError::PermissionDenied)),
            // Read PermissionDenied is evidence about reads only. Even positive
            // mutation evidence must not invent a denied write.
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            performance.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            performance.operations.write.status,
            CapabilityStatus::Unsupported
        );

        let battery = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::PermissionDenied)),
            // Read PermissionDenied is evidence about reads only. Even positive
            // mutation evidence must not invent a denied write.
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            battery.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            battery.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[tokio::test]
    async fn probes_never_call_mutation_methods() {
        // ScriptedProvider::set_profile / set_charge_limit return a distinct
        // error; the probes must not reach them even when write_supported.
        let provider =
            ScriptedProvider::performance(Scripted::Value(vec![PerformanceProfile::Silent]));
        // Mutation evidence = Supported; the probe must derive write from the
        // evidence without ever calling set_profile.
        let performance = probe_performance(&provider, CapabilityStatus::Supported)
            .await
            .unwrap();
        assert_eq!(
            performance.operations.write.status,
            CapabilityStatus::Supported
        );

        let battery = ScriptedProvider::battery(Scripted::Value(charge_limit(None)));
        // Mutation evidence = Supported; the probe must derive write from the
        // evidence without ever calling set_charge_limit.
        let charge = probe_charge_limit(&battery, CapabilityStatus::Supported)
            .await
            .unwrap();
        assert_eq!(charge.operations.write.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn probes_assemble_through_registry_builder() {
        let performance = probe_performance(
            &ScriptedProvider::performance(Scripted::Value(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
            ])),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        let battery = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Value(charge_limit(None))),
            CapabilityStatus::Unsupported,
        )
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

    /// Mock FanProvider: scripted active_curve результат.
    struct ScriptedFanProvider {
        active: Scripted<orbis_core::fan::FanCurve>,
    }

    impl ScriptedFanProvider {
        fn ok() -> Self {
            Self {
                active: Scripted::Value(orbis_core::fan::FanCurve {
                    profile: PerformanceProfile::Balanced,
                    fan: orbis_core::fan::FanId::Cpu,
                    points: vec![
                        orbis_core::fan::FanCurvePoint::new(
                            orbis_core::newtypes::TemperatureC::new(40).unwrap(),
                            orbis_core::newtypes::FanPwm::new(20).unwrap(),
                        ),
                        orbis_core::fan::FanCurvePoint::new(
                            orbis_core::newtypes::TemperatureC::new(50).unwrap(),
                            orbis_core::newtypes::FanPwm::new(40).unwrap(),
                        ),
                    ],
                }),
            }
        }

        fn backend_missing() -> Self {
            Self {
                active: Scripted::Error(ScriptedError::BackendMissing),
            }
        }
    }

    impl Provider for ScriptedFanProvider {
        fn id(&self) -> &'static str {
            "scripted-fan"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-fan")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted fan: функция '{feature}' недоступна")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl FanProvider for ScriptedFanProvider {
        async fn fan_ids(&self) -> Result<Vec<orbis_core::fan::FanId>, ProviderError> {
            Ok(vec![orbis_core::fan::FanId::Cpu])
        }

        async fn fan_rpms(
            &self,
        ) -> Result<Vec<(orbis_core::fan::FanId, orbis_core::newtypes::Rpm)>, ProviderError>
        {
            Err(ProviderError::Unsupported("no rpm".into()))
        }

        async fn fan_curve(
            &self,
            _profile: PerformanceProfile,
            _fan: &orbis_core::fan::FanId,
        ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
            Err(ProviderError::Unsupported("no profile curve".into()))
        }

        async fn fan_curve_for_profile(
            &self,
            _profile: orbis_core::profile::AsusdFanProfile,
            _fan: &orbis_core::fan::FanId,
        ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
            Err(ProviderError::Unsupported("no profile curve".into()))
        }

        async fn active_curve(
            &self,
            _fan: &orbis_core::fan::FanId,
        ) -> Result<orbis_core::fan::FanCurve, ProviderError> {
            self.active.result()
        }

        async fn set_fan_curve(
            &self,
            _curve: &orbis_core::fan::FanCurve,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }

        async fn set_curves_to_defaults(
            &self,
            _profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }

        fn curve_point_count(&self) -> usize {
            8
        }

        fn allow_decreasing(&self) -> bool {
            false
        }

        fn validate_curve(&self, _curve: &orbis_core::fan::FanCurve) -> ValidationResult {
            ValidationResult::ok()
        }
    }

    #[tokio::test]
    async fn fan_curve_probe_reports_write_unsupported_without_hardware_backend() {
        let provider = ScriptedFanProvider::ok();
        // Mutation evidence = Unsupported (no proven backend).
        let capability = probe_fan_curve(
            &provider,
            &orbis_core::fan::FanId::Cpu,
            CapabilityStatus::Unsupported,
        )
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
    }

    #[tokio::test]
    async fn fan_curve_probe_reports_write_supported_when_hardware_backend_proven() {
        let provider = ScriptedFanProvider::ok();
        // Mutation backend evidence = Supported (Hardware1 reports a proven
        // fan curve mutation backend).
        let capability = probe_fan_curve(
            &provider,
            &orbis_core::fan::FanId::Cpu,
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Supported
        );
        assert_eq!(capability.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn fan_curve_probe_write_status_matches_runtime_evidence() {
        // Read is Supported in all cases; only the mutation evidence varies.
        // Each evidence class must be preserved exactly — never collapsed to
        // a bool or a generic error.
        for (evidence, expected_write) in [
            (CapabilityStatus::Supported, CapabilityStatus::Supported),
            (CapabilityStatus::Unsupported, CapabilityStatus::Unsupported),
            (
                CapabilityStatus::TemporarilyUnavailable,
                CapabilityStatus::TemporarilyUnavailable,
            ),
            (
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (
                CapabilityStatus::PermissionDenied,
                CapabilityStatus::PermissionDenied,
            ),
            (CapabilityStatus::Unknown, CapabilityStatus::Unknown),
        ] {
            let provider = ScriptedFanProvider::ok();
            let capability = probe_fan_curve(&provider, &orbis_core::fan::FanId::Cpu, evidence)
                .await
                .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported,
                "read must stay Supported for evidence {evidence:?}"
            );
            assert_eq!(
                capability.operations.write.status, expected_write,
                "write must mirror evidence {evidence:?}"
            );
        }
    }

    #[tokio::test]
    async fn fan_curve_probe_validation_alone_does_not_make_write_supported() {
        // validate_curve accepts the curve (returns Valid), but the runtime
        // mutation evidence says Unknown. Validation alone must never make
        // write Supported.
        let provider = ScriptedFanProvider::ok();
        let capability = probe_fan_curve(
            &provider,
            &orbis_core::fan::FanId::Cpu,
            CapabilityStatus::Unknown,
        )
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unknown,
            "validate_curve must not prove mutation availability"
        );
    }

    #[tokio::test]
    async fn charge_limit_probe_reports_write_unsupported_when_mutation_path_not_proven() {
        let bounds =
            ChargeLimitBounds::new(Percent::new(40).unwrap(), Percent::new(100).unwrap(), 1)
                .unwrap();
        // validate_charge_limit would accept the value (write_supported=true),
        // but the runtime mutation evidence says Unsupported. Validation alone
        // must never make write Supported.
        let mut provider = ScriptedProvider::battery(Scripted::Value(charge_limit(Some(bounds))));
        provider.write_supported = true;
        let capability = probe_charge_limit(&provider, CapabilityStatus::Unsupported)
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
        // Overall status is Supported (read is Supported), but write is
        // Unsupported. The controller uses operations.write.status for gating.
        assert_eq!(capability.status, CapabilityStatus::Supported);
    }

    #[tokio::test]
    async fn charge_limit_probe_write_status_matches_runtime_evidence() {
        // Read is Supported in all cases; only the mutation evidence varies.
        // Each evidence class must be preserved exactly — never collapsed to
        // a bool or a generic error.
        let bounds =
            ChargeLimitBounds::new(Percent::new(40).unwrap(), Percent::new(100).unwrap(), 1)
                .unwrap();
        for (evidence, expected_write) in [
            (CapabilityStatus::Supported, CapabilityStatus::Supported),
            (CapabilityStatus::Unsupported, CapabilityStatus::Unsupported),
            (
                CapabilityStatus::TemporarilyUnavailable,
                CapabilityStatus::TemporarilyUnavailable,
            ),
            (
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (
                CapabilityStatus::PermissionDenied,
                CapabilityStatus::PermissionDenied,
            ),
            (CapabilityStatus::Unknown, CapabilityStatus::Unknown),
        ] {
            let provider = ScriptedProvider::battery(Scripted::Value(charge_limit(Some(bounds))));
            let capability = probe_charge_limit(&provider, evidence).await.unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported,
                "read must stay Supported for evidence {evidence:?}"
            );
            assert_eq!(
                capability.operations.write.status, expected_write,
                "write must mirror evidence {evidence:?}"
            );
        }
    }

    #[tokio::test]
    async fn battery_backend_missing_does_not_corrupt_performance_in_registry() {
        // Multi-domain independence: Battery BackendMissing must not prevent
        // Performance from being Supported in the same registry snapshot.
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        let performance = probe_performance(
            &ScriptedProvider::performance(Scripted::Value(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::Performance, performance)
            .unwrap();

        let battery = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::BackendMissing)),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::ChargeLimit, battery)
            .unwrap();

        let snapshot = builder.build().unwrap();
        // Performance is fully Supported.
        let perf = snapshot
            .capability(orbis_core::FeatureId::Performance)
            .unwrap();
        assert_eq!(perf.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(perf.operations.write.status, CapabilityStatus::Unsupported);
        assert_eq!(perf.status, CapabilityStatus::Supported);

        // Battery is BackendMissing — independently classified.
        let bat = snapshot
            .capability(orbis_core::FeatureId::ChargeLimit)
            .unwrap();
        assert_eq!(bat.operations.read.status, CapabilityStatus::BackendMissing);
        assert_eq!(
            bat.operations.write.status,
            CapabilityStatus::BackendMissing
        );
        assert_eq!(bat.status, CapabilityStatus::BackendMissing);
    }

    #[tokio::test]
    async fn battery_permission_denied_does_not_affect_gpu_in_registry() {
        // PermissionDenied on Battery read is evidence about reads only.
        // GPU primitives in the same registry remain independently classified.
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        let battery = probe_charge_limit(
            &ScriptedProvider::battery(Scripted::Error(ScriptedError::PermissionDenied)),
            CapabilityStatus::PermissionDenied,
        )
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::ChargeLimit, battery)
            .unwrap();

        let gpu_power = probe_gpu_power(&ScriptedProvider::gpu(
            Scripted::Value(GpuPowerState::Active),
            Scripted::Error(ScriptedError::Unsupported),
            Scripted::Error(ScriptedError::Unsupported),
        ))
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::GpuPower, gpu_power)
            .unwrap();

        let snapshot = builder.build().unwrap();
        let bat = snapshot
            .capability(orbis_core::FeatureId::ChargeLimit)
            .unwrap();
        assert_eq!(
            bat.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        // Write stays Unsupported: PermissionDenied on read does NOT invent
        // a denied write — we have no evidence about write authorization.
        assert_eq!(bat.operations.write.status, CapabilityStatus::Unsupported);

        let gpu = snapshot
            .capability(orbis_core::FeatureId::GpuPower)
            .unwrap();
        assert_eq!(gpu.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(gpu.operations.write.status, CapabilityStatus::Unsupported);
    }

    #[tokio::test]
    async fn no_optimistic_available_without_evidence() {
        // An empty registry must not claim any capability is Supported.
        let builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        let snapshot = builder.build().unwrap();
        assert!(snapshot.is_empty());
        // Unknown is the default for absent capabilities (via DeviceCapabilities).
        let device_caps = snapshot.device_capabilities();
        assert_eq!(
            device_caps.status(orbis_core::FeatureId::Performance),
            CapabilityStatus::Unknown
        );
        assert_eq!(
            device_caps.status(orbis_core::FeatureId::ChargeLimit),
            CapabilityStatus::Unknown
        );
        assert_eq!(
            device_caps.status(orbis_core::FeatureId::GpuPower),
            CapabilityStatus::Unknown
        );
    }

    #[tokio::test]
    async fn fan_curve_probe_write_mirrors_backend_missing_when_read_fails() {
        let provider = ScriptedFanProvider::backend_missing();
        // Even positive mutation evidence must not override a structural read
        // failure: write mirrors the read classification.
        let capability = probe_fan_curve(
            &provider,
            &orbis_core::fan::FanId::Cpu,
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::BackendMissing
        );
        // Write не фальсифицируется: при структурном отказе read write тоже
        // BackendMissing, даже если Hardware1 backend доказан.
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::BackendMissing
        );
    }

    /// Scripted Panel Overdrive provider.
    struct ScriptedPanelProvider {
        state: Scripted<orbis_core::display::PanelOverdriveState>,
    }

    impl ScriptedPanelProvider {
        fn new(state: Scripted<orbis_core::display::PanelOverdriveState>) -> Self {
            Self { state }
        }
    }

    impl Provider for ScriptedPanelProvider {
        fn id(&self) -> &'static str {
            "scripted-panel"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-panel")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted panel: функция '{feature}' недоступна")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl PanelOverdriveProvider for ScriptedPanelProvider {
        async fn panel_overdrive_state(
            &self,
        ) -> Result<orbis_core::display::PanelOverdriveState, ProviderError> {
            self.state.result()
        }
    }

    #[tokio::test]
    async fn panel_overdrive_probe_reports_supported_without_storing_state() {
        for state in [
            orbis_core::display::PanelOverdriveState::Disabled,
            orbis_core::display::PanelOverdriveState::Enabled,
            orbis_core::display::PanelOverdriveState::Unknown,
        ] {
            let capability = probe_panel_overdrive(
                &ScriptedPanelProvider::new(Scripted::Value(state)),
                CapabilityStatus::Unsupported,
            )
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
    async fn panel_overdrive_probe_preserves_backend_missing_and_unsupported() {
        for (error, expected) in [
            (
                ScriptedError::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (ScriptedError::Unsupported, CapabilityStatus::Unsupported),
        ] {
            let capability = probe_panel_overdrive(
                &ScriptedPanelProvider::new(Scripted::Error(error)),
                CapabilityStatus::Supported,
            )
            .await
            .unwrap();
            assert_eq!(capability.operations.read.status, expected);
            // Write mirrors the read classification on structural failure,
            // even with positive mutation evidence.
            assert_eq!(capability.operations.write.status, expected);
        }
    }

    #[tokio::test]
    async fn panel_overdrive_probe_write_status_matches_runtime_evidence() {
        for (evidence, expected_write) in [
            (CapabilityStatus::Supported, CapabilityStatus::Supported),
            (CapabilityStatus::Unsupported, CapabilityStatus::Unsupported),
            (
                CapabilityStatus::TemporarilyUnavailable,
                CapabilityStatus::TemporarilyUnavailable,
            ),
            (
                CapabilityStatus::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (
                CapabilityStatus::PermissionDenied,
                CapabilityStatus::PermissionDenied,
            ),
            (CapabilityStatus::Unknown, CapabilityStatus::Unknown),
        ] {
            let capability = probe_panel_overdrive(
                &ScriptedPanelProvider::new(Scripted::Value(
                    orbis_core::display::PanelOverdriveState::Enabled,
                )),
                evidence,
            )
            .await
            .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported,
                "read must stay Supported for evidence {evidence:?}"
            );
            assert_eq!(
                capability.operations.write.status, expected_write,
                "write must mirror evidence {evidence:?}"
            );
        }
    }

    #[tokio::test]
    async fn panel_overdrive_probe_read_permission_denied_does_not_invent_denied_write() {
        let capability = probe_panel_overdrive(
            &ScriptedPanelProvider::new(Scripted::Error(ScriptedError::PermissionDenied)),
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[tokio::test]
    async fn panel_overdrive_probe_malformed_backend_error_is_not_unsupported() {
        let capability = probe_panel_overdrive(
            &ScriptedPanelProvider::new(Scripted::Error(ScriptedError::Internal)),
            CapabilityStatus::Unsupported,
        )
        .await;
        // An unknown/malformed backend failure must surface as a probe error,
        // never as a fake Unsupported (or a fake default).
        assert!(matches!(capability, Err(ProbeError::Internal(_))));
    }

    #[tokio::test]
    async fn panel_overdrive_failure_does_not_corrupt_performance_in_registry() {
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        let performance = probe_performance(
            &ScriptedProvider::performance(Scripted::Value(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::Performance, performance)
            .unwrap();

        let panel = probe_panel_overdrive(
            &ScriptedPanelProvider::new(Scripted::Error(ScriptedError::BackendMissing)),
            CapabilityStatus::Unsupported,
        )
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::PanelOverdrive, panel)
            .unwrap();

        let snapshot = builder.build().unwrap();
        let perf = snapshot
            .capability(orbis_core::FeatureId::Performance)
            .unwrap();
        assert_eq!(perf.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(perf.status, CapabilityStatus::Supported);

        let panel = snapshot
            .capability(orbis_core::FeatureId::PanelOverdrive)
            .unwrap();
        assert_eq!(
            panel.operations.read.status,
            CapabilityStatus::BackendMissing
        );
    }

    /// Scripted MiniLED mode provider.
    struct ScriptedMiniLedProvider {
        state: Scripted<orbis_core::display::MiniLedModeState>,
    }

    impl ScriptedMiniLedProvider {
        fn new(state: Scripted<orbis_core::display::MiniLedModeState>) -> Self {
            Self { state }
        }
    }

    impl Provider for ScriptedMiniLedProvider {
        fn id(&self) -> &'static str {
            "scripted-mini-led"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-mini-led")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted mini-led: функция '{feature}' недоступна")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl MiniLedModeProvider for ScriptedMiniLedProvider {
        async fn mini_led_mode_state(
            &self,
        ) -> Result<orbis_core::display::MiniLedModeState, ProviderError> {
            self.state.result()
        }
    }

    fn mini_led_state(current: u32) -> orbis_core::display::MiniLedModeState {
        use orbis_core::display::MiniLedModeValue;
        orbis_core::display::MiniLedModeState {
            allowed: vec![MiniLedModeValue::new(0), MiniLedModeValue::new(1)],
            current: MiniLedModeValue::new(current),
            semantics: None,
        }
    }

    #[tokio::test]
    async fn mini_led_probe_reports_supported_with_readonly_write() {
        for current in [0u32, 1] {
            let capability = probe_mini_led_mode(&ScriptedMiniLedProvider::new(Scripted::Value(
                mini_led_state(current),
            )))
            .await
            .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported
            );
            // Write никогда не Supported: mutation backend отсутствует.
            assert_eq!(
                capability.operations.write.status,
                CapabilityStatus::ReadOnly
            );
            assert_eq!(capability.status, CapabilityStatus::Supported);
            assert_eq!(capability.constraints, CapabilityConstraints::Unknown);
        }
    }

    #[tokio::test]
    async fn mini_led_probe_preserves_backend_missing_and_unsupported() {
        for (error, expected) in [
            (
                ScriptedError::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (ScriptedError::Unsupported, CapabilityStatus::Unsupported),
        ] {
            let capability =
                probe_mini_led_mode(&ScriptedMiniLedProvider::new(Scripted::Error(error)))
                    .await
                    .unwrap();
            assert_eq!(capability.operations.read.status, expected);
            // Write зеркалит read при структурном отказе (не фальсифицируется).
            assert_eq!(capability.operations.write.status, expected);
        }
    }

    #[tokio::test]
    async fn mini_led_probe_read_permission_denied_does_not_invent_denied_write() {
        let capability = probe_mini_led_mode(&ScriptedMiniLedProvider::new(Scripted::Error(
            ScriptedError::PermissionDenied,
        )))
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[tokio::test]
    async fn mini_led_probe_malformed_backend_error_is_not_unsupported() {
        let capability = probe_mini_led_mode(&ScriptedMiniLedProvider::new(Scripted::Error(
            ScriptedError::Internal,
        )))
        .await;
        assert!(matches!(capability, Err(ProbeError::Internal(_))));
    }

    #[tokio::test]
    async fn mini_led_failure_does_not_corrupt_panel_overdrive_in_registry() {
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        let panel = probe_panel_overdrive(
            &ScriptedPanelProvider::new(Scripted::Value(
                orbis_core::display::PanelOverdriveState::Enabled,
            )),
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::PanelOverdrive, panel)
            .unwrap();

        let mini_led = probe_mini_led_mode(&ScriptedMiniLedProvider::new(Scripted::Error(
            ScriptedError::BackendMissing,
        )))
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::MiniLed, mini_led)
            .unwrap();

        let snapshot = builder.build().unwrap();
        let panel = snapshot
            .capability(orbis_core::FeatureId::PanelOverdrive)
            .unwrap();
        assert_eq!(panel.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(panel.status, CapabilityStatus::Supported);

        let mini_led = snapshot.capability(orbis_core::FeatureId::MiniLed).unwrap();
        assert_eq!(
            mini_led.operations.read.status,
            CapabilityStatus::BackendMissing
        );
    }

    /// Scripted Screen Auto Brightness provider.
    struct ScriptedSabProvider {
        state: Scripted<orbis_core::display::ScreenAutoBrightnessState>,
    }

    impl ScriptedSabProvider {
        fn new(state: Scripted<orbis_core::display::ScreenAutoBrightnessState>) -> Self {
            Self { state }
        }
    }

    impl Provider for ScriptedSabProvider {
        fn id(&self) -> &'static str {
            "scripted-sab"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-sab")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted sab: функция '{feature}' недоступна")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl ScreenAutoBrightnessProvider for ScriptedSabProvider {
        async fn screen_auto_brightness_state(
            &self,
        ) -> Result<orbis_core::display::ScreenAutoBrightnessState, ProviderError> {
            self.state.result()
        }
    }

    #[tokio::test]
    async fn sab_probe_reports_supported_with_readonly_write() {
        for state in [
            orbis_core::display::ScreenAutoBrightnessState::Disabled,
            orbis_core::display::ScreenAutoBrightnessState::Enabled,
            orbis_core::display::ScreenAutoBrightnessState::Unknown,
        ] {
            let capability =
                probe_screen_auto_brightness(&ScriptedSabProvider::new(Scripted::Value(state)))
                    .await
                    .unwrap();
            assert_eq!(
                capability.operations.read.status,
                CapabilityStatus::Supported
            );
            // Write никогда не Supported: mutation backend отсутствует.
            assert_eq!(
                capability.operations.write.status,
                CapabilityStatus::ReadOnly
            );
            assert_eq!(capability.status, CapabilityStatus::Supported);
            assert_eq!(capability.constraints, CapabilityConstraints::Unknown);
        }
    }

    #[tokio::test]
    async fn sab_probe_preserves_backend_missing_and_unsupported() {
        for (error, expected) in [
            (
                ScriptedError::BackendMissing,
                CapabilityStatus::BackendMissing,
            ),
            (ScriptedError::Unsupported, CapabilityStatus::Unsupported),
        ] {
            let capability =
                probe_screen_auto_brightness(&ScriptedSabProvider::new(Scripted::Error(error)))
                    .await
                    .unwrap();
            assert_eq!(capability.operations.read.status, expected);
            // Write зеркалит read при структурном отказе (не фальсифицируется).
            assert_eq!(capability.operations.write.status, expected);
        }
    }

    #[tokio::test]
    async fn sab_probe_read_permission_denied_does_not_make_write_denied() {
        // Read PermissionDenied — evidence только о read. Write остаётся
        // ReadOnly (intentionally), а не PermissionDenied.
        let capability = probe_screen_auto_brightness(&ScriptedSabProvider::new(Scripted::Error(
            ScriptedError::PermissionDenied,
        )))
        .await
        .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[tokio::test]
    async fn sab_probe_malformed_backend_error_is_not_unsupported() {
        let capability = probe_screen_auto_brightness(&ScriptedSabProvider::new(Scripted::Error(
            ScriptedError::Internal,
        )))
        .await;
        assert!(matches!(capability, Err(ProbeError::Internal(_))));
    }

    #[tokio::test]
    async fn sab_failure_does_not_corrupt_mini_led_in_registry() {
        let mut builder = orbis_capabilities::CapabilityRegistryBuilder::new(
            1,
            std::time::SystemTime::UNIX_EPOCH,
        );
        let mini_led = probe_mini_led_mode(&ScriptedMiniLedProvider::new(Scripted::Value(
            mini_led_state(1),
        )))
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::MiniLed, mini_led)
            .unwrap();

        let sab = probe_screen_auto_brightness(&ScriptedSabProvider::new(Scripted::Error(
            ScriptedError::BackendMissing,
        )))
        .await
        .unwrap();
        builder
            .add(orbis_core::FeatureId::ScreenAutoBrightness, sab)
            .unwrap();

        let snapshot = builder.build().unwrap();
        let mini_led = snapshot.capability(orbis_core::FeatureId::MiniLed).unwrap();
        assert_eq!(mini_led.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(mini_led.status, CapabilityStatus::Supported);

        let sab = snapshot
            .capability(orbis_core::FeatureId::ScreenAutoBrightness)
            .unwrap();
        assert_eq!(sab.operations.read.status, CapabilityStatus::BackendMissing);
    }
}
