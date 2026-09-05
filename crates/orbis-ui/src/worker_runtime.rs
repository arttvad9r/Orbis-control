//! Production sequential worker for hardware state and mutations.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::composition::{
    ApplicationRuntime, BatteryServiceRuntime, FanServiceRuntime, GpuServicesRuntime,
    PerformanceServiceRuntime,
};
use orbis_application::{
    ChargeLimitCommandOutcome, GpuCommandOutcome, PerformanceCommandOutcome, PerformanceState,
    SetChargeLimitError, SetFanDefaultsError, SetGpuModeError, SetPerformanceError,
};
use orbis_core::action::ApplyResult;
use orbis_core::fan::{FanCurve, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::{AsusdFanProfile, PerformanceProfile};
use orbis_providers::bounded_provider_call;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::FanCurvePoints;
use orbis_session_client::{HardwareProductGpuSource, ProductGpuMutationResult, ProductGpuStatus};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Typed command accepted by the single sequential worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerCommand {
    SetPerformance(PerformanceProfile),
    SetGpuMode {
        mode: GpuMode,
        confirmed: bool,
    },
    /// Queue an ASUS product GPU mode through the typed Hardware1 operation.
    ///
    /// `raw` is the exact product wire target (0=Hybrid, 1=Integrated,
    /// 2=Ultimate); validation happens at the Hardware1 boundary.
    SetProductGpuMode {
        raw: u32,
    },
    SetChargeLimit {
        percent: u8,
    },
    RefreshChargeLimit,
    RefreshGpuCapabilities,
    RefreshProductGpuStatus,
    RefreshPerformance,
    RefreshCapabilities,
    RefreshTelemetry,
    SetFanCurve {
        profile: AsusdFanProfile,
        fan: FanId,
        curve: FanCurvePoints,
    },
    RefreshFanCurve {
        profile: AsusdFanProfile,
        fan: FanId,
    },
    ResetFanCurvesToDefaults {
        profile: AsusdFanProfile,
    },
}

/// Typed result emitted to the presentation boundary.
#[derive(Debug)]
pub enum WorkerEvent {
    Performance(Result<PerformanceCommandOutcome, SetPerformanceError>),
    Gpu(Result<GpuCommandOutcome, SetGpuModeError>),
    /// Authoritative result of the ASUS product GPU queue operation.
    ProductGpu(Result<ProductGpuMutationResult, ProviderError>),
    ProductGpuStatus(Result<ProductGpuStatus, ProviderError>),
    ChargeLimit(Result<ChargeLimitCommandOutcome, SetChargeLimitError>),
    ChargeLimitRefresh(Result<orbis_core::battery::ChargeLimit, ProviderError>),
    GpuPowerRefresh(Result<GpuPowerState, ProviderError>),
    GpuMuxRefresh(Result<GpuMuxState, ProviderError>),
    GpuAccessRefresh(Result<GpuAccessPolicy, ProviderError>),
    PerformanceRefresh(Result<PerformanceState, ProviderError>),
    RegistryChange(
        Result<
            (u64, Arc<orbis_capabilities::CapabilityRegistrySnapshot>),
            orbis_capabilities::ProbeError,
        >,
    ),
    TelemetryRefresh(Result<orbis_core::telemetry::Telemetry, ProviderError>),
    FanCurve(Result<ApplyResult, ProviderError>),
    FanCurveRefresh {
        profile: AsusdFanProfile,
        result: Result<FanCurve, ProviderError>,
    },
    FanCurveDefaults {
        profile: AsusdFanProfile,
        result: Result<ApplyResult, SetFanDefaultsError>,
    },
}

pub fn command_channel() -> (
    UnboundedSender<WorkerCommand>,
    UnboundedReceiver<WorkerCommand>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

pub async fn run_worker<G, B, R, F>(
    runtime: ApplicationRuntime<G, B, R>,
    receiver: UnboundedReceiver<WorkerCommand>,
    emit: F,
) where
    G: GpuServicesRuntime + 'static,
    B: BatteryServiceRuntime + 'static,
    R: PerformanceServiceRuntime + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    run_worker_with_product_gpu(runtime, receiver, emit, None, None).await;
}

pub async fn run_worker_with_polling<G, B, R, F>(
    runtime: ApplicationRuntime<G, B, R>,
    receiver: UnboundedReceiver<WorkerCommand>,
    emit: F,
    poll_interval: Duration,
) where
    G: GpuServicesRuntime + 'static,
    B: BatteryServiceRuntime + 'static,
    R: PerformanceServiceRuntime + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    run_worker_with_product_gpu(runtime, receiver, emit, Some(poll_interval), None).await;
}

/// Worker entry point with an optional ASUS product GPU Hardware1 source.
///
/// The source is composed by the production main so the original application
/// caller identity is preserved. `None` keeps the operation fail-closed:
/// the command is answered with a typed `Unsupported` error instead of a
/// simulated success.
pub async fn run_worker_with_product_gpu<G, B, R, F>(
    runtime: ApplicationRuntime<G, B, R>,
    receiver: UnboundedReceiver<WorkerCommand>,
    emit: F,
    poll_interval: Option<Duration>,
    product_gpu: Option<std::sync::Arc<dyn HardwareProductGpuSource>>,
) where
    G: GpuServicesRuntime + 'static,
    B: BatteryServiceRuntime + 'static,
    R: PerformanceServiceRuntime + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    run_worker_inner(runtime, receiver, emit, poll_interval, product_gpu).await;
}

async fn bounded_performance_state<R>(performance: &R) -> Result<PerformanceState, ProviderError>
where
    R: PerformanceServiceRuntime + ?Sized,
{
    let provider = performance.provider_performance();
    let current = bounded_provider_call(
        provider,
        "performance.current_profile",
        provider.current_profile(),
    )
    .await?;
    let available =
        bounded_provider_call(provider, "performance.profiles", provider.profiles()).await?;
    Ok(PerformanceState { current, available })
}

async fn bounded_charge_limit<B>(
    battery: &B,
) -> Result<orbis_core::battery::ChargeLimit, ProviderError>
where
    B: BatteryServiceRuntime + ?Sized,
{
    let provider = battery.provider_battery();
    bounded_provider_call(provider, "battery.charge_limit", provider.charge_limit()).await
}

async fn bounded_gpu_capabilities<G>(
    gpu: &G,
) -> (
    Result<GpuPowerState, ProviderError>,
    Result<GpuMuxState, ProviderError>,
    Result<GpuAccessPolicy, ProviderError>,
)
where
    G: GpuServicesRuntime + ?Sized,
{
    let power = gpu.provider_power();
    let mux = gpu.provider_mux();
    let access = gpu.provider_access();
    tokio::join!(
        bounded_provider_call(power, "gpu.power_state", power.power_state()),
        bounded_provider_call(mux, "gpu.mux_state", mux.mux_state()),
        bounded_provider_call(access, "gpu.access_policy", access.access_policy()),
    )
}

async fn bounded_fan_curve(
    fan_service: &dyn FanServiceRuntime,
    profile: AsusdFanProfile,
    fan: &FanId,
) -> Result<FanCurve, ProviderError> {
    let provider = fan_service.provider_fan();
    bounded_provider_call(
        provider,
        "fan.fan_curve_for_profile",
        provider.fan_curve_for_profile(profile, fan),
    )
    .await
}

async fn run_worker_inner<G, B, R, F>(
    mut runtime: ApplicationRuntime<G, B, R>,
    mut receiver: UnboundedReceiver<WorkerCommand>,
    mut emit: F,
    poll_interval: Option<Duration>,
    product_gpu: Option<std::sync::Arc<dyn HardwareProductGpuSource>>,
) where
    G: GpuServicesRuntime + 'static,
    B: BatteryServiceRuntime + 'static,
    R: PerformanceServiceRuntime + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    let mut deferred_command: Option<WorkerCommand> = None;
    let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::unbounded_channel::<
        Result<orbis_core::telemetry::Telemetry, ProviderError>,
    >();
    let mut poll_in_progress = false;
    let mut poll_timer = None;
    if let Some(interval) = poll_interval {
        let mut timer = tokio::time::interval(interval);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        timer.tick().await;
        poll_timer = Some(timer);
    }

    const GPU_REFRESH_INTERVAL: u32 = 5;
    const CAPABILITY_REFRESH_INTERVAL: u32 = 30;
    let mut gpu_refresh_counter = 0_u32;
    let mut capability_refresh_counter = 0_u32;

    loop {
        let command = match deferred_command.take() {
            Some(command) => Some(command),
            None => match &mut poll_timer {
                Some(timer) => {
                    tokio::select! {
                        command = receiver.recv() => command,
                        snapshot = snapshot_rx.recv(), if poll_in_progress => {
                            if let Some(snapshot) = snapshot {
                                emit(WorkerEvent::TelemetryRefresh(snapshot));
                                poll_in_progress = false;
                            }
                            continue;
                        }
                        _ = timer.tick(), if !poll_in_progress => {
                            let tx = snapshot_tx.clone();
                            let telemetry = runtime.telemetry.clone();
                            tokio::spawn(async move {
                                let _ = tx.send(telemetry.snapshot().await);
                            });
                            poll_in_progress = true;

                            gpu_refresh_counter += 1;
                            if gpu_refresh_counter >= GPU_REFRESH_INTERVAL {
                                gpu_refresh_counter = 0;
                                let (power, mux, access) = bounded_gpu_capabilities(&runtime.gpu).await;
                                emit(WorkerEvent::GpuPowerRefresh(power));
                                emit(WorkerEvent::GpuMuxRefresh(mux));
                                emit(WorkerEvent::GpuAccessRefresh(access));
                            }

                            capability_refresh_counter += 1;
                            if capability_refresh_counter >= CAPABILITY_REFRESH_INTERVAL {
                                capability_refresh_counter = 0;
                                match refresh_capability_registry(&mut runtime).await {
                                    Ok(snapshot) => {
                                        let generation = snapshot.generation();
                                        let snapshot = Arc::new(snapshot);
                                        runtime.replace_capabilities((*snapshot).clone());
                                        emit(WorkerEvent::RegistryChange(Ok((generation, snapshot))));
                                    }
                                    Err(error) => emit(WorkerEvent::RegistryChange(Err(error))),
                                }
                            }
                            continue;
                        }
                    }
                }
                None => {
                    tokio::select! {
                        command = receiver.recv() => command,
                    }
                }
            },
        };

        let Some(command) = command else {
            return;
        };

        if matches!(command, WorkerCommand::RefreshCapabilities) {
            match refresh_capability_registry(&mut runtime).await {
                Ok(snapshot) => {
                    let generation = snapshot.generation();
                    let snapshot = Arc::new(snapshot);
                    runtime.replace_capabilities((*snapshot).clone());
                    emit(WorkerEvent::RegistryChange(Ok((generation, snapshot))));
                }
                Err(error) => emit(WorkerEvent::RegistryChange(Err(error))),
            }
            continue;
        }

        if matches!(command, WorkerCommand::RefreshTelemetry) {
            let snapshot = runtime.telemetry.snapshot().await;
            emit(WorkerEvent::TelemetryRefresh(snapshot));
            continue;
        }

        let event = match command {
            WorkerCommand::SetPerformance(profile) => {
                WorkerEvent::Performance(runtime.performance.set_performance(profile).await)
            }
            WorkerCommand::SetGpuMode { mode, confirmed } => {
                WorkerEvent::Gpu(runtime.gpu.set_gpu_mode(mode, confirmed).await)
            }
            WorkerCommand::SetProductGpuMode { raw } => {
                let result = match product_gpu.as_deref() {
                    Some(source) => source.set_product_gpu_mode(raw).await,
                    None => Err(ProviderError::Unsupported(
                        "ASUS product GPU mutation is not promoted on this build".into(),
                    )),
                };
                WorkerEvent::ProductGpu(result)
            }
            WorkerCommand::RefreshProductGpuStatus => {
                let result = match product_gpu.as_deref() {
                    Some(source) => source.product_gpu_status().await,
                    None => Err(ProviderError::Unsupported(
                        "ASUS product GPU status source unavailable".into(),
                    )),
                };
                WorkerEvent::ProductGpuStatus(result)
            }
            WorkerCommand::SetChargeLimit { percent } => {
                let mut latest_percent = percent;
                loop {
                    match receiver.try_recv() {
                        Ok(WorkerCommand::SetChargeLimit { percent: next }) => {
                            latest_percent = next;
                        }
                        Ok(other) => {
                            deferred_command = Some(other);
                            break;
                        }
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }
                WorkerEvent::ChargeLimit(runtime.battery.set_charge_limit(latest_percent).await)
            }
            WorkerCommand::RefreshChargeLimit => {
                WorkerEvent::ChargeLimitRefresh(bounded_charge_limit(&runtime.battery).await)
            }
            WorkerCommand::RefreshGpuCapabilities => {
                let (power, mux, access) = bounded_gpu_capabilities(&runtime.gpu).await;
                emit(WorkerEvent::GpuPowerRefresh(power));
                emit(WorkerEvent::GpuMuxRefresh(mux));
                emit(WorkerEvent::GpuAccessRefresh(access));
                continue;
            }
            WorkerCommand::RefreshPerformance => {
                let result = bounded_performance_state(&runtime.performance).await;
                WorkerEvent::PerformanceRefresh(result)
            }
            WorkerCommand::SetFanCurve {
                profile,
                fan,
                curve,
            } => WorkerEvent::FanCurve(runtime.fan.set_fan_curve(profile, fan, curve).await),
            WorkerCommand::RefreshFanCurve { profile, fan } => WorkerEvent::FanCurveRefresh {
                profile,
                result: bounded_fan_curve(runtime.fan.as_ref(), profile, &fan).await,
            },
            WorkerCommand::ResetFanCurvesToDefaults { profile } => WorkerEvent::FanCurveDefaults {
                profile,
                result: runtime.fan.reset_fan_curves_to_defaults(profile).await,
            },
            WorkerCommand::RefreshCapabilities | WorkerCommand::RefreshTelemetry => {
                unreachable!("handled before service dispatch")
            }
        };
        emit(event);
    }
}

/// Canonical capability refresh path used by both explicit and periodic refresh.
///
/// Mutation-owner/status evidence is always re-queried before the next
/// generation is built. This keeps explicit `RefreshCapabilities` and periodic
/// refresh semantically identical and prevents publication from stale cached
/// write evidence.
async fn refresh_capability_registry<G, B, R>(
    runtime: &mut ApplicationRuntime<G, B, R>,
) -> Result<orbis_capabilities::CapabilityRegistrySnapshot, orbis_capabilities::ProbeError>
where
    G: GpuServicesRuntime,
    B: BatteryServiceRuntime,
    R: PerformanceServiceRuntime,
{
    runtime.requery_mutation_statuses().await;
    let next_generation = runtime.capabilities().generation() + 1;
    crate::composition::probe_capability_registry(
        runtime.battery.provider_battery(),
        runtime.performance.provider_performance(),
        runtime.gpu.provider_power(),
        runtime.gpu.provider_mux(),
        runtime.gpu.provider_access(),
        runtime.fan.provider_fan(),
        runtime.fan_mutation_status(),
        runtime.battery_mutation_status(),
        runtime.performance_mutation_status(),
        next_generation,
        SystemTime::now(),
    )
    .await
}

#[cfg(test)]
mod tests {
    fn production_source() -> &'static str {
        include_str!("worker_runtime.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("worker source always has a production prefix")
    }

    #[test]
    fn worker_source_has_no_automation_runtime_hooks() {
        let source = production_source();
        assert!(!source.contains("AutomationWorkerDriver"));
        assert!(!source.contains("publish_prepare_for_sleep"));
        assert!(!source.contains("observe_automation_telemetry"));
        assert!(!source.contains("load_automation_policy"));
    }

    #[test]
    fn product_gpu_request_is_fifo_only_and_never_automated() {
        let source = production_source();
        assert!(source.contains("WorkerCommand::SetProductGpuMode { raw }"));
        assert!(source.contains("source.set_product_gpu_mode(raw).await"));
        assert!(!source.contains("set_product_gpu_mode_for_automation"));
    }

    #[test]
    fn capability_refresh_has_one_canonical_status_requery_path() {
        let source = production_source();
        assert_eq!(
            source
                .matches("runtime.requery_mutation_statuses().await")
                .count(),
            1,
            "mutation status re-query must have one canonical owner"
        );
        assert_eq!(
            source
                .matches("refresh_capability_registry(&mut runtime).await")
                .count(),
            2,
            "periodic and explicit refresh must use the same helper"
        );
        assert!(!source.contains("run_capability_refresh"));
    }

    #[test]
    fn worker_authoritative_reads_use_bounded_helpers_where_provider_identity_exists() {
        let source = production_source();
        assert!(!source.contains("runtime.gpu.refresh_gpu_capabilities().await"));
        assert!(!source.contains("runtime.battery.charge_limit().await"));
        assert!(!source.contains("runtime.performance.performance_state().await"));
        assert!(!source.contains("runtime.fan.fan_curve_for_profile"));
        assert!(source.contains("bounded_gpu_capabilities(&runtime.gpu).await"));
        assert!(source.contains("bounded_charge_limit(&runtime.battery).await"));
        assert!(source.contains("bounded_performance_state(&runtime.performance).await"));
        assert!(source.contains("bounded_fan_curve(runtime.fan.as_ref(), profile, &fan).await"));
    }
}
