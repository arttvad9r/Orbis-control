//! Application runtime composition for the interactive UI.
//!
//! This module owns construction and grouping of application services. It does
//! not define provider semantics or transport contracts.

use std::sync::Arc;

use async_trait::async_trait;
use orbis_application::{
    AppService, ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome,
    PerformanceCommandOutcome, PerformanceState, SetChargeLimitError, SetGpuModeError,
    SetPerformanceError,
};
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
}

/// All application services owned by one worker runtime.
pub struct ApplicationRuntime<G, B, R> {
    /// Grouped GPU capabilities.
    pub gpu: G,
    /// Battery service.
    pub battery: B,
    /// Performance service.
    pub performance: R,
}

impl<G, B, R> ApplicationRuntime<G, B, R> {
    /// Create an application runtime from already-built services.
    pub fn new(gpu: G, battery: B, performance: R) -> Self {
        Self {
            gpu,
            battery,
            performance,
        }
    }
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

    let battery = AppService::new(Arc::new(battery_provider));
    let performance = AppService::new(Arc::new(SessionHardwarePerformanceProvider::new(
        ZbusSessionPerformanceSource::new(session_connection),
        ZbusHardwarePerformanceSource::new(system_connection),
    )));

    Ok((
        ApplicationRuntime::new(gpu, battery, performance),
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

/// Test-only grouped runtime using one mock provider for all capabilities.
#[cfg(test)]
pub fn mock_runtime() -> ApplicationRuntime<
    GpuServices<MockProvider, MockProvider, MockProvider, MockProvider>,
    AppService<MockProvider>,
    AppService<MockProvider>,
> {
    let state = build_state_arc("zephyrus-full").expect("profile exists");
    let provider = Arc::new(MockProvider::new(state));
    let gpu = GpuServices::new(
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
        AppService::new(provider.clone()),
    );
    ApplicationRuntime::new(
        gpu,
        AppService::new(provider.clone()),
        AppService::new(provider),
    )
}

#[cfg(test)]
mod tests {
    use super::{GpuPrimitiveServices, GpuServicesRuntime};
    use orbis_application::CommandError;
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
}
