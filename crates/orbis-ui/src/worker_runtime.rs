//! Production sequential worker with worker-owned Automation observation.
//!
//! This module preserves the existing `orbis_ui::worker` public command/event
//! API while colocating Automation policy, lifecycle revision, capability
//! generation, serialization and recovery with the same owner that performs
//! application mutations. Automation execution remains promotion-gated: the
//! Performance-only executor is compiled, but the exact build must pass the
//! promotion evidence before the constant below may be changed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use crate::automation_capability::augment_snapshot_with_automation_shadow;
use crate::automation_execution_promotion::authorize_prepared_execution;
use crate::automation_performance_executor::{
    AutomationPerformanceExecutionOutcome, execute_prepared_performance,
};
use crate::automation_worker_driver::{
    AutomationPolicyRevisionError, AutomationWorkerDriver, AutomationWorkerObservation,
};
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
use orbis_session_client::{HardwareProductGpuSource, ProductGpuMutationResult};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// This exact revision has not passed executable Cargo/Clippy/Slint validation
/// in the available environment. Keep unattended mutation unreachable even if
/// an external registry accidentally advertises Automation write support.
const AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED: bool = false;
const AUTOMATION_MAX_CAPABILITY_AGE: Duration = Duration::from_secs(45);

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

#[derive(Debug, Clone, Copy)]
struct AutomationLifecycleSignal {
    start: bool,
    observed_at: SystemTime,
}

#[derive(Clone)]
struct LifecycleRegistration {
    id: u64,
    sender: UnboundedSender<AutomationLifecycleSignal>,
}

static NEXT_LIFECYCLE_REGISTRATION: AtomicU64 = AtomicU64::new(1);
static LIFECYCLE_REGISTRATION: OnceLock<Mutex<Option<LifecycleRegistration>>> = OnceLock::new();

fn lifecycle_registration() -> &'static Mutex<Option<LifecycleRegistration>> {
    LIFECYCLE_REGISTRATION.get_or_init(|| Mutex::new(None))
}

struct LifecycleRegistrationGuard {
    id: u64,
}

impl Drop for LifecycleRegistrationGuard {
    fn drop(&mut self) {
        let Ok(mut slot) = lifecycle_registration().lock() else {
            return;
        };
        if slot.as_ref().map(|registration| registration.id) == Some(self.id) {
            *slot = None;
        }
    }
}

fn register_lifecycle_sender(
    sender: UnboundedSender<AutomationLifecycleSignal>,
) -> LifecycleRegistrationGuard {
    let id = NEXT_LIFECYCLE_REGISTRATION.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut slot) = lifecycle_registration().lock() {
        *slot = Some(LifecycleRegistration { id, sender });
    } else {
        tracing::error!(
            "Automation lifecycle sender registry poisoned; resume automation disabled"
        );
    }
    LifecycleRegistrationGuard { id }
}

/// Publish one already-observed logind `PrepareForSleep(bool)` event to the
/// current worker owner. No hardware action occurs here; the worker consumes the
/// signal in FIFO/select order with commands and capability replacement.
///
/// Returns false when no worker is registered or its lifecycle receiver closed.
pub fn publish_prepare_for_sleep(start: bool, observed_at: SystemTime) -> bool {
    let sender = match lifecycle_registration().lock() {
        Ok(slot) => slot
            .as_ref()
            .map(|registration| registration.sender.clone()),
        Err(_) => None,
    };
    let Some(sender) = sender else {
        return false;
    };
    sender
        .send(AutomationLifecycleSignal { start, observed_at })
        .is_ok()
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

fn sync_persisted_automation_policy(driver: &mut AutomationWorkerDriver) {
    match orbis_config::load_automation_policy() {
        Ok(policy) => {
            if driver.persisted_policy() == Some(&policy) {
                return;
            }
            if let Err(error) = driver.replace_persisted_policy(policy) {
                tracing::error!(
                    ?error,
                    "Automation policy revision exhausted; execution disabled"
                );
            }
        }
        Err(error) => {
            if driver.persisted_policy().is_some() {
                match driver.clear_persisted_policy() {
                    Ok(revision) => tracing::warn!(
                        error = %error,
                        policy_revision = revision.get(),
                        "Automation persisted policy became unavailable; cached intent cleared"
                    ),
                    Err(AutomationPolicyRevisionError::SequenceExhausted) => tracing::error!(
                        error = %error,
                        "Automation policy unavailable and revision exhausted"
                    ),
                }
            } else {
                tracing::debug!(error = %error, "Automation persisted policy unavailable");
            }
        }
    }
}

async fn observe_automation_telemetry<R>(
    driver: &mut AutomationWorkerDriver,
    performance: &R,
    source_capabilities: &orbis_capabilities::CapabilityRegistrySnapshot,
    telemetry: &orbis_core::telemetry::Telemetry,
    allow_execution: bool,
) where
    R: PerformanceServiceRuntime + Sync,
{
    sync_persisted_automation_policy(driver);

    let capabilities = match augment_snapshot_with_automation_shadow(source_capabilities) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::error!(
                ?error,
                "Automation capability view failed; observation skipped fail-closed"
            );
            return;
        }
    };
    let now = SystemTime::now();
    let observation = driver.observe_telemetry(telemetry, &capabilities, now);
    match observation {
        AutomationWorkerObservation::PolicyUnavailable
        | AutomationWorkerObservation::NoConfirmedEvent => return,
        AutomationWorkerObservation::Failed(error) => {
            tracing::error!(?error, "Automation worker observation failed closed");
            return;
        }
        AutomationWorkerObservation::Confirmed(event) => {
            tracing::debug!(
                revision = event.revision().get(),
                outcome = ?event.outcome(),
                "Automation lifecycle event confirmed under worker owner"
            );
        }
    }

    let envelope =
        match driver.prepare_latest_dry_run(&capabilities, now, AUTOMATION_MAX_CAPABILITY_AGE) {
            Ok(envelope) => envelope,
            Err(block) => {
                tracing::debug!(?block, "Automation dry-run preparation blocked");
                return;
            }
        };

    // This is the production dry-run boundary. Until the exact build is
    // promoted, even a future Supported Automation capability cannot reach the
    // owner call. The move-only envelope is still consumed exactly once.
    if !allow_execution {
        if let Err(error) = driver.finish_dry_run(envelope) {
            tracing::error!(?error, "Resume dry-run lease release failed");
        }
        return;
    }

    if !AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED {
        tracing::info!(
            policy_revision = envelope.required_policy_revision().get(),
            lifecycle_revision = envelope.prepared().lease().required_revision().get(),
            generation = envelope.prepared().lease().required_generation(),
            actions = ?envelope.prepared().lease().actions(),
            "Automation dry-run prepared; unattended mutation promotion disabled"
        );
        if let Err(error) = driver.finish_dry_run(envelope) {
            tracing::error!(?error, "Automation dry-run lease release failed");
        }
        return;
    }

    // The compile-time promotion bit is necessary but not sufficient. The same
    // immutable generation must still be fresh and explicitly advertise direct
    // Automation write support. Shadow/dry-run only removes this one runtime
    // blocker; action-level evidence has already been checked twice under the
    // exact same generation.
    let permit = match authorize_prepared_execution(
        &envelope,
        &capabilities,
        SystemTime::now(),
        AUTOMATION_MAX_CAPABILITY_AGE,
    ) {
        Ok(permit) => permit,
        Err(block) => {
            tracing::warn!(?block, "Automation strict execution promotion blocked");
            if let Err(error) = driver.finish_dry_run(envelope) {
                tracing::error!(?error, "Automation blocked lease release failed");
            }
            return;
        }
    };
    debug_assert_eq!(permit.required_generation(), capabilities.generation());

    let outcome = execute_prepared_performance(
        performance,
        &envelope,
        driver.current_policy_revision(),
        driver.current_revision(),
        permit.required_generation(),
    )
    .await;

    match outcome {
        AutomationPerformanceExecutionOutcome::NoOp => {
            if let Err(error) = driver.finish_dry_run(envelope) {
                tracing::error!(?error, "Automation no-op lease release failed");
            }
        }
        AutomationPerformanceExecutionOutcome::Applied {
            lease_id,
            revision,
            generation,
            profile,
        } => {
            tracing::info!(
                lease_id,
                revision = revision.get(),
                generation,
                ?profile,
                "Automation Performance mutation confirmed"
            );
            if let Err(error) = driver.finish_dry_run(envelope) {
                tracing::error!(?error, "Automation applied lease release failed");
            }
        }
        AutomationPerformanceExecutionOutcome::DefiniteFailure(error) => {
            tracing::warn!(
                ?error,
                "Automation Performance execution failed before unknown outcome"
            );
            if let Err(finish_error) = driver.finish_dry_run(envelope) {
                tracing::error!(?finish_error, "Automation failed lease release failed");
            }
        }
        AutomationPerformanceExecutionOutcome::RecoveryRequired { requested, reason } => {
            tracing::error!(
                ?requested,
                ?reason,
                "Automation Performance outcome unknown; recovery required"
            );
            if let Err(error) = driver.finish_performance_unknown(envelope) {
                tracing::error!(?error, "Automation could not enter recovery barrier");
                return;
            }
            match bounded_performance_state(performance).await {
                Ok(state) => {
                    let recovered = driver.reconcile_performance(&state);
                    tracing::warn!(
                        ?recovered,
                        "Automation Performance recovery reconciled from fresh read"
                    );
                }
                Err(error) => {
                    tracing::error!(
                        ?error,
                        "Automation Performance recovery read failed; barrier remains active"
                    );
                }
            }
        }
    }
}

fn reconcile_automation_from_performance_result(
    driver: &mut AutomationWorkerDriver,
    result: &Result<PerformanceState, ProviderError>,
) {
    if let Ok(state) = result {
        let outcome = driver.reconcile_performance(state);
        if !matches!(
            outcome,
            crate::automation_recovery::AutomationPerformanceRecoveryOutcome::NotRequired
        ) {
            tracing::warn!(
                ?outcome,
                "Automation recovery cleared by authoritative Performance refresh"
            );
        }
    }
}

/// Reconcile read-only state after a paired resume observation.
///
/// This publishes fresh provider results and a new capability generation. It
/// deliberately does not dispatch or restore any hardware mutation.
async fn reconcile_after_resume<G, B, R, F>(
    runtime: &mut ApplicationRuntime<G, B, R>,
    automation: &mut AutomationWorkerDriver,
    emit: &mut F,
) where
    G: GpuServicesRuntime,
    B: BatteryServiceRuntime,
    R: PerformanceServiceRuntime + Sync,
    F: FnMut(WorkerEvent),
{
    match refresh_capability_registry(runtime).await {
        Ok(snapshot) => {
            let generation = snapshot.generation();
            let snapshot = Arc::new(snapshot);
            runtime.replace_capabilities((*snapshot).clone());
            emit(WorkerEvent::RegistryChange(Ok((generation, snapshot))));
        }
        Err(error) => emit(WorkerEvent::RegistryChange(Err(error))),
    }

    let (power, mux, access) = bounded_gpu_capabilities(&runtime.gpu).await;
    emit(WorkerEvent::GpuPowerRefresh(power));
    emit(WorkerEvent::GpuMuxRefresh(mux));
    emit(WorkerEvent::GpuAccessRefresh(access));

    let performance = bounded_performance_state(&runtime.performance).await;
    reconcile_automation_from_performance_result(automation, &performance);
    emit(WorkerEvent::PerformanceRefresh(performance));
    emit(WorkerEvent::ChargeLimitRefresh(
        bounded_charge_limit(&runtime.battery).await,
    ));

    let telemetry = runtime.telemetry.snapshot().await;
    if let Err(error) = &telemetry {
        // Canonical provider identity and declared deadline flow into the
        // diagnostic path (#123): failures name the backend and its contract
        // deadline instead of an anonymous read.
        tracing::warn!(
            provider = runtime.telemetry.provider_id(),
            deadline_ms = runtime.telemetry.snapshot_timeout().as_millis() as u64,
            "telemetry refresh failed: {error:?}"
        );
    }
    if let Ok(sample) = &telemetry {
        let capabilities = runtime.capabilities().clone();
        observe_automation_telemetry(
            automation,
            &runtime.performance,
            &capabilities,
            sample,
            false,
        )
        .await;
    }
    emit(WorkerEvent::TelemetryRefresh(telemetry));
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
    let mut automation = AutomationWorkerDriver::new();
    sync_persisted_automation_policy(&mut automation);

    let (lifecycle_tx, mut lifecycle_rx) = tokio::sync::mpsc::unbounded_channel();
    let _lifecycle_registration = register_lifecycle_sender(lifecycle_tx);

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
                        lifecycle = lifecycle_rx.recv() => {
                            if let Some(lifecycle) = lifecycle {
                                let outcome = automation.observe_prepare_for_sleep(
                                    lifecycle.start,
                                    lifecycle.observed_at,
                                );
                                tracing::debug!(
                                    start = lifecycle.start,
                                    outcome = ?outcome,
                                    "Automation lifecycle signal consumed by worker"
                                );
                                if !lifecycle.start {
                                    reconcile_after_resume(&mut runtime, &mut automation, &mut emit)
                                        .await;
                                }
                            }
                            continue;
                        }
                        snapshot = snapshot_rx.recv(), if poll_in_progress => {
                            if let Some(snapshot) = snapshot {
                                if let Ok(telemetry) = &snapshot {
                                    let capabilities = runtime.capabilities().clone();
                                    observe_automation_telemetry(
                                        &mut automation,
                                        &runtime.performance,
                                        &capabilities,
                                        telemetry,
                                        true,
                                    ).await;
                                }
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
                        lifecycle = lifecycle_rx.recv() => {
                            if let Some(lifecycle) = lifecycle {
                                let outcome = automation.observe_prepare_for_sleep(
                                    lifecycle.start,
                                    lifecycle.observed_at,
                                );
                                tracing::debug!(start = lifecycle.start, outcome = ?outcome, "Automation lifecycle signal consumed by worker");
                                if !lifecycle.start {
                                    reconcile_after_resume(&mut runtime, &mut automation, &mut emit)
                                        .await;
                                }
                            }
                            continue;
                        }
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
            if let Ok(telemetry) = &snapshot {
                let capabilities = runtime.capabilities().clone();
                observe_automation_telemetry(
                    &mut automation,
                    &runtime.performance,
                    &capabilities,
                    telemetry,
                    true,
                )
                .await;
            }
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
                reconcile_automation_from_performance_result(&mut automation, &result);
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
    use super::*;

    fn production_source() -> &'static str {
        include_str!("worker_runtime.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("worker source always has a production prefix")
    }

    #[test]
    fn execution_promotion_is_fail_closed_on_this_revision() {
        const {
            assert!(!AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED);
        }
    }

    #[test]
    fn worker_source_owns_automation_and_only_performance_executor() {
        let source = production_source();
        assert!(source.contains("AutomationWorkerDriver::new()"));
        assert!(source.contains("observe_prepare_for_sleep"));
        assert!(source.contains("observe_automation_telemetry"));
        assert!(source.contains("authorize_prepared_execution"));
        assert!(source.contains("execute_prepared_performance"));
        assert!(source.contains("finish_performance_unknown"));
        assert!(source.contains("reconcile_performance"));
        assert!(source.contains("reconcile_after_resume"));
        assert!(source.contains("sample,\n            false,"));
        assert!(!source.contains("set_gpu_mode_for_automation"));
        assert!(!source.contains("set_fan_curve_for_automation"));
        assert!(!source.contains("set_charge_limit_for_automation"));
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

    #[test]
    fn lifecycle_publish_fails_closed_without_registered_worker() {
        if let Ok(mut slot) = lifecycle_registration().lock() {
            *slot = None;
        }
        assert!(!publish_prepare_for_sleep(false, SystemTime::UNIX_EPOCH));
    }
}
