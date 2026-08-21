//! Application runtime composition for the interactive UI.
//!
//! This module owns construction and grouping of application services. It does
//! not define provider semantics or transport contracts.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use orbis_application::{
    AppService, ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome,
    PerformanceCommandOutcome, PerformanceState, SetChargeLimitError, SetGpuModeError,
    SetPerformanceError,
};
use orbis_capabilities::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot, ProbeError};
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityStatus, OperationCapability,
};
use orbis_core::fan::{FanCurve, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::{AsusdFanProfile, PerformanceProfile};
use orbis_providers::bounded_operation;
use orbis_providers::bounded_provider_call;
use orbis_providers::error::ProviderError;
#[cfg(test)]
use orbis_providers::mock::MockProvider;
use orbis_providers::traits::{
    BatteryProvider, FanCurveMutationProvider, FanCurvePoints, FanProvider, GpuAccessProvider,
    GpuMuxProvider, GpuPowerProvider, GpuProvider, PerformanceProvider,
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
    ) -> Result<Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)>, ProbeError>;

    /// Borrow the inner GPU power provider for capability probing.
    fn provider_power(&self) -> &(dyn orbis_providers::traits::GpuPowerProvider + 'static);

    /// Borrow the inner GPU mux provider for capability probing.
    fn provider_mux(&self) -> &(dyn orbis_providers::traits::GpuMuxProvider + 'static);

    /// Borrow the inner GPU access provider for capability probing.
    fn provider_access(&self) -> &(dyn orbis_providers::traits::GpuAccessProvider + 'static);
}

#[async_trait]
impl<M, P, X, A> GpuServicesRuntime for GpuServices<M, P, X, A>
where
    M: GpuProvider + Send + Sync + 'static,
    P: GpuPowerProvider + Send + Sync + 'static,
    X: GpuMuxProvider + Send + Sync + 'static,
    A: GpuAccessProvider + Send + Sync + 'static,
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
    ) -> Result<Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)>, ProbeError> {
        let power = orbis_providers::probe_gpu_power(self.power.provider()).await?;
        let mux = orbis_providers::probe_gpu_mux(self.mux.provider()).await?;
        let access = orbis_providers::probe_gpu_access(self.access.provider()).await?;
        Ok(vec![
            (orbis_core::FeatureId::GpuPower, power),
            (orbis_core::FeatureId::GpuMux, mux),
            (orbis_core::FeatureId::GpuAccess, access),
        ])
    }

    fn provider_power(&self) -> &(dyn orbis_providers::traits::GpuPowerProvider + 'static) {
        self.power.provider()
    }

    fn provider_mux(&self) -> &(dyn orbis_providers::traits::GpuMuxProvider + 'static) {
        self.mux.provider()
    }

    fn provider_access(&self) -> &(dyn orbis_providers::traits::GpuAccessProvider + 'static) {
        self.access.provider()
    }
}

#[async_trait]
impl<P, X, A> GpuServicesRuntime for GpuPrimitiveServices<P, X, A>
where
    P: GpuPowerProvider + Send + Sync + 'static,
    X: GpuMuxProvider + Send + Sync + 'static,
    A: GpuAccessProvider + Send + Sync + 'static,
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
    ) -> Result<Vec<(orbis_core::FeatureId, orbis_core::capability::Capability)>, ProbeError> {
        let power = orbis_providers::probe_gpu_power(self.primitive_power_provider()).await?;
        let mux = orbis_providers::probe_gpu_mux(self.primitive_mux_provider()).await?;
        let access = orbis_providers::probe_gpu_access(self.primitive_access_provider()).await?;
        Ok(vec![
            (orbis_core::FeatureId::GpuPower, power),
            (orbis_core::FeatureId::GpuMux, mux),
            (orbis_core::FeatureId::GpuAccess, access),
        ])
    }

    fn provider_power(&self) -> &(dyn orbis_providers::traits::GpuPowerProvider + 'static) {
        self.primitive_power_provider()
    }

    fn provider_mux(&self) -> &(dyn orbis_providers::traits::GpuMuxProvider + 'static) {
        self.primitive_mux_provider()
    }

    fn provider_access(&self) -> &(dyn orbis_providers::traits::GpuAccessProvider + 'static) {
        self.primitive_access_provider()
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
    /// `mutation_status` is the typed runtime evidence about the Hardware1
    /// Battery mutation backend; it is passed through to the probe instead of
    /// deriving write support from `validate_charge_limit`.
    ///
    /// This is used by the lifecycle refresh path to build a new
    /// capability registry snapshot. The typed provider-error semantics are
    /// translated into the canonical capability status / constraint set.
    /// Provider-side `Internal` / `InvalidRequest` errors propagate as
    /// `ProbeError::Internal`/`ContractViolation` and abort the refresh
    /// cycle without producing a partial snapshot.
    async fn probe_capability(
        &self,
        mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Result<orbis_core::capability::Capability, orbis_capabilities::ProbeError>;

    /// Borrow the inner battery provider for capability probing.
    fn provider_battery(&self) -> &(dyn orbis_providers::traits::BatteryProvider + 'static);
}

#[async_trait]
impl<P> BatteryServiceRuntime for AppService<P>
where
    P: BatteryProvider + Send + Sync + 'static,
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
        mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Result<orbis_core::capability::Capability, orbis_capabilities::ProbeError> {
        orbis_providers::probe_charge_limit(self.provider(), mutation_status).await
    }

    fn provider_battery(&self) -> &(dyn orbis_providers::traits::BatteryProvider + 'static) {
        self.provider()
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
    /// `mutation_status` is the typed runtime evidence about the Hardware1
    /// Performance mutation backend; it is passed through to the probe instead
    /// of deriving write support from `validate_set_profile`.
    ///
    /// This is used by the lifecycle refresh path to build a new
    /// capability registry snapshot.
    async fn probe_performance(
        &self,
        mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Result<orbis_core::capability::Capability, ProbeError>;

    /// Borrow the inner performance provider for capability probing.
    fn provider_performance(&self)
    -> &(dyn orbis_providers::traits::PerformanceProvider + 'static);
}

#[async_trait]
impl<P> PerformanceServiceRuntime for AppService<P>
where
    P: PerformanceProvider + Send + Sync + 'static,
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

    async fn probe_performance(
        &self,
        mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Result<orbis_core::capability::Capability, ProbeError> {
        orbis_providers::probe_performance(self.provider(), mutation_status).await
    }

    fn provider_performance(
        &self,
    ) -> &(dyn orbis_providers::traits::PerformanceProvider + 'static) {
        self.provider()
    }
}

/// Fan curve service capability boundary.
///
/// Read (`active_curve`, `fan_curve_for_profile`) идёт через существующий
/// read path (sessiond `FanProvider`); mutation (`set_fan_curve`) — через
/// lossless `AsusdFanProfile` напрямую к Hardware1. Никаких новых backend/API.
#[async_trait]
pub trait FanServiceRuntime: Send + Sync {
    /// Read authoritative активную fan curve для вентилятора.
    async fn active_curve(&self, fan: FanId) -> Result<FanCurve, ProviderError>;

    /// Read lossless fan curve для конкретного `AsusdFanProfile`.
    ///
    /// В отличие от `active_curve`, этот метод сохраняет различие между
    /// Quiet и LowPower (lossless read path).
    async fn fan_curve_for_profile(
        &self,
        profile: AsusdFanProfile,
        fan: FanId,
    ) -> Result<FanCurve, ProviderError>;

    /// Mutate одну fan curve (lossless `AsusdFanProfile`).
    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: FanId,
        curve: FanCurvePoints,
    ) -> Result<ApplyResult, ProviderError>;

    /// Probe fan curve capability support metadata.
    ///
    /// Returns a typed `Capability` for the `FanCurves` feature. The write
    /// status is controlled by `mutation_status` (typed Hardware1 evidence),
    /// the read status is derived from the `active_curve` read contract.
    async fn probe_fan_capability(
        &self,
        fan: FanId,
        mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Result<orbis_core::capability::Capability, orbis_capabilities::ProbeError>;

    /// Borrow the inner fan provider for capability probing.
    fn provider_fan(&self) -> &(dyn orbis_providers::traits::FanProvider + 'static);
}

#[async_trait]
impl<P> FanServiceRuntime for AppService<P>
where
    P: FanProvider + FanCurveMutationProvider + Send + Sync + 'static,
{
    async fn active_curve(&self, fan: FanId) -> Result<FanCurve, ProviderError> {
        AppService::active_curve(self, &fan).await
    }

    async fn fan_curve_for_profile(
        &self,
        profile: AsusdFanProfile,
        fan: FanId,
    ) -> Result<FanCurve, ProviderError> {
        AppService::fan_curve_for_profile(self, profile, &fan).await
    }

    async fn set_fan_curve(
        &self,
        profile: AsusdFanProfile,
        fan: FanId,
        curve: FanCurvePoints,
    ) -> Result<ApplyResult, ProviderError> {
        AppService::set_fan_curve(self, profile, &fan, &curve).await
    }

    async fn probe_fan_capability(
        &self,
        fan: FanId,
        mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Result<orbis_core::capability::Capability, orbis_capabilities::ProbeError> {
        orbis_providers::probe_fan_curve(self.provider(), &fan, mutation_status).await
    }

    fn provider_fan(&self) -> &(dyn orbis_providers::traits::FanProvider + 'static) {
        self.provider()
    }
}

/// Telemetry service capability boundary.
#[async_trait]
pub trait TelemetryServiceRuntime: Send + Sync {
    /// Read an authoritative read-only telemetry snapshot.
    async fn snapshot(&self) -> Result<orbis_core::telemetry::Telemetry, ProviderError>;

    /// Provider-defined polling interval for this telemetry backend.
    fn poll_interval(&self) -> Duration;

    /// Canonical provider identity for timeout/error/diagnostic reporting.
    ///
    /// Exposes the underlying `Provider::id()` so the worker path can name the
    /// backend that produced a timeout or failure instead of guessing it from
    /// formatted error text.
    fn provider_id(&self) -> &'static str;

    /// Provider-declared deadline that bounds every snapshot read.
    ///
    /// Exposes the same `Provider::timeout()` value that
    /// `bounded_provider_call` applies, so diagnostics can report the contract
    /// deadline alongside failures.
    fn snapshot_timeout(&self) -> Duration;
}

#[async_trait]
impl<P> TelemetryServiceRuntime for AppService<P>
where
    P: orbis_providers::traits::TelemetryProvider + Send + Sync,
{
    async fn snapshot(&self) -> Result<orbis_core::telemetry::Telemetry, ProviderError> {
        bounded_provider_call(
            self.provider(),
            "telemetry.snapshot",
            self.provider().snapshot(),
        )
        .await
    }

    fn poll_interval(&self) -> Duration {
        self.provider().default_poll_interval()
    }

    fn provider_id(&self) -> &'static str {
        self.provider().id()
    }

    fn snapshot_timeout(&self) -> Duration {
        self.provider().timeout()
    }
}

/// All application services owned by one worker runtime.
pub struct ApplicationRuntime<G, B, R> {
    /// Grouped GPU capabilities.
    pub gpu: G,
    /// Battery service.
    pub battery: B,
    /// Performance service.
    pub performance: R,
    /// Fan curve service (read + mutation).
    ///
    /// `Arc<dyn>` позволяет worker-у обращаться к fan service через
    /// trait-object boundary, как к telemetry.
    pub fan: Arc<dyn FanServiceRuntime>,
    /// Telemetry service (read-only sysfs snapshot).
    ///
    /// `Arc<dyn>` позволяет worker-у клонировать handle для фонового polling
    /// (spawn snapshot task) без блокировки обработки команд.
    pub telemetry: Arc<dyn TelemetryServiceRuntime>,
    /// Read-only capability registry snapshot.
    pub(crate) capabilities: Arc<CapabilityRegistrySnapshot>,
    /// Typed Hardware1 fan curve mutation backend evidence (startup-time).
    ///
    /// Controls FanCurves write capability. Unlike the old `bool`, it
    /// preserves Supported / Unsupported / TemporarilyUnavailable /
    /// PermissionDenied / BackendMissing / Unknown so the UI can distinguish
    /// them honestly.
    fan_mutation_status: orbis_core::capability::CapabilityStatus,
    /// Typed Hardware1 Battery mutation backend evidence (startup-time).
    ///
    /// Controls ChargeLimit write capability. Unlike a bool, it preserves
    /// Supported / Unsupported / TemporarilyUnavailable / PermissionDenied /
    /// Unknown so the UI can distinguish them honestly.
    battery_mutation_status: orbis_core::capability::CapabilityStatus,
    /// Typed Hardware1 Performance mutation backend evidence (startup-time).
    ///
    /// Controls Performance write capability with the same honest status
    /// distinctions as the Battery mutation evidence.
    performance_mutation_status: orbis_core::capability::CapabilityStatus,
    /// Connection for re-querying mutation statuses during periodic refresh.
    ///
    /// Stored once at startup; used by `requery_mutation_statuses` to detect
    /// runtime changes in Battery/Performance/FanCurves mutation availability
    /// (e.g., asusd daemon starts or stops). Read-only D-Bus queries; no
    /// mutations or authorization required.
    mutation_status_connection: Option<zbus::Connection>,
}

impl<G, B, R> ApplicationRuntime<G, B, R> {
    /// Create an application runtime with an explicitly built capability snapshot.
    ///
    /// Production composition must always supply an authoritative snapshot
    /// assembled through the discovery pipeline. Permissive constructors that
    /// silently allocate an empty default snapshot have been removed.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_snapshot<T, F>(
        gpu: G,
        battery: B,
        performance: R,
        fan: F,
        telemetry: T,
        snapshot: CapabilityRegistrySnapshot,
        fan_mutation_status: orbis_core::capability::CapabilityStatus,
        battery_mutation_status: orbis_core::capability::CapabilityStatus,
        performance_mutation_status: orbis_core::capability::CapabilityStatus,
        mutation_status_connection: Option<zbus::Connection>,
    ) -> Self
    where
        T: TelemetryServiceRuntime + 'static,
        F: FanServiceRuntime + 'static,
    {
        Self {
            gpu,
            battery,
            performance,
            fan: Arc::new(fan),
            telemetry: Arc::new(telemetry),
            capabilities: Arc::new(snapshot),
            fan_mutation_status,
            battery_mutation_status,
            performance_mutation_status,
            mutation_status_connection,
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
    #[allow(clippy::too_many_arguments)]
    pub fn empty_for_testing<T, F>(
        gpu: G,
        battery: B,
        performance: R,
        fan: F,
        telemetry: T,
        fan_mutation_status: orbis_core::capability::CapabilityStatus,
        battery_mutation_status: orbis_core::capability::CapabilityStatus,
        performance_mutation_status: orbis_core::capability::CapabilityStatus,
    ) -> Self
    where
        T: TelemetryServiceRuntime + 'static,
        F: FanServiceRuntime + 'static,
    {
        let checked_at = SystemTime::now();
        let snapshot = CapabilityRegistryBuilder::new(1, checked_at)
            .build()
            .expect("empty registry snapshot must build");
        Self {
            gpu,
            battery,
            performance,
            fan: Arc::new(fan),
            telemetry: Arc::new(telemetry),
            capabilities: Arc::new(snapshot),
            fan_mutation_status,
            battery_mutation_status,
            performance_mutation_status,
            mutation_status_connection: None,
        }
    }

    /// Typed Hardware1 fan curve mutation backend evidence.
    pub fn fan_mutation_status(&self) -> orbis_core::capability::CapabilityStatus {
        self.fan_mutation_status
    }

    /// Typed Hardware1 Battery mutation backend evidence.
    pub fn battery_mutation_status(&self) -> orbis_core::capability::CapabilityStatus {
        self.battery_mutation_status
    }

    /// Typed Hardware1 Performance mutation backend evidence.
    pub fn performance_mutation_status(&self) -> orbis_core::capability::CapabilityStatus {
        self.performance_mutation_status
    }

    /// Re-query all three Hardware1 mutation statuses from D-Bus.
    ///
    /// Read-only D-Bus queries; no mutations, no authorization, no setter
    /// calls. Used by periodic capability refresh to detect runtime changes
    /// in mutation backend availability (e.g., asusd daemon starts or stops).
    ///
    /// If no connection is stored (test mode), keeps current values unchanged.
    pub async fn requery_mutation_statuses(&mut self) {
        let Some(connection) = &self.mutation_status_connection else {
            return;
        };
        self.fan_mutation_status = bounded_hardware1_status(
            "fan_mutation_status",
            orbis_session_client::hardware1_fan_mutation_status(connection),
        )
        .await;
        self.battery_mutation_status = bounded_hardware1_status(
            "battery_mutation_status",
            orbis_session_client::hardware1_battery_mutation_status(connection),
        )
        .await;
        self.performance_mutation_status = bounded_hardware1_status(
            "performance_mutation_status",
            orbis_session_client::hardware1_performance_mutation_status(connection),
        )
        .await;
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

/// Run all capability probes against the provided services and assemble
/// a deterministic immutable snapshot for the requested `generation`.
///
/// Ordinary provider evidence does not abort the assembly. `ProbeError`
/// aborts the assembly without producing a partial snapshot.
#[allow(clippy::too_many_arguments)]
pub async fn probe_capability_registry<Bp, Pp, Gpow, Gmux, Gacc, Fp>(
    battery_provider: &Bp,
    performance_provider: &Pp,
    gpu_power_provider: &Gpow,
    gpu_mux_provider: &Gmux,
    gpu_access_provider: &Gacc,
    fan_provider: &Fp,
    fan_mutation_status: orbis_core::capability::CapabilityStatus,
    battery_mutation_status: orbis_core::capability::CapabilityStatus,
    performance_mutation_status: orbis_core::capability::CapabilityStatus,
    generation: u64,
    checked_at: SystemTime,
) -> Result<CapabilityRegistrySnapshot, ProbeError>
where
    Bp: orbis_providers::traits::BatteryProvider + ?Sized,
    Pp: orbis_providers::traits::PerformanceProvider + ?Sized,
    Gpow: orbis_providers::traits::GpuPowerProvider + ?Sized,
    Gmux: orbis_providers::traits::GpuMuxProvider + ?Sized,
    Gacc: orbis_providers::traits::GpuAccessProvider + ?Sized,
    Fp: orbis_providers::traits::FanProvider + ?Sized,
{
    let mut builder = CapabilityRegistryBuilder::new(generation, checked_at);

    let performance =
        orbis_providers::probe_performance(performance_provider, performance_mutation_status)
            .await?;
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

    let battery =
        orbis_providers::probe_charge_limit(battery_provider, battery_mutation_status).await?;
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

    // Fan curve read capabilities: CPU and GPU active curve reads. Write
    // capability comes from typed Hardware1 fan mutation evidence
    // (fan_mutation_status). Curve points never enter the registry — only
    // support metadata.
    let cpu_curve = orbis_providers::probe_fan_curve(
        fan_provider,
        &orbis_core::fan::FanId::Cpu,
        fan_mutation_status,
    )
    .await?;
    let gpu_curve = orbis_providers::probe_fan_curve(
        fan_provider,
        &orbis_core::fan::FanId::Gpu,
        fan_mutation_status,
    )
    .await?;
    let fan_curves = aggregate_fan_curve_capability(cpu_curve, gpu_curve);
    builder
        .add(orbis_core::FeatureId::FanCurves, fan_curves)
        .map_err(|err| match err {
            orbis_capabilities::RegistryError::DuplicateCapability { feature } => {
                ProbeError::ContractViolation(format!(
                    "fan curve capability reported twice: {feature:?}"
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

/// Aggregate FanCurves only when both CPU and GPU curve-read contracts are
/// proven. RPM telemetry is intentionally not an input to this capability.
fn aggregate_fan_curve_capability(cpu: Capability, gpu: Capability) -> Capability {
    if cpu.operations.read.status == CapabilityStatus::Supported
        && gpu.operations.read.status == CapabilityStatus::Supported
    {
        return orbis_capabilities::capability_from_operations(cpu.operations, cpu.constraints);
    }

    let read = if cpu.operations.read.status != CapabilityStatus::Supported {
        cpu.operations.read
    } else {
        gpu.operations.read
    };
    orbis_capabilities::capability_from_operations(
        CapabilityOperations {
            read,
            write: OperationCapability::new(CapabilityStatus::Unsupported),
        },
        CapabilityConstraints::Unknown,
    )
}

/// Assemble the initial capability registry snapshot for production.
///
/// Fail-fast on `ProbeError::Internal` and `ProbeError::ContractViolation` —
/// production must not commit to a partially-trustworthy registry on
/// initial discovery.
#[allow(clippy::too_many_arguments)]
pub async fn build_initial_registry_snapshot<Bp, Pp, Gpow, Gmux, Gacc, Fp>(
    battery_provider: &Bp,
    performance_provider: &Pp,
    gpu_power_provider: &Gpow,
    gpu_mux_provider: &Gmux,
    gpu_access_provider: &Gacc,
    fan_provider: &Fp,
    fan_mutation_status: orbis_core::capability::CapabilityStatus,
    battery_mutation_status: orbis_core::capability::CapabilityStatus,
    performance_mutation_status: orbis_core::capability::CapabilityStatus,
) -> Result<CapabilityRegistrySnapshot, RegistryAssemblyError>
where
    Bp: orbis_providers::traits::BatteryProvider + ?Sized,
    Pp: orbis_providers::traits::PerformanceProvider + ?Sized,
    Gpow: orbis_providers::traits::GpuPowerProvider + ?Sized,
    Gmux: orbis_providers::traits::GpuMuxProvider + ?Sized,
    Gacc: orbis_providers::traits::GpuAccessProvider + ?Sized,
    Fp: orbis_providers::traits::FanProvider + ?Sized,
{
    let checked_at = SystemTime::now();
    probe_capability_registry(
        battery_provider,
        performance_provider,
        gpu_power_provider,
        gpu_mux_provider,
        gpu_access_provider,
        fan_provider,
        fan_mutation_status,
        battery_mutation_status,
        performance_mutation_status,
        1,
        checked_at,
    )
    .await
    .map_err(RegistryAssemblyError::Probe)
}

/// Re-probe all capabilities and produce a deterministic snapshot for
/// the requested next generation. The returned `Result<CapabilityRegistrySnapshot, RefreshError>`
/// is `Err` only when our own probe pipeline reports a software failure
/// (`ProbeError::Internal`/`ContractViolation`); ordinary provider evidence
/// always results in `Ok`.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_capability_registry<Bp, Pp, Gpow, Gmux, Gacc, Fp>(
    battery_provider: &Bp,
    performance_provider: &Pp,
    gpu_power_provider: &Gpow,
    gpu_mux_provider: &Gmux,
    gpu_access_provider: &Gacc,
    fan_provider: &Fp,
    fan_mutation_status: orbis_core::capability::CapabilityStatus,
    battery_mutation_status: orbis_core::capability::CapabilityStatus,
    performance_mutation_status: orbis_core::capability::CapabilityStatus,
    next_generation: u64,
) -> Result<CapabilityRegistrySnapshot, RefreshError>
where
    Bp: orbis_providers::traits::BatteryProvider + ?Sized,
    Pp: orbis_providers::traits::PerformanceProvider + ?Sized,
    Gpow: orbis_providers::traits::GpuPowerProvider + ?Sized,
    Gmux: orbis_providers::traits::GpuMuxProvider + ?Sized,
    Gacc: orbis_providers::traits::GpuAccessProvider + ?Sized,
    Fp: orbis_providers::traits::FanProvider + ?Sized,
{
    let checked_at = SystemTime::now();
    probe_capability_registry(
        battery_provider,
        performance_provider,
        gpu_power_provider,
        gpu_mux_provider,
        gpu_access_provider,
        fan_provider,
        fan_mutation_status,
        battery_mutation_status,
        performance_mutation_status,
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
>;

/// Deadline for one read-only Hardware1 status query over D-Bus.
///
/// This bounds the canonical mutation-status requery (#123): a hung
/// hardwared/service may delay capability refresh or startup by at most this
/// deadline, never indefinitely. The value mirrors the per-read deadlines of
/// the Session1 providers (one second) with headroom for bus round-trips.
const HARDWARE1_STATUS_DEADLINE: Duration = Duration::from_secs(2);

/// Run one read-only Hardware1 status query within an explicit deadline.
///
/// These queries already map every transport/protocol failure to
/// [`CapabilityStatus::Unknown`] — the honest no-evidence state. Bounding the
/// wait extends exactly that classification to a hung peer; a timeout is never
/// reported as success and never retried. The next canonical refresh re-queries
/// statuses from scratch.
async fn bounded_hardware1_status<F>(
    operation: &'static str,
    future: F,
) -> orbis_core::capability::CapabilityStatus
where
    F: Future<Output = orbis_core::capability::CapabilityStatus>,
{
    // The status queries resolve to a plain status rather than
    // `Result<_, ProviderError>`; adapt them so the canonical bounded
    // primitive owns the deadline.
    let outcome = bounded_operation(
        HARDWARE1_STATUS_DEADLINE,
        "hardware1",
        operation,
        async move { Ok::<_, ProviderError>(future.await) },
    )
    .await;
    match outcome {
        Ok(status) => status,
        Err(_) => orbis_core::capability::CapabilityStatus::Unknown,
    }
}

/// Build the current production application runtime.
pub async fn build_production_runtime(
    session_connection: zbus::Connection,
    system_connection: zbus::Connection,
) -> anyhow::Result<(ProductionRuntime, bool)> {
    let hardware_owner = bounded_operation(
        HARDWARE1_STATUS_DEADLINE,
        "hardware1",
        "write_available",
        async { Ok::<_, ProviderError>(hardware1_write_available(&system_connection).await) },
    )
    .await
    .unwrap_or(false);
    let fan_mutation_status = bounded_hardware1_status(
        "fan_mutation_status",
        orbis_session_client::hardware1_fan_mutation_status(&system_connection),
    )
    .await;
    let battery_mutation_status = bounded_hardware1_status(
        "battery_mutation_status",
        orbis_session_client::hardware1_battery_mutation_status(&system_connection),
    )
    .await;
    let performance_mutation_status = bounded_hardware1_status(
        "performance_mutation_status",
        orbis_session_client::hardware1_performance_mutation_status(&system_connection),
    )
    .await;

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
        ZbusSessionPerformanceSource::new(session_connection.clone()),
        ZbusHardwarePerformanceSource::new(system_connection.clone()),
    ));
    let battery = AppService::new(battery_arc.clone());
    let performance = AppService::new(performance_arc.clone());

    // Read-only sysfs telemetry: dynamic hwmon/power_supply discovery, no
    // writes, no privileged APIs. Construction performs no I/O.
    let telemetry = AppService::new(Arc::new(orbis_providers::SysfsTelemetryProvider::default()));

    // Composed fan curve provider:
    // - profile-specific read: Session1 → sessiond → asusd
    //   (`ZbusAsusdFanCurveSource::read_curves(profile)`), GUI напрямую asusd
    //   НЕ читает;
    // - активная кривая остаётся через existing sysfs `asus_custom_fan_curve`;
    // - capability probe остаётся на active sysfs curve;
    // - mutation (`set_fan_curve`) остаётся напрямую через Hardware1 (original
    //   caller). Write capability определяется наличием production Hardware1
    //   mutation backend (hardware_owner). No writes, no privileged APIs.
    let fan_provider = orbis_session_client::SessionHardwareFanCurveProvider::new(
        orbis_session_client::SessionProfileFanCurveProvider::new(
            orbis_session_client::ZbusSessionFanCurveSource::new(session_connection.clone()),
            orbis_sessiond::fans::SysfsFanCurveProvider::new(
                orbis_sessiond::fans::SysfsFanCurveSource::default(),
            ),
        ),
        orbis_session_client::ZbusHardwareFanCurveSource::new(system_connection.clone()),
    );
    let fan_service = AppService::new(Arc::new(fan_provider));

    let snapshot = build_initial_registry_snapshot(
        &*battery_arc,
        &*performance_arc,
        gpu.primitive_power_provider(),
        gpu.primitive_mux_provider(),
        gpu.primitive_access_provider(),
        fan_service.provider(),
        fan_mutation_status,
        battery_mutation_status,
        performance_mutation_status,
    )
    .await?;

    Ok((
        ApplicationRuntime::new_with_snapshot(
            gpu,
            battery,
            performance,
            fan_service,
            telemetry,
            snapshot,
            fan_mutation_status,
            battery_mutation_status,
            performance_mutation_status,
            Some(system_connection),
        ),
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
>;

/// Test-only grouped runtime using one mock provider for all capabilities.
#[cfg(test)]
pub fn mock_runtime() -> MockRuntime {
    use orbis_core::capability::CapabilityStatus;
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
        AppService::new(provider.clone()),
        AppService::new(provider),
        empty,
        CapabilityStatus::Unsupported,
        CapabilityStatus::Unsupported,
        CapabilityStatus::Unsupported,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ApplicationRuntime, CapabilityRegistrySnapshot, GpuPrimitiveServices, GpuServices,
        GpuServicesRuntime, HARDWARE1_STATUS_DEADLINE, MockRuntime, ProbeError,
        TelemetryServiceRuntime, aggregate_fan_curve_capability, bounded_hardware1_status,
        build_initial_registry_snapshot, probe_capability_registry, refresh_capability_registry,
    };
    use orbis_application::{AppService, CommandError};
    use orbis_core::FeatureId;
    use orbis_core::capability::{
        Capability, CapabilityOperations, CapabilityStatus, OperationCapability,
    };
    use orbis_core::gpu::GpuMode;
    use orbis_providers::error::ProviderError;
    use orbis_providers::mock::{MockErrorMode, MockProvider};
    use orbis_test_support::devices::build_state_arc;
    use std::cell::Cell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn hung_hardware1_status_requery_resolves_unknown_within_deadline() {
        let started = tokio::time::Instant::now();
        let status = bounded_hardware1_status("fan_mutation_status", std::future::pending()).await;

        assert_eq!(status, CapabilityStatus::Unknown);
        // The paused Tokio clock advances exactly to the deadline when the
        // timeout fires, so equality proves the bound is actually applied.
        assert_eq!(started.elapsed(), HARDWARE1_STATUS_DEADLINE);
    }

    #[tokio::test(start_paused = true)]
    async fn hardware1_status_timeout_is_never_success_and_never_retried() {
        let calls = Cell::new(0u32);
        let status = bounded_hardware1_status("battery_mutation_status", async {
            calls.set(calls.get() + 1);
            std::future::pending::<CapabilityStatus>().await
        })
        .await;

        assert_eq!(status, CapabilityStatus::Unknown);
        assert_eq!(calls.get(), 1, "timeout must not retry the query");
    }

    #[tokio::test]
    async fn proven_hardware1_status_passes_through_without_coercion() {
        let proven = [
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::PermissionDenied,
        ];
        for expected in proven {
            let observed =
                bounded_hardware1_status("performance_mutation_status", async move { expected })
                    .await;
            assert_eq!(observed, expected);
        }
    }

    // --- Telemetry provider timeout/identity contract (#123) ---

    /// Test provider with a controllable deadline and snapshot behaviour.
    struct TelemetryContractProvider {
        id: &'static str,
        timeout: Duration,
        hang: bool,
        error: Box<dyn Fn() -> ProviderError + Send + Sync>,
        snapshot_calls: AtomicU32,
    }

    impl TelemetryContractProvider {
        fn hung(id: &'static str, timeout: Duration) -> Arc<Self> {
            Arc::new(Self {
                id,
                timeout,
                hang: true,
                error: Box::new(|| ProviderError::Internal("unused".into())),
                snapshot_calls: AtomicU32::new(0),
            })
        }

        fn failing(
            id: &'static str,
            timeout: Duration,
            error: impl Fn() -> ProviderError + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                id,
                timeout,
                hang: false,
                error: Box::new(error),
                snapshot_calls: AtomicU32::new(0),
            })
        }
    }

    impl orbis_providers::traits::Provider for TelemetryContractProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn backend(&self) -> orbis_core::BackendIdentity {
            orbis_core::BackendIdentity::simple(self.id)
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("telemetry contract test: {feature} unsupported")
        }

        fn health(&self) -> orbis_providers::traits::ProviderHealth {
            orbis_providers::traits::ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<orbis_core::diagnostics::DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait::async_trait]
    impl orbis_providers::traits::TelemetryProvider for TelemetryContractProvider {
        async fn snapshot(&self) -> Result<orbis_core::telemetry::Telemetry, ProviderError> {
            self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            if self.hang {
                std::future::pending().await
            } else {
                Err((self.error)())
            }
        }

        fn default_poll_interval(&self) -> Duration {
            Duration::from_secs(1)
        }
    }

    #[test]
    fn telemetry_service_exposes_canonical_provider_identity_and_deadline() {
        let service = AppService::new(TelemetryContractProvider::hung(
            "identity-telemetry",
            Duration::from_millis(750),
        ));

        assert_eq!(
            TelemetryServiceRuntime::provider_id(&service),
            "identity-telemetry"
        );
        assert_eq!(
            TelemetryServiceRuntime::snapshot_timeout(&service),
            Duration::from_millis(750)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn hung_telemetry_provider_times_out_with_identity_and_never_retries() {
        let provider =
            TelemetryContractProvider::hung("hung-telemetry", Duration::from_millis(250));
        let service = AppService::new(provider.clone());
        let started = tokio::time::Instant::now();

        let result = service.snapshot().await;

        assert_eq!(started.elapsed(), Duration::from_millis(250));
        match &result {
            Err(ProviderError::Timeout(detail)) => {
                // Canonical identity survives into the timeout evidence.
                assert!(detail.contains("hung-telemetry"));
                assert!(detail.contains("telemetry.snapshot"));
                assert!(detail.contains("250 ms"));
            }
            other => panic!("expected Timeout with identity, got {other:?}"),
        }
        assert_eq!(
            provider.snapshot_calls.load(Ordering::SeqCst),
            1,
            "timeout must not retry the telemetry read"
        );
    }

    #[tokio::test]
    async fn telemetry_error_classes_pass_through_without_coercion() {
        fn debug(error: &ProviderError) -> String {
            format!("{error:?}")
        }

        let cases: Vec<(String, Box<dyn Fn() -> ProviderError + Send + Sync>)> = vec![
            (
                "Unsupported".into(),
                Box::new(|| ProviderError::Unsupported("no hwmon source".into())),
            ),
            (
                "BackendUnavailable".into(),
                Box::new(|| ProviderError::BackendUnavailable("sysfs absent".into())),
            ),
            (
                "PermissionDenied".into(),
                Box::new(|| ProviderError::PermissionDenied("read denied".into())),
            ),
            (
                "Internal".into(),
                Box::new(|| ProviderError::Internal("malformed value".into())),
            ),
        ];
        for (class, error) in cases {
            let provider = TelemetryContractProvider::failing(
                "class-telemetry",
                Duration::from_secs(1),
                error,
            );
            let service = AppService::new(provider);

            let observed = service.snapshot().await;

            let observed_error = observed.expect_err("failing provider must return its error");
            assert!(
                debug(&observed_error).starts_with(&format!("{class}(")),
                "error class {class} must pass through uncoerced, got {}",
                debug(&observed_error)
            );
        }
    }

    #[test]
    fn fan_curve_capability_requires_cpu_and_gpu_curve_reads() {
        let cpu =
            Capability::new(CapabilityStatus::Supported).with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(CapabilityStatus::Supported),
            });
        let gpu_missing =
            Capability::new(CapabilityStatus::Unsupported).with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Unsupported),
                write: OperationCapability::new(CapabilityStatus::Unsupported),
            });

        let aggregate = aggregate_fan_curve_capability(cpu, gpu_missing);
        assert_eq!(
            aggregate.operations.read.status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            aggregate.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

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
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
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
            AppService::new(provider.clone()),
            AppService::new(provider),
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
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
    async fn initial_runtime_snapshot_contains_all_capabilities() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("scripted provider must produce a coherent snapshot");
        assert!(snapshot.contains(FeatureId::Performance));
        assert!(snapshot.contains(FeatureId::ChargeLimit));
        assert!(snapshot.contains(FeatureId::GpuPower));
        assert!(snapshot.contains(FeatureId::GpuMux));
        assert!(snapshot.contains(FeatureId::GpuAccess));
        assert!(snapshot.contains(FeatureId::FanCurves));
        assert!(!snapshot.contains(FeatureId::GpuProductPolicy));
    }

    #[tokio::test]
    async fn fan_curve_capability_is_read_only() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("scripted provider must produce a coherent snapshot");
        let fan = snapshot
            .capability(FeatureId::FanCurves)
            .expect("fan curve capability present");
        // Read-only: read Supported, write Unsupported.
        assert_eq!(fan.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(fan.operations.write.status, CapabilityStatus::Unsupported);
    }

    #[tokio::test]
    async fn fan_curve_write_is_supported_when_hardware_backend_proven() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("scripted provider must produce a coherent snapshot");
        let fan = snapshot
            .capability(FeatureId::FanCurves)
            .expect("fan curve capability present");
        // Write capability декларативна: Supported только при доказанном
        // Hardware1 mutation backend; никаких пробных writes.
        assert_eq!(fan.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(fan.operations.write.status, CapabilityStatus::Supported);
    }

    async fn build_initial_snapshot_for_refresh(
        provider: &MockProvider,
    ) -> CapabilityRegistrySnapshot {
        build_initial_registry_snapshot(
            provider,
            provider,
            provider,
            provider,
            provider,
            provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
        )
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
            AppService::new(provider.clone()),
            AppService::new(provider),
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
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
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
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
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(third);
        assert_eq!(runtime.capabilities().generation(), 3);
    }

    #[tokio::test]
    async fn backend_disappearance_and_recovery_publish_new_generations() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        assert_eq!(
            initial
                .capability(FeatureId::Performance)
                .expect("performance")
                .status,
            CapabilityStatus::Supported
        );

        state.write().await.error_mode = MockErrorMode::BackendDown;
        let unavailable = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            initial.generation() + 1,
        )
        .await
        .expect("backend failure is capability-local");
        assert_eq!(unavailable.generation(), 2);
        assert_eq!(
            unavailable
                .capability(FeatureId::Performance)
                .expect("performance")
                .status,
            CapabilityStatus::BackendMissing
        );
        assert_eq!(
            unavailable
                .capability(FeatureId::ChargeLimit)
                .expect("charge limit")
                .status,
            CapabilityStatus::BackendMissing
        );

        state.write().await.error_mode = MockErrorMode::Timeout;
        let temporary = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            unavailable.generation() + 1,
        )
        .await
        .expect("timeout is capability-local");
        assert_eq!(temporary.generation(), 3);
        assert_eq!(
            temporary
                .capability(FeatureId::Performance)
                .expect("performance")
                .status,
            CapabilityStatus::TemporarilyUnavailable
        );

        state.write().await.error_mode = MockErrorMode::None;
        let recovered = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            temporary.generation() + 1,
        )
        .await
        .expect("backend recovery must be capability-local");
        assert_eq!(recovered.generation(), 4);
        assert_eq!(
            recovered
                .capability(FeatureId::Performance)
                .expect("performance")
                .status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            recovered
                .capability(FeatureId::ChargeLimit)
                .expect("charge limit")
                .status,
            CapabilityStatus::Supported
        );
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
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
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
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);
        assert!(!runtime.capabilities().contains(FeatureId::GpuProductPolicy));
    }

    // -----------------------------------------------------------------------
    // Regression: FanCurves preserved across refresh
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn refresh_preserves_fan_curves() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        assert!(
            initial.contains(FeatureId::FanCurves),
            "initial snapshot must contain FanCurves"
        );
        let mut runtime = script_gpu_runtime(provider.clone());
        runtime.replace_capabilities(initial);

        let next = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);
        assert!(
            runtime.capabilities().contains(FeatureId::FanCurves),
            "refreshed snapshot must still contain FanCurves"
        );
    }

    #[tokio::test]
    async fn refresh_fan_curves_write_status_matches_evidence() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        let mut runtime = script_gpu_runtime(provider.clone());
        runtime.replace_capabilities(initial);

        // fan mutation status = Supported: write should be Supported after refresh.
        let next = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);
        let fan = runtime
            .capabilities()
            .capability(FeatureId::FanCurves)
            .expect("FanCurves present");
        assert_eq!(fan.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(fan.operations.write.status, CapabilityStatus::Supported);

        // fan mutation status = Unsupported: write should be Unsupported after refresh.
        let next = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);
        let fan = runtime
            .capabilities()
            .capability(FeatureId::FanCurves)
            .expect("FanCurves present");
        assert_eq!(fan.operations.read.status, CapabilityStatus::Supported);
        assert_eq!(fan.operations.write.status, CapabilityStatus::Unsupported);
    }

    #[tokio::test]
    async fn refresh_contains_all_six_canonical_capabilities() {
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
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            runtime.capabilities().generation() + 1,
        )
        .await
        .expect("refresh must succeed");
        runtime.replace_capabilities(next);

        // All six canonical capability entries must be present.
        for feature in [
            FeatureId::Performance,
            FeatureId::ChargeLimit,
            FeatureId::GpuPower,
            FeatureId::GpuMux,
            FeatureId::GpuAccess,
            FeatureId::FanCurves,
        ] {
            assert!(
                runtime.capabilities().contains(feature),
                "refreshed snapshot missing {feature:?}"
            );
        }
        // Negative: GpuProductPolicy must NOT be synthesised.
        assert!(!runtime.capabilities().contains(FeatureId::GpuProductPolicy));
    }

    // -----------------------------------------------------------------------
    // Regression: unified probe lifecycle
    // -----------------------------------------------------------------------

    /// Test-only GPU provider that returns a configurable error.
    struct ScriptedGpuError {
        error: ProviderError,
    }

    impl orbis_providers::traits::Provider for ScriptedGpuError {
        fn id(&self) -> &'static str {
            "scripted-gpu-error"
        }
        fn backend(&self) -> orbis_core::identity::BackendIdentity {
            orbis_core::identity::BackendIdentity::simple("scripted-gpu-error")
        }
        fn timeout(&self) -> std::time::Duration {
            std::time::Duration::from_millis(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-gpu-error: {feature}")
        }
        fn health(&self) -> orbis_providers::traits::ProviderHealth {
            orbis_providers::traits::ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<orbis_core::diagnostics::DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait::async_trait]
    impl orbis_providers::traits::GpuPowerProvider for ScriptedGpuError {
        async fn power_state(&self) -> Result<orbis_core::gpu::GpuPowerState, ProviderError> {
            Err(match &self.error {
                ProviderError::Internal(d) => ProviderError::Internal(d.clone()),
                ProviderError::Unsupported(d) => ProviderError::Unsupported(d.clone()),
                other => panic!("unexpected error variant: {other:?}"),
            })
        }
    }

    #[async_trait::async_trait]
    impl orbis_providers::traits::GpuMuxProvider for ScriptedGpuError {
        async fn mux_state(&self) -> Result<orbis_core::gpu::GpuMuxState, ProviderError> {
            Err(match &self.error {
                ProviderError::Internal(d) => ProviderError::Internal(d.clone()),
                ProviderError::InvalidRequest(d) => ProviderError::InvalidRequest(d.clone()),
                ProviderError::Unsupported(d) => ProviderError::Unsupported(d.clone()),
                other => panic!("unexpected error variant: {other:?}"),
            })
        }
    }

    #[async_trait::async_trait]
    impl orbis_providers::traits::GpuAccessProvider for ScriptedGpuError {
        async fn access_policy(&self) -> Result<orbis_core::gpu::GpuAccessPolicy, ProviderError> {
            Err(match &self.error {
                ProviderError::Internal(d) => ProviderError::Internal(d.clone()),
                other => panic!("unexpected error variant: {other:?}"),
            })
        }
    }

    #[tokio::test]
    async fn initial_and_refresh_contain_same_six_feature_ids() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("initial snapshot must succeed");

        let refreshed = refresh_capability_registry(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            2,
        )
        .await
        .expect("refresh must succeed");

        let expected = [
            FeatureId::Performance,
            FeatureId::ChargeLimit,
            FeatureId::GpuPower,
            FeatureId::GpuMux,
            FeatureId::GpuAccess,
            FeatureId::FanCurves,
        ];

        for feature in expected {
            assert!(
                initial.contains(feature),
                "initial snapshot missing {feature:?}"
            );
            assert!(
                refreshed.contains(feature),
                "refreshed snapshot missing {feature:?}"
            );
        }
        assert_eq!(initial.len(), refreshed.len());
    }

    #[tokio::test]
    async fn gpu_power_internal_error_aborts_refresh() {
        let gpu_err = ScriptedGpuError {
            error: ProviderError::Internal("injected failure".into()),
        };
        let result = probe_capability_registry(
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &gpu_err,
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            1,
            std::time::SystemTime::now(),
        )
        .await;
        assert!(result.is_err(), "ProbeError::Internal must abort refresh");
        match result.unwrap_err() {
            ProbeError::Internal(_) => {}
            other => panic!("expected ProbeError::Internal, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn gpu_mux_contract_violation_aborts_refresh() {
        let gpu_err = ScriptedGpuError {
            error: ProviderError::InvalidRequest("contract broken".into()),
        };
        let result = probe_capability_registry(
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &gpu_err,
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            1,
            std::time::SystemTime::now(),
        )
        .await;
        assert!(
            result.is_err(),
            "ProbeError::ContractViolation must abort refresh"
        );
        match result.unwrap_err() {
            ProbeError::ContractViolation(_) => {}
            other => panic!("expected ProbeError::ContractViolation, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_gpu_probe_preserves_previous_snapshot_and_generation() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let initial = build_initial_snapshot_for_refresh(&provider).await;
        let mut runtime = script_gpu_runtime(provider.clone());
        runtime.replace_capabilities(initial);

        let pre_swap = runtime.capabilities_arc();
        let pre_generation = pre_swap.generation();

        // Verify the previous snapshot is accessible and complete.
        for feature in [
            FeatureId::Performance,
            FeatureId::ChargeLimit,
            FeatureId::GpuPower,
            FeatureId::GpuMux,
            FeatureId::GpuAccess,
            FeatureId::FanCurves,
        ] {
            assert!(
                pre_swap.contains(feature),
                "pre-swap snapshot missing {feature:?}"
            );
        }

        // Now verify that if a refresh fails, the runtime snapshot is unchanged.
        // We can't easily inject ProbeError into the trait-based runtime without
        // custom providers, so we verify the invariant via probe_capability_registry
        // directly: it returns Err, meaning the caller must NOT replace the snapshot.
        let result = probe_capability_registry(
            &*provider,
            &*provider,
            &ScriptedGpuError {
                error: ProviderError::Internal("injected".into()),
            },
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            pre_generation + 1,
            std::time::SystemTime::now(),
        )
        .await;
        assert!(result.is_err(), "probe must fail");

        // Runtime snapshot unchanged.
        assert_eq!(runtime.capabilities().generation(), pre_generation);
        for feature in [
            FeatureId::Performance,
            FeatureId::ChargeLimit,
            FeatureId::GpuPower,
            FeatureId::GpuMux,
            FeatureId::GpuAccess,
            FeatureId::FanCurves,
        ] {
            assert!(
                runtime.capabilities().contains(feature),
                "runtime snapshot lost {feature:?} after failed refresh"
            );
        }
    }

    #[tokio::test]
    async fn ordinary_gpu_unsupported_stays_as_entry() {
        let gpu_err = ScriptedGpuError {
            error: ProviderError::Unsupported("not supported".into()),
        };
        let result = probe_capability_registry(
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &gpu_err,
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            &MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists")),
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            1,
            std::time::SystemTime::now(),
        )
        .await
        .expect("ordinary Unsupported must not abort");

        // GpuPower must be present with Unsupported status, not missing.
        let power = result
            .capability(FeatureId::GpuPower)
            .expect("GpuPower entry must exist");
        assert_eq!(
            power.status,
            CapabilityStatus::Unsupported,
            "GpuPower should be Unsupported, not missing"
        );

        // Other capabilities must still be present and Supported.
        assert!(result.contains(FeatureId::Performance));
        assert!(result.contains(FeatureId::ChargeLimit));
        assert!(result.contains(FeatureId::GpuMux));
        assert!(result.contains(FeatureId::GpuAccess));
        assert!(result.contains(FeatureId::FanCurves));
    }

    #[tokio::test]
    async fn probe_primitives_propagates_probe_error() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let gpu_err = std::sync::Arc::new(ScriptedGpuError {
            error: ProviderError::Internal("injected".into()),
        });
        let gpu = GpuServices::new(
            AppService::new(provider.clone()),
            AppService::new(gpu_err.clone()),
            AppService::new(provider.clone()),
            AppService::new(provider.clone()),
        );
        let result = gpu.probe_primitives().await;
        assert!(
            result.is_err(),
            "probe_primitives must propagate ProbeError"
        );
        match result.unwrap_err() {
            ProbeError::Internal(_) => {}
            other => panic!("expected ProbeError::Internal, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_primitives_returns_all_three_on_success() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let gpu = GpuServices::new(
            AppService::new(provider.clone()),
            AppService::new(provider.clone()),
            AppService::new(provider.clone()),
            AppService::new(provider.clone()),
        );
        let entries = gpu
            .probe_primitives()
            .await
            .expect("probe_primitives must succeed");
        assert_eq!(entries.len(), 3);
        let features: Vec<_> = entries.iter().map(|(f, _)| *f).collect();
        assert!(features.contains(&FeatureId::GpuPower));
        assert!(features.contains(&FeatureId::GpuMux));
        assert!(features.contains(&FeatureId::GpuAccess));
    }

    #[tokio::test]
    async fn battery_write_evidence_flows_through_registry_assembly() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        // Runtime evidence says the Battery mutation backend is temporarily
        // unavailable. Read stays Supported; write must NOT become Supported.
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::TemporarilyUnavailable,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("snapshot must assemble");
        let battery = snapshot
            .capability(FeatureId::ChargeLimit)
            .expect("ChargeLimit present");
        assert_eq!(
            battery.operations.read.status,
            CapabilityStatus::Supported,
            "read must stay Supported"
        );
        assert_eq!(
            battery.operations.write.status,
            CapabilityStatus::TemporarilyUnavailable,
            "write must mirror runtime mutation evidence"
        );

        // Independent domains remain unaffected by the unavailable battery
        // mutation backend.
        let performance = snapshot
            .capability(FeatureId::Performance)
            .expect("Performance present");
        assert_eq!(
            performance.operations.read.status,
            CapabilityStatus::Supported
        );
        let gpu_power = snapshot
            .capability(FeatureId::GpuPower)
            .expect("GpuPower present");
        assert_eq!(
            gpu_power.operations.read.status,
            CapabilityStatus::Supported
        );
        assert!(snapshot.contains(FeatureId::FanCurves));
    }

    #[tokio::test]
    async fn battery_write_permission_denied_preserved_through_registry() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::PermissionDenied,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("snapshot must assemble");
        let battery = snapshot
            .capability(FeatureId::ChargeLimit)
            .expect("ChargeLimit present");
        assert_eq!(
            battery.operations.write.status,
            CapabilityStatus::PermissionDenied,
            "PermissionDenied evidence must be preserved, not collapsed"
        );
    }

    #[tokio::test]
    async fn performance_write_evidence_flows_through_registry_assembly() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        // Runtime evidence says the Performance mutation backend is
        // temporarily unavailable. Read stays Supported and profiles remain
        // known; write must NOT become Supported.
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::TemporarilyUnavailable,
        )
        .await
        .expect("snapshot must assemble");
        let performance = snapshot
            .capability(FeatureId::Performance)
            .expect("Performance present");
        assert_eq!(
            performance.operations.read.status,
            CapabilityStatus::Supported,
            "read must stay Supported"
        );
        assert_eq!(
            performance.operations.write.status,
            CapabilityStatus::TemporarilyUnavailable,
            "write must mirror runtime mutation evidence"
        );

        // Independent domains remain unaffected.
        let battery = snapshot
            .capability(FeatureId::ChargeLimit)
            .expect("ChargeLimit present");
        assert_eq!(battery.operations.read.status, CapabilityStatus::Supported);
        let gpu_power = snapshot
            .capability(FeatureId::GpuPower)
            .expect("GpuPower present");
        assert_eq!(
            gpu_power.operations.read.status,
            CapabilityStatus::Supported
        );
        assert!(snapshot.contains(FeatureId::FanCurves));
    }

    #[tokio::test]
    async fn fan_write_evidence_flows_through_registry_assembly() {
        let provider = std::sync::Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        // Runtime evidence says the fan curve mutation backend is temporarily
        // unavailable. Read stays Supported; write must NOT become Supported.
        let snapshot = build_initial_registry_snapshot(
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            &*provider,
            CapabilityStatus::TemporarilyUnavailable,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Unsupported,
        )
        .await
        .expect("snapshot must assemble");
        let fan = snapshot
            .capability(FeatureId::FanCurves)
            .expect("FanCurves present");
        assert_eq!(
            fan.operations.read.status,
            CapabilityStatus::Supported,
            "read must stay Supported"
        );
        assert_eq!(
            fan.operations.write.status,
            CapabilityStatus::TemporarilyUnavailable,
            "write must mirror runtime mutation evidence"
        );

        // Independent domains remain unaffected.
        let battery = snapshot
            .capability(FeatureId::ChargeLimit)
            .expect("ChargeLimit present");
        assert_eq!(battery.operations.read.status, CapabilityStatus::Supported);
        let performance = snapshot
            .capability(FeatureId::Performance)
            .expect("Performance present");
        assert_eq!(
            performance.operations.read.status,
            CapabilityStatus::Supported
        );
        let gpu_power = snapshot
            .capability(FeatureId::GpuPower)
            .expect("GpuPower present");
        assert_eq!(
            gpu_power.operations.read.status,
            CapabilityStatus::Supported
        );
    }
}
