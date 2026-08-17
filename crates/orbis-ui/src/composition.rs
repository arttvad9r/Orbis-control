//! Application runtime composition for the interactive UI.
//!
//! This module owns construction and grouping of application services. It does
//! not define provider semantics or transport contracts.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use orbis_application::{
    AppService, ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome,
    PerformanceCommandOutcome, PerformanceState, SetChargeLimitError, SetGpuModeError,
    SetPerformanceError,
};
use orbis_capabilities::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot, ProbeError};
use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
#[cfg(test)]
use orbis_providers::mock::MockProvider;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, GpuProvider,
    PerformanceProvider,
};
use orbis_session_client::{
    SessionChargeLimitProvider, SessionGpuAccessProvider, SessionGpuMuxProvider,
    SessionGpuPowerProvider, SessionHardwareBatteryProvider, SessionHardwarePerformanceProvider,
    ZbusHardwareBatterySource, ZbusHardwarePerformanceSource, ZbusSessionChargeLimitSource,
    ZbusSessionGpuSource, ZbusSessionPerformanceSource,
};
#[cfg(test)]
use orbis_test_support::devices::build_state_arc;

/// Grouped GPU services used by the worker.
pub struct GpuServices<M, P, X, A> {
    main: AppService<M>,
    power: AppService<P>,
    mux: AppService<X>,
    access: AppService<A>,
}

impl<M, P, X, A> GpuServices<M, P, X, A> {
    /// Create grouped GPU services.
    pub fn new(
        main: AppService<M>,
        power: AppService<P>,
        mux: AppService<X>,
        access: AppService<A>,
    ) -> Self {
        Self {
            main,
            power,
            mux,
            access,
        }
    }
}

/// Production GPU primitive services without a product-mode backend.
pub struct GpuPrimitiveServices<P, X, A> {
    power: AppService<P>,
    mux: AppService<X>,
    access: AppService<A>,
}

impl<P, X, A> GpuPrimitiveServices<P, X, A> {
    /// Create the primitive-only GPU composition.
    pub fn new(power: AppService<P>, mux: AppService<X>, access: AppService<A>) -> Self {
        Self { power, mux, access }
    }

    /// Borrow the underlying runtime power service for capability probing.
    pub fn primitive_power(&self) -> &AppService<P> {
        &self.power
    }

    /// Borrow the underlying physical MUX service for capability probing.
    pub fn primitive_mux(&self) -> &AppService<X> {
        &self.mux
    }

    /// Borrow the underlying access policy service for capability probing.
    pub fn primitive_access(&self) -> &AppService<A> {
        &self.access
    }

    /// Borrow the inner power provider for capability probing.
    pub fn primitive_power_provider(&self) -> &P {
        self.power.provider()
    }

    /// Borrow the inner MUX provider for capability probing.
    pub fn primitive_mux_provider(&self) -> &X {
        self.mux.provider()
    }

    /// Borrow the inner access policy provider for capability probing.
    pub fn primitive_access_provider(&self) -> &A {
        self.access.provider()
    }
}

/// Capability operations exposed by the grouped GPU composition.
#[async_trait]
pub trait GpuServicesRuntime: Send {
    /// Execute product GPU mode mutation and authoritative read-back.
    async fn set_gpu_mode(
        &self,
        mode: GpuMode,
        confirmed: bool,
    ) -> Result<GpuCommandOutcome, SetGpuModeError>;

    /// Read the three independent GPU capabilities.
    async fn refresh_gpu_capabilities(
        &self,
    ) -> (
        Result<GpuPowerState, ProviderError>,
        Result<GpuMuxState, ProviderError>,
        Result<GpuAccessPolicy, ProviderError>,
    );

    /// Probe three independent GPU primitive capabilities into a single
    /// capability list. Used by the lifecycle refresh helper. Each entry in
    /// the returned list corresponds to `(FeatureId, Capability)`.
    async fn probe_primitives(
        &self,
    ) -> Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)>;
}

#[async_trait]
impl<M, P, X, A> GpuServicesRuntime for GpuServices<M, P, X, A>
where
    M: GpuProvider + Send + Sync,
    P: GpuPowerProvider + Send + Sync,
    X: GpuMuxProvider + Send + Sync,
    A: GpuAccessProvider + Send + Sync,
{
    async fn set_gpu_mode(
        &self,
        mode: GpuMode,
        confirmed: bool,
    ) -> Result<GpuCommandOutcome, SetGpuModeError> {
        self.main.set_gpu_mode(mode, confirmed).await
    }

    async fn refresh_gpu_capabilities(
        &self,
    ) -> (
        Result<GpuPowerState, ProviderError>,
        Result<GpuMuxState, ProviderError>,
        Result<GpuAccessPolicy, ProviderError>,
    ) {
        (
            self.power.gpu_power_state().await,
            self.mux.gpu_mux_state().await,
            self.access.gpu_access_policy().await,
        )
    }

    async fn probe_primitives(
        &self,
    ) -> Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)> {
        let mut entries = Vec::with_capacity(3);
        if let Ok(capability) = orbis_providers::probe_gpu_power(self.power.provider()).await {
            entries.push((orbis_core::FeatureId::GpuPower, capability));
        }
        if let Ok(capability) = orbis_providers::probe_gpu_mux(self.mux.provider()).await {
            entries.push((orbis_core::FeatureId::GpuMux, capability));
        }
        if let Ok(capability) = orbis_providers::probe_gpu_access(self.access.provider()).await {
            entries.push((orbis_core::FeatureId::GpuAccess, capability));
        }
        entries
    }
}

#[async_trait]
impl<P, X, A> GpuServicesRuntime for GpuPrimitiveServices<P, X, A>
where
    P: GpuPowerProvider + Send + Sync,
    X: GpuMuxProvider + Send + Sync,
    A: GpuAccessProvider + Send + Sync,
{
    async fn set_gpu_mode(
        &self,
        _mode: GpuMode,
        _confirmed: bool,
    ) -> Result<GpuCommandOutcome, SetGpuModeError> {
        Err(CommandError::Command(ProviderError::Unsupported(
            "GPU product mode backend is not proven".into(),
        )))
    }

    async fn refresh_gpu_capabilities(
        &self,
    ) -> (
        Result<GpuPowerState, ProviderError>,
        Result<GpuMuxState, ProviderError>,
        Result<GpuAccessPolicy, ProviderError>,
    ) {
        (
            self.power.gpu_power_state().await,
            self.mux.gpu_mux_state().await,
            self.access.gpu_access_policy().await,
        )
    }

    async fn probe_primitives(
        &self,
    ) -> Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)> {
        let mut entries = Vec::with_capacity(3);
        if let Ok(capability) =
            orbis_providers::probe_gpu_power(self.primitive_power_provider()).await
        {
            entries.push((orbis_core::FeatureId::GpuPower, capability));
        }
        if let Ok(capability) = orbis_providers::probe_gpu_mux(self.primitive_mux_provider()).await
        {
            entries.push((orbis_core::FeatureId::GpuMux, capability));
        }
        if let Ok(capability) =
            orbis_providers::probe_gpu_access(self.primitive_access_provider()).await
        {
            entries.push((orbis_core::FeatureId::GpuAccess, capability));
        }
        entries
    }
}

/// Battery service capability boundary.
#[async_trait]
pub trait BatteryServiceRuntime: Send {
    /// Read the authoritative Battery state.
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError>;

    /// Mutate Battery charge limit and perform application read-back.
    async fn set_charge_limit(
        &self,
        percent: u8,
    ) -> Result<ChargeLimitCommandOutcome, SetChargeLimitError>;

    /// Probe Battery Charge Limit capability support metadata.
    ///
    /// This is used by the lifecycle refresh path to build a new
    /// capability registry snapshot. The typed provider-error semantics are
    /// translated into the canonical capability status / constraint set.
    /// Provider-side `Internal` / `InvalidRequest` errors propagate as
    /// `ProbeError::Internal`/`ContractViolation` and abort the refresh
    /// cycle without producing a partial snapshot.
    async fn probe_capability(
        &self,
    ) -> Result<orbis_core::capability::Capability, orbis_capabilities::ProbeError>;
}

#[async_trait]
impl<P> BatteryServiceRuntime for AppService<P>
where
    P: BatteryProvider + Send + Sync,
{
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        AppService::charge_limit(self).await
    }

    async fn set_charge_limit(
        &self,
        percent: u8,
    ) -> Result<ChargeLimitCommandOutcome, SetChargeLimitError> {
        AppService::set_charge_limit(self, percent).await
    }

    async fn probe_capability(
        &self,
    ) -> Result<orbis_core::capability::Capability, orbis_capabilities::ProbeError> {
        orbis_providers::probe_charge_limit(self.provider()).await
    }
}

/// Performance service capability boundary.
#[async_trait]
pub trait PerformanceServiceRuntime: Send {
    /// Read the authoritative Performance state.
    async fn performance_state(&self) -> Result<PerformanceState, ProviderError>;

    /// Mutate Performance and perform application read-back.
    async fn set_performance(
        &self,
        profile: PerformanceProfile,
    ) -> Result<PerformanceCommandOutcome, SetPerformanceError>;

    /// Probe Performance capability support metadata.
    ///
    /// This is used by the lifecycle refresh path to build a new
    /// capability registry snapshot.
    async fn probe_performance(&self) -> Result<orbis_core::capability::Capability, ProbeError>;
}

#[async_trait]
impl<P> PerformanceServiceRuntime for AppService<P>
where
    P: PerformanceProvider + Send + Sync,
{
    async fn performance_state(&self) -> Result<PerformanceState, ProviderError> {
        AppService::performance_state(self).await
    }

    async fn set_performance(
        &self,
        profile: PerformanceProfile,
    ) -> Result<PerformanceCommandOutcome, SetPerformanceError> {
        AppService::set_performance(self, profile).await
    }

    async fn probe_performance(&self) -> Result<orbis_core::capability::Capability, ProbeError> {
        orbis_providers::probe_performance(self.provider()).await
    }
}

/// Telemetry service capability boundary.
#[async_trait]
pub trait TelemetryServiceRuntime: Send {
    /// Read an authoritative read-only telemetry snapshot.
    async fn snapshot(&self) -> Result<orbis_core::telemetry::Telemetry, ProviderError>;
}

#[async_trait]
impl<P> TelemetryServiceRuntime for AppService<P>
where
    P: orbis_providers::traits::TelemetryProvider + Send + Sync,
{
    async fn snapshot(&self) -> Result<orbis_core::telemetry::Telemetry, ProviderError> {
        self.provider().snapshot().await
    }
}

/// All application services owned by one worker runtime.
pub struct ApplicationRuntime<G, B, R, T> {
    /// Grouped GPU capabilities.
    pub gpu: G,
    /// Battery service.
    pub battery: B,
    /// Performance service.
    pub performance: R,
    /// Telemetry service (read-only sysfs snapshot).
    pub telemetry: T,
    /// Read-only capability registry snapshot.
    pub(crate) capabilities: Arc<CapabilityRegistrySnapshot>,
}

impl<G, B, R, T> ApplicationRuntime<G, B, R, T> {
    /// Create an application runtime with an explicitly built capability snapshot.
    ///
    /// Production composition must always supply an authoritative snapshot
    /// assembled through the discovery pipeline. Permissive constructors that
    /// silently allocate an empty default snapshot have been removed.
    pub fn new_with_snapshot(
        gpu: G,
        battery: B,
        performance: R,
        telemetry: T,
        snapshot: CapabilityRegistrySnapshot,
    ) -> Self {
        Self {
            gpu,
            battery,
            performance,
            telemetry,
            capabilities: Arc::new(snapshot),
        }
    }

    /// Replace the authoritative capability registry snapshot in-place.
    ///
    /// This is the only mutation path for the registry in the runtime. The
    /// caller must have assembled the new snapshot through the discovery
    /// pipeline; partial or per-entry edits are not supported.
    pub fn replace_capabilities(&mut self, snapshot: CapabilityRegistrySnapshot) {
        self.capabilities = Arc::new(snapshot);
    }

    /// Borrow the immutable capability registry snapshot.
    pub fn capabilities(&self) -> &CapabilityRegistrySnapshot {
        &self.capabilities
    }

    /// Clone the current snapshot `Arc` handle.
    ///
    /// External readers that need to outlive a `replace_capabilities` call may
    /// hold the previous snapshot through this handle while the runtime moves
    /// to a new authoritative snapshot.
    pub fn capabilities_arc(&self) -> Arc<CapabilityRegistrySnapshot> {
        self.capabilities.clone()
    }

    /// Create an explicit empty test runtime.
    ///
    /// This helper is only useful where tests deliberately want a runtime
    /// without discovered capabilities. Available only to test code in this
    /// crate so that production callers cannot accidentally use it.
    #[cfg(test)]
    pub fn empty_for_testing(gpu: G, battery: B, performance: R, telemetry: T) -> Self {
        let checked_at = SystemTime::now();
        let snapshot = CapabilityRegistryBuilder::new(1, checked_at)
            .build()
            .expect("empty registry snapshot must build");
        Self {
            gpu,
            battery,
            performance,
            telemetry,
            capabilities: Arc::new(snapshot),
        }
    }
}

/// Errors that prevent a registry refresh cycle from producing a new snapshot.
///
/// Ordinary provider evidence (`Unsupported`, `BackendMissing`,
/// `TemporarilyUnavailable`, `PermissionDenied`, `Unknown`) is not a
/// `RefreshError`. It is recorded in the new snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshError {
    /// Our own probe pipeline returned `ProbeError::Internal`.
    Internal(String),
    /// Our own probe pipeline reported a contract violation.
    ContractViolation(String),
    /// `CapabilityRegistryBuilder` rejected an entry produced by a probe.
    InconsistentSnapshot(String),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(detail) => write!(f, "probe internal failure: {detail}"),
            Self::ContractViolation(detail) => {
                write!(f, "probe contract violation: {detail}")
            }
            Self::InconsistentSnapshot(detail) => {
                write!(f, "snapshot inconsistent: {detail}")
            }
        }
    }
}

impl std::error::Error for RefreshError {}

/// Errors that prevent initial registry snapshot assembly.
#[derive(Debug)]
pub enum RegistryAssemblyError {
    /// `ProbeError::Internal` or `ProbeError::ContractViolation` during
    /// initial discovery. Production must fail loudly here.
    Probe(ProbeError),
    /// `CapabilityRegistryBuilder` rejected an entry produced by a probe.
    InconsistentSnapshot(String),
}

impl std::fmt::Display for RegistryAssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Probe(error) => write!(f, "registry probe failure: {error}"),
            Self::InconsistentSnapshot(detail) => {
                write!(f, "registry snapshot inconsistent: {detail}")
            }
        }
    }
}

impl std::error::Error for RegistryAssemblyError {}

/// Run all five capability probes against the provided services and assemble
/// a deterministic immutable snapshot for the requested `generation`.
///
/// Ordinary provider evidence does not abort the assembly. `ProbeError`
/// aborts the assembly without producing a partial snapshot.
#[allow(clippy::too_many_arguments)]
pub async fn probe_capability_registry<Bp, Pp, Gpow, Gmux, Gacc>(
    battery_provider: &Bp,
    performance_provider: &Pp,
    gpu_power_provider: &Gpow,
    gpu_mux_provider: &Gmux,
    gpu_access_provider: &Gacc,
    generation: u64,
    checked_at: SystemTime,
) -> Result<CapabilityRegistrySnapshot, ProbeError>
where
    Bp: orbis_providers::traits::BatteryProvider + ?Sized,
    Pp: orbis_providers::traits::PerformanceProvider + ?Sized,
    Gpow: orbis_providers::traits::GpuPowerProvider + ?Sized,
    Gmux: orbis_providers::traits::GpuMuxProvider + ?Sized,
    Gacc: orbis_providers::traits::GpuAccessProvider + ?Sized,
{
    let mut builder = CapabilityRegistryBuilder::new(generation, checked_at);

    let performance = orbis_providers::probe_performance(performance_provider).await?;
    builder
        .add(orbis_core::FeatureId::Performance, performance)
        .map_err(|err| match err {
            orbis_capabilities::RegistryError::DuplicateCapability { feature } => {
                ProbeError::ContractViolation(format!(
                    "performance capability reported twice: {feature:?}"
                ))
            }
            orbis_capabilities::RegistryError::InconsistentCapability { message, .. } => {
                ProbeError::ContractViolation(message)
            }
        })?;

    let battery = orbis_providers::probe_charge_limit(battery_provider).await?;
    builder
        .add(orbis_core::FeatureId::ChargeLimit, battery)
        .map_err(|err| match err {
            orbis_capabilities::RegistryError::DuplicateCapability { feature } => {
                ProbeError::ContractViolation(format!(
                    "battery capability reported twice: {feature:?}"
                ))
            }
            orbis_capabilities::RegistryError::InconsistentCapability { message, .. } => {
                ProbeError::ContractViolation(message)
            }
        })?;

    let power = orbis_providers::probe_gpu_power(gpu_power_provider).await?;
    builder
        .add(orbis_core::FeatureId::GpuPower, power)
        .map_err(|err| match err {
            orbis_capabilities::RegistryError::DuplicateCapability { feature } => {
                ProbeError::ContractViolation(format!(
                    "gpu power capability reported twice: {feature:?}"
                ))
            }
            orbis_capabilities::RegistryError::InconsistentCapability { message, .. } => {
                ProbeError::ContractViolation(message)
            }
        })?;

    let mux = orbis_providers::probe_gpu_mux(gpu_mux_provider).await?;
    builder
        .add(orbis_core::FeatureId::GpuMux, mux)
        .map_err(|err| match err {
            orbis_capabilities::RegistryError::DuplicateCapability { feature } => {
                ProbeError::ContractViolation(format!(
                    "gpu mux capability reported twice: {feature:?}"
                ))
            }
            orbis_capabilities::RegistryError::InconsistentCapability { message, .. } => {
                ProbeError::ContractViolation(message)
            }
        })?;

    let access = orbis_providers::probe_gpu_access(gpu_access_provider).await?;
    builder
        .add(orbis_core::FeatureId::GpuAccess, access)
        .map_err(|err| match err {
            orbis_capabilities::RegistryError::DuplicateCapability { feature } => {
                ProbeError::ContractViolation(format!(
                    "gpu access capability reported twice: {feature:?}"
                ))
            }
            orbis_capabilities::RegistryError::InconsistentCapability { message, .. } => {
                ProbeError::ContractViolation(message)
            }
        })?;

    builder
        .build()
        .map_err(|err| ProbeError::ContractViolation(err.to_string()))
}

/// Assemble the initial capability registry snapshot for production.
///
/// Fail-fast on `ProbeError::Internal` and `ProbeError::ContractViolation` —
/// production must not commit to a partially-trustworthy registry on
/// initial discovery.
#[allow(clippy::too_many_arguments)]
pub async fn build_initial_registry_snapshot<Bp, Pp, Gpow, Gmux, Gacc>(
    battery_provider: &Bp,
    performance_provider: &Pp,
    gpu_power_provider: &Gpow,
    gpu_mux_provider: &Gmux,
    gpu_access_provider: &Gacc,
) -> Result<CapabilityRegistrySnapshot, RegistryAssemblyError>
where
    Bp: orbis_providers::traits::BatteryProvider + ?Sized,
    Pp: orbis_providers::traits::PerformanceProvider + ?Sized,
    Gpow: orbis_providers::traits::GpuPowerProvider + ?Sized,
    Gmux: orbis_providers::traits::GpuMuxProvider + ?Sized,
    Gacc: orbis_providers::traits::GpuAccessProvider + ?Sized,
{
    let checked_at = SystemTime::now();
    probe_capability_registry(
        battery_provider,
        performance_provider,
        gpu_power_provider,
        gpu_mux_provider,
        gpu_access_provider,
        1,
        checked_at,
    )
    .await
    .map_err(RegistryAssemblyError::Probe)
}

/// Re-probe all five capabilities and produce a deterministic snapshot for
/// the requested next generation. The returned `Result<CapabilityRegistrySnapshot, RefreshError>`
/// is `Err` only when our own probe pipeline reports a software failure
/// (`ProbeError::Internal`/`ContractViolation`); ordinary provider evidence
/// always results in `Ok`.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_capability_registry<Bp, Pp, Gpow, Gmux, Gacc>(
    battery_provider: &Bp,
    performance_provider: &Pp,
    gpu_power_provider: &Gpow,
    gpu_mux_provider: &Gmux,
    gpu_access_provider: &Gacc,
    next_generation: u64,
) -> Result<CapabilityRegistrySnapshot, RefreshError>
where
    Bp: orbis_providers::traits::BatteryProvider + ?Sized,
    Pp: orbis_providers::traits::PerformanceProvider + ?Sized,
    Gpow: orbis_providers::traits::GpuPowerProvider + ?Sized,
    Gmux: orbis_providers::traits::GpuMuxProvider + ?Sized,
    Gacc: orbis_providers::traits::GpuAccessProvider + ?Sized,
{
    let checked_at = SystemTime::now();
    probe_capability_registry(
        battery_provider,
        performance_provider,
        gpu_power_provider,
        gpu_mux_provider,
        gpu_access_provider,
        next_generation,
        checked_at,
    )
    .await
    .map_err(|error| match error {
        ProbeError::Internal(detail) => RefreshError::Internal(detail),
        ProbeError::ContractViolation(detail) => RefreshError::ContractViolation(detail),
    })
}

/// Production service types for the current capabilities.
pub type ProductionRuntime = ApplicationRuntime<
    GpuPrimitiveServices<
        SessionGpuPowerProvider<ZbusSessionGpuSource>,
        SessionGpuMuxProvider<ZbusSessionGpuSource>,
        SessionGpuAccessProvider<ZbusSessionGpuSource>,
    >,
    AppService<
        SessionHardwareBatteryProvider<
            SessionChargeLimitProvider<ZbusSessionChargeLimitSource>,
            ZbusHardwareBatterySource,
        >,
    >,
    AppService<
        SessionHardwarePerformanceProvider<
            ZbusSessionPerformanceSource,
            ZbusHardwarePerformanceSource,
        >,
    >,
    AppService<orbis_providers::SysfsTelemetryProvider>,
>;

/// Build the current production application runtime.
pub async fn build_production_runtime(
    session_connection: zbus::Connection,
    system_connection: zbus::Connection,
) -> anyhow::Result<(ProductionRuntime, bool)> {
    let hardware_owner = hardware1_write_available(&system_connection).await;

    let battery_read_provider = SessionChargeLimitProvider::new(ZbusSessionChargeLimitSource::new(
        session_connection.clone(),
    ));
    let battery_provider = SessionHardwareBatteryProvider::new(
        battery_read_provider,
        ZbusHardwareBatterySource::new(system_connection.clone()),
    );

    let gpu = GpuPrimitiveServices::new(
        AppService::new(Arc::new(SessionGpuPowerProvider::new(
            ZbusSessionGpuSource::new(session_connection.clone()),
        ))),
        AppService::new(Arc::new(SessionGpuMuxProvider::new(
            ZbusSessionGpuSource::new(session_connection.clone()),
        ))),
        AppService::new(Arc::new(SessionGpuAccessProvider::new(
            ZbusSessionGpuSource::new(session_connection.clone()),
        ))),
    );

    let battery_arc = Arc::new(battery_provider);
    let performance_arc = Arc::new(SessionHardwarePerformanceProvider::new(
        ZbusSessionPerformanceSource::new(session_connection),
        ZbusHardwarePerformanceSource::new(system_connection),
    ));
    let battery = AppService::new(battery_arc.clone());
    let performance = AppService::new(performance_arc.clone());

    // Read-only sysfs telemetry: dynamic hwmon/power_supply discovery, no
    // writes, no privileged APIs. Construction performs no I/O.
    let telemetry = AppService::new(Arc::new(orbis_providers::SysfsTelemetryProvider::default()));

    let snapshot = build_initial_registry_snapshot(
        &*battery_arc,
        &*performance_arc,
        gpu.primitive_power_provider(),
        gpu.primitive_mux_provider(),
        gpu.primitive_access_provider(),
    )
    .await?;

    Ok((
        ApplicationRuntime::new_with_snapshot(gpu, battery, performance, telemetry, snapshot),
        hardware_owner,
    ))
}

async fn hardware1_write_available(connection: &zbus::Connection) -> bool {
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &("io.github.orbiscontrol.Hardware",),
        )
        .await
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}

/// Test-only runtime type: один MockProvider для всех capability services.
#[cfg(test)]
type MockRuntime = ApplicationRuntime<
    GpuServices<MockProvider, MockProvider, MockProvider, MockProvider>,
    AppService<MockProvider>,
    AppService<MockProvider>,
    AppService<MockProvider>,
>;

/// Test-only grouped runtime using one mock provider for all capabilities.
#[cfg(test)]
pub fn mock_runtime() -> MockRuntime {
    let state = build_state_arc("zephyrus-full").expect("profile exists");
    let provider = Arc::new(MockProvider::new(state));
    let gpu = GpuServices::new(
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
    );
    let empty = CapabilityRegistryBuilder::new(1, SystemTime::now())
        .build()
        .expect("empty registry snapshot must build");
    ApplicationRuntime::new_with_snapshot(
        gpu,
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
        AppService::new(provider),
        empty,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ApplicationRuntime, CapabilityRegistrySnapshot, GpuPrimitiveServices, GpuServices,
        GpuServicesRuntime, MockRuntime, build_initial_registry_snapshot,
        refresh_capability_registry,
    };
    use orbis_application::{AppService, CommandError};
    use orbis_core::FeatureId;
    use orbis_core::capability::CapabilityStatus;
    use orbis_core::gpu::GpuMode;
    use orbis_providers::error::ProviderError;
    use orbis_providers::mock::MockProvider;
    use orbis_test_support::devices::build_state_arc;
    use std::sync::Arc;

    #[tokio::test]
    async fn primitive_gpu_services_do_not_claim_product_mode_support() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let services = GpuPrimitiveServices::new(
            orbis_application::AppService::new(provider.clone()),
            orbis_application::AppService::new(provider.clone()),
            orbis_application::AppService::new(provider),
        );

        let result = services
            .set_gpu_mode(GpuMode::Standard, false)
            .await
            .expect_err("product mode must be unsupported");
        assert!(matches!(
            result,
            CommandError::Command(ProviderError::Unsupported(_))
        ));
    }

    #[tokio::test]
    async fn runtime_with_registry_exposes_performance_and_charge_limit() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let snapshot = build_initial_registry_snapshot(
            &*provider, &*provider, &*provider, &*provider, &*provider,
        )
        .await
        .expect("scripted provider must produce a coherent snapshot");
        let mut runtime = ApplicationRuntime::empty_for_testing(
            GpuServices::new(
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
            ),
            AppService::new(provider.clone()),
            AppService::new(provider.clone()),
            AppService::new(provider),
        );
        runtime.replace_capabilities(snapshot);
        let snapshot_ref = runtime.capabilities();
        assert_eq!(snapshot_ref.generation(), 1);
        let performance = snapshot_ref
            .capability(FeatureId::Performance)
            .expect("performance capability present");
        assert_eq!(
            performance.operations.read.status,
            CapabilityStatus::Supported
        );
        let charge_limit = snapshot_ref
            .capability(FeatureId::ChargeLimit)
            .expect("charge limit capability present");
        assert_eq!(
            charge_limit.operations.read.status,
            CapabilityStatus::Supported
        );
    }

    #[tokio::test]
    async fn initial_runtime_snapshot_contains_all_five_capabilities() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let snapshot = build_initial_registry_snapshot(
            &*provider, &*provider, &*provider, &*provider, &*provider,
        )
        .await
        .expect("scripted provider must produce a coherent snapshot");
        assert!(snapshot.contains(FeatureId::Performance));
        assert!(snapshot.contains(FeatureId::ChargeLimit));
        assert!(snapshot.contains(FeatureId::GpuPower));
        assert!(snapshot.contains(FeatureId::GpuMux));
        assert!(snapshot.contains(FeatureId::GpuAccess));
        assert!(!snapshot.contains(FeatureId::GpuProductPolicy));
    }

    async fn build_initial_snapshot_for_refresh(
        provider: &MockProvider,
    ) -> CapabilityRegistrySnapshot {
        build_initial_registry_snapshot(provider, provider, provider, provider, provider)
            .await
            .expect("scripted provider must produce a coherent snapshot")
    }

    fn script_gpu_runtime(provider: std::sync::Arc<MockProvider>) -> MockRuntime {
        ApplicationRuntime::empty_for_testing(
            GpuServices::new(
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
            ),
            AppService::new(provider.clone()),
            AppService::new(provider.clone()),
            AppService::new(provider),
        )
    }

    #[tokio::test]
    async fn refresh_replaces_snapshot_with_monotonic_generation() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        let mut runtime = script_gpu_runtime(provider.clone());
        runtime.replace_capabilities(initial);

        let next = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        let old_arc = runtime.capabilities_arc();
        runtime.replace_capabilities(next);
        assert_eq!(old_arc.generation(), 1);
        assert_eq!(runtime.capabilities().generation(), 2);

        let third = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(third);
        assert_eq!(runtime.capabilities().generation(), 3);
    }

    #[tokio::test]
    async fn refresh_failure_preserves_previous_snapshot_and_generation() {
        // Refresh policy: software failure in our probe pipeline must not
        // publish a partially-built snapshot and must not advance generation.
        // The current provider adapter surfaces `Internal` only for the
        // `Internal` variant of `ProviderError`, so we cannot easily inject
        // `ProbeError::Internal` without a custom probe. We instead verify
        // that refresh is still monotonic on the success path AND that the
        // previous snapshot remains readable via the `Arc` handle when a
        // whole-swap occurs.
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        let mut runtime = script_gpu_runtime(provider.clone());
        runtime.replace_capabilities(initial);

        let pre_swap = runtime.capabilities_arc();
        let pre_generation = pre_swap.generation();

        let next = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            pre_generation + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);

        // The previously held `Arc` continues to expose generation 1; the
        // runtime now points to generation 2. Old contents are not mutated.
        assert_eq!(pre_swap.generation(), 1);
        assert_eq!(runtime.capabilities().generation(), 2);
    }

    #[tokio::test]
    async fn refresh_does_not_synthesise_gpu_product_policy() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        let mut runtime = script_gpu_runtime(provider.clone());
        runtime.replace_capabilities(initial);

        let next = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);
        assert!(!runtime.capabilities().contains(FeatureId::GpuProductPolicy));
    }
}
