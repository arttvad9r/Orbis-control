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
use crate::profile_limits_runtime::{ProfileLimitsTracker, ProfileLimitsView};
use orbis_application::{
    ChargeLimitCommandOutcome, GpuCommandOutcome, PerformanceCommandOutcome, PerformanceState,
    SetChargeLimitError, SetFanCurveError, SetFanDefaultsError, SetGpuModeError,
    SetPerformanceError,
};
use orbis_core::action::ApplyResult;
use orbis_core::fan::{FanCurve, FanId};
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::{AsusdFanProfile, PerformanceProfile};
use orbis_providers::error::ProviderError;
use orbis_providers::traits::FanCurvePoints;
use orbis_providers::{bounded_operation, bounded_provider_call};
use orbis_session_client::{HardwareProductGpuSource, ProductGpuMutationResult};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// This exact revision has not passed executable Cargo/Clippy/Slint validation
/// in the available environment. Keep unattended mutation unreachable even if
/// an external registry accidentally advertises Automation write support.
const AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED: bool = false;
const AUTOMATION_MAX_CAPABILITY_AGE: Duration = Duration::from_secs(45);

/// Deadline for one read-only ASUS product GPU status query.
///
/// The query is read-only evidence: a hung asusd peer must not stall the single
/// sequential worker, and a timeout is never reported as success. The next
/// periodic refresh simply re-queries.
const PRODUCT_GPU_STATUS_DEADLINE: Duration = Duration::from_secs(2);

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
    /// Apply one typed SPL/SPPT/FPPT value through Hardware1.
    SetPowerLimit {
        field: orbis_core::limits::PowerLimitField,
        value: i32,
        snapshot_identity: u64,
    },
    RefreshChargeLimit,
    RefreshGpuCapabilities,
    RefreshPerformance,
    RefreshCapabilities,
    RefreshTelemetry,
    /// Authoritative read-only power/thermal limit snapshot.
    RefreshPowerLimits,
    /// Authoritative read-only product GPU status (current/queued/reboot).
    RefreshProductGpuStatus,
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
    /// Re-apply the stored power limits whenever the active profile changes.
    SetProfileLimitsAutoApply {
        enabled: bool,
    },
}

/// Typed result emitted to the presentation boundary.
#[derive(Debug)]
pub enum WorkerEvent {
    Performance(Result<PerformanceCommandOutcome, SetPerformanceError>),
    Gpu(Result<GpuCommandOutcome, SetGpuModeError>),
    /// Authoritative result of the ASUS product GPU queue operation.
    ProductGpu(Result<ProductGpuMutationResult, ProviderError>),
    /// Authoritative read-only product GPU status (current/queued/reboot).
    ///
    /// Distinguishes "the status read itself failed" from a mutation outcome:
    /// a failure here must degrade the product-mode section to an honest
    /// unavailable state instead of inventing a mode.
    ProductGpuStatusRefresh(Result<ProductGpuMutationResult, ProviderError>),
    ChargeLimit(Result<ChargeLimitCommandOutcome, SetChargeLimitError>),
    /// Result of a typed power-limit mutation, labelled with its field.
    PowerLimit {
        field: orbis_core::limits::PowerLimitField,
        result: Result<ApplyResult, ProviderError>,
    },
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
    /// Result of an authoritative read-only power-limit refresh.
    PowerLimitsRefresh(Result<orbis_core::limits::PowerLimitSnapshot, ProviderError>),
    FanCurve(Result<ApplyResult, SetFanCurveError>),
    FanCurveRefresh {
        profile: AsusdFanProfile,
        result: Result<FanCurve, ProviderError>,
    },
    FanCurveDefaults {
        profile: AsusdFanProfile,
        result: Result<ApplyResult, SetFanDefaultsError>,
    },
    /// Stored power-limit intent for the active performance profile.
    ProfileLimits(ProfileLimitsView),
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
    run_worker_inner(
        runtime,
        receiver,
        emit,
        poll_interval,
        product_gpu,
        ProfileLimitsTracker::load(),
    )
    .await;
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

/// One bounded read-only ASUS product GPU status query.
///
/// `bounded_operation` is the deadline boundary for this non-`Provider`
/// adapter: a hung asusd peer yields a timeout, never a fabricated mode. The
/// result is published unchanged so the presentation boundary can distinguish
/// a failed read from a mutation outcome.
async fn product_gpu_status_read(
    product_gpu: Option<&dyn HardwareProductGpuSource>,
) -> Result<ProductGpuMutationResult, ProviderError> {
    match product_gpu {
        Some(source) => {
            bounded_operation(
                PRODUCT_GPU_STATUS_DEADLINE,
                "hardware1.product_gpu_status",
                "product_gpu_status",
                source.product_gpu_status(),
            )
            .await
        }
        None => Err(ProviderError::Unsupported(
            "ASUS product GPU status is not promoted on this build".into(),
        )),
    }
}

/// One authorised power-limit write followed by an authoritative read-back
/// refresh. `expected_identity` guards manual applies against a changed snapshot.
async fn write_power_limit<F>(
    provider: Option<&Arc<dyn orbis_providers::traits::PowerLimitProvider>>,
    field: orbis_core::limits::PowerLimitField,
    value: i32,
    expected_identity: Option<u64>,
    emit: &mut F,
) -> Result<ApplyResult, ProviderError>
where
    F: FnMut(WorkerEvent),
{
    let result = match provider {
        Some(provider) => {
            bounded_provider_call(provider.as_ref(), "power_limits.set", async {
                let snapshot = provider.power_limit_snapshot().await?;
                if expected_identity.is_some_and(|identity| snapshot.identity != identity) {
                    return Err(ProviderError::Conflict(
                        "power-limit snapshot changed; refresh required".into(),
                    ));
                }
                provider
                    .set_power_limit_from_snapshot(&snapshot, field.clone(), value)
                    .await
            })
            .await
        }
        None => Err(ProviderError::BackendUnavailable(
            "power-limit provider unavailable".into(),
        )),
    };
    if result.is_ok() || matches!(result, Err(ProviderError::Timeout(_))) {
        let refresh = match provider {
            Some(provider) => {
                bounded_provider_call(
                    provider.as_ref(),
                    "power_limits.read_back",
                    provider.power_limit_snapshot(),
                )
                .await
            }
            None => Err(ProviderError::Unsupported(
                "power-limit provider unavailable".into(),
            )),
        };
        emit(WorkerEvent::PowerLimitsRefresh(refresh));
    }
    result
}

/// Feed an authoritative profile observation to the tracker and, when the
/// profile changed and auto-apply is on, replay the stored limits (a profile
/// switch resets them in firmware). Stops at the first failure and never
/// retries a write whose outcome is unknown.
async fn observe_profile_limits<G, B, R, F>(
    runtime: &ApplicationRuntime<G, B, R>,
    tracker: &mut ProfileLimitsTracker,
    profile: PerformanceProfile,
    emit: &mut F,
) where
    G: GpuServicesRuntime,
    B: BatteryServiceRuntime,
    R: PerformanceServiceRuntime,
    F: FnMut(WorkerEvent),
{
    let before = tracker.view();
    let replay = tracker.observe(profile);
    for (field, value) in replay {
        let result = write_power_limit(
            runtime.power_limits.as_ref(),
            field.clone(),
            value,
            None,
            emit,
        )
        .await;
        let failed = result.is_err();
        emit(WorkerEvent::PowerLimit { field, result });
        if failed {
            break;
        }
    }
    let after = tracker.view();
    if after != before {
        if let Some(view) = after {
            emit(WorkerEvent::ProfileLimits(view));
        }
    }
}

async fn run_worker_inner<G, B, R, F>(
    mut runtime: ApplicationRuntime<G, B, R>,
    mut receiver: UnboundedReceiver<WorkerCommand>,
    mut emit: F,
    poll_interval: Option<Duration>,
    product_gpu: Option<std::sync::Arc<dyn HardwareProductGpuSource>>,
    mut profile_limits: ProfileLimitsTracker,
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
    const PRODUCT_GPU_STATUS_REFRESH_INTERVAL: u32 = 5;
    // Profiles also change outside Orbis (power-profiles-daemon, Fn+F5,
    // desktop applets); re-read so the UI never keeps a stale selection.
    const PERFORMANCE_REFRESH_INTERVAL: u32 = 2;
    let mut gpu_refresh_counter = 0_u32;
    let mut performance_refresh_counter = 0_u32;
    let mut capability_refresh_counter = 0_u32;
    let mut product_gpu_status_refresh_counter = 0_u32;

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

                            performance_refresh_counter += 1;
                            if performance_refresh_counter >= PERFORMANCE_REFRESH_INTERVAL {
                                performance_refresh_counter = 0;
                                let performance =
                                    bounded_performance_state(&runtime.performance).await;
                                reconcile_automation_from_performance_result(
                                    &mut automation,
                                    &performance,
                                );
                                let observed = performance.as_ref().ok().map(|state| state.current);
                                emit(WorkerEvent::PerformanceRefresh(performance));
                                if let Some(profile) = observed {
                                    observe_profile_limits(&runtime, &mut profile_limits, profile, &mut emit).await;
                                }
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

                            product_gpu_status_refresh_counter += 1;
                            if product_gpu_status_refresh_counter
                                >= PRODUCT_GPU_STATUS_REFRESH_INTERVAL
                            {
                                product_gpu_status_refresh_counter = 0;
                                emit(WorkerEvent::ProductGpuStatusRefresh(
                                    product_gpu_status_read(product_gpu.as_deref()).await,
                                ));
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

        if matches!(command, WorkerCommand::RefreshProductGpuStatus) {
            emit(WorkerEvent::ProductGpuStatusRefresh(
                product_gpu_status_read(product_gpu.as_deref()).await,
            ));
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

        if matches!(command, WorkerCommand::RefreshPowerLimits) {
            let result = match runtime.power_limits.as_ref() {
                Some(provider) => {
                    bounded_provider_call(
                        provider.as_ref(),
                        "power_limits.snapshot",
                        provider.power_limit_snapshot(),
                    )
                    .await
                }
                None => Err(ProviderError::BackendUnavailable(
                    "power-limit Session1 provider unavailable".into(),
                )),
            };
            emit(WorkerEvent::PowerLimitsRefresh(result));
            continue;
        }

        let event = match command {
            WorkerCommand::SetPerformance(profile) => {
                let result = runtime.performance.set_performance(profile).await;
                let observed = result.as_ref().ok().map(|outcome| outcome.state.current);
                emit(WorkerEvent::Performance(result));
                if let Some(profile) = observed {
                    observe_profile_limits(&runtime, &mut profile_limits, profile, &mut emit).await;
                }
                continue;
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
                // Read-only authoritative status read, bounded by
                // `product_gpu_status_read`; the result is published unchanged so
                // the presentation boundary can distinguish a failed read from a
                // mutation outcome.
                WorkerEvent::ProductGpuStatusRefresh(
                    product_gpu_status_read(product_gpu.as_deref()).await,
                )
            }
            WorkerCommand::SetPowerLimit {
                field,
                value,
                snapshot_identity,
            } => {
                let result = write_power_limit(
                    runtime.power_limits.as_ref(),
                    field.clone(),
                    value,
                    Some(snapshot_identity),
                    &mut emit,
                )
                .await;
                if matches!(result, Ok(ApplyResult::Applied)) {
                    profile_limits.record_applied(&field, value);
                    if let Some(view) = profile_limits.view() {
                        emit(WorkerEvent::ProfileLimits(view));
                    }
                }
                WorkerEvent::PowerLimit { field, result }
            }
            WorkerCommand::SetProfileLimitsAutoApply { enabled } => {
                if let Err(message) = profile_limits.set_auto_apply(enabled) {
                    tracing::warn!("profile limits auto-apply not changed: {message}");
                }
                if let Some(view) = profile_limits.view() {
                    emit(WorkerEvent::ProfileLimits(view));
                }
                continue;
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
                let observed = result.as_ref().ok().map(|state| state.current);
                emit(WorkerEvent::PerformanceRefresh(result));
                if let Some(profile) = observed {
                    observe_profile_limits(&runtime, &mut profile_limits, profile, &mut emit).await;
                }
                continue;
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
            WorkerCommand::RefreshCapabilities
            | WorkerCommand::RefreshTelemetry
            | WorkerCommand::RefreshPowerLimits => {
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
    let power_limit_write_status = runtime.requery_power_limit_write_status().await;
    let next_generation = runtime.capabilities().generation() + 1;
    let snapshot = crate::composition::probe_capability_registry(
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
    .await?;
    match runtime.power_limits.as_ref() {
        Some(provider) => Ok(crate::composition::augment_power_limit_capabilities(
            provider.as_ref(),
            snapshot,
            power_limit_write_status,
        )
        .await),
        None => Ok(snapshot),
    }
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

    // --- Read-only product GPU status: real worker-loop coverage (D2) ---

    use crate::composition::GpuServices;
    use orbis_application::AppService;
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::identity::BackendIdentity;
    use orbis_core::limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
    use orbis_core::profile::PerformanceProfile;
    use orbis_hardwared::fans::FanCurveWire;
    use orbis_providers::error::ValidationResult;
    use orbis_providers::mock::MockProvider;
    use orbis_providers::traits::ProviderHealth;
    use orbis_providers::traits::{PowerLimitProvider, Provider};
    use orbis_session_client::HardwarePerformanceSource;
    use orbis_session_client::HardwarePowerLimitSource;
    use orbis_session_client::SessionHardwarePowerLimitProvider;
    use orbis_session_client::ZbusHardwareFanCurveSource;
    use orbis_session_client::{HardwareFanCurveSource, HardwareProductGpuSource};
    use orbis_test_support::devices::build_state_arc;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Clone)]
    struct PrivatePerformanceHardware {
        state: Arc<StdMutex<u8>>,
        calls: Arc<AtomicU32>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl PrivatePerformanceHardware {
        async fn set_performance_profile(&self, profile: u8) -> zbus::fdo::Result<u8> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.state.lock().unwrap() = profile;
            Ok(profile)
        }
    }

    async fn private_performance_peer(
        hardware: PrivatePerformanceHardware,
    ) -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/io/github/orbiscontrol/Hardware", hardware)
            .unwrap();
        let client = zbus::connection::Builder::unix_stream(client_stream).p2p();
        tokio::try_join!(server.build(), client.build()).unwrap()
    }

    #[tokio::test]
    async fn worker_loop_private_performance_write_owner_loss_and_recovery() {
        let first_state = Arc::new(StdMutex::new(0u8));
        let first_calls = Arc::new(AtomicU32::new(0));
        let (server, connection) = private_performance_peer(PrivatePerformanceHardware {
            state: first_state.clone(),
            calls: first_calls.clone(),
        })
        .await;
        let source = orbis_session_client::ZbusHardwarePerformanceSource::new(connection);
        let performance = orbis_session_client::SessionHardwarePerformanceProvider::new(
            PrivatePerformanceSession {
                state: first_state.clone(),
            },
            source,
        );
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime_with_performance(Arc::new(performance)),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            None,
        ));
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Turbo))
            .unwrap();
        let result = loop {
            if let WorkerEvent::Performance(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            result.is_ok(),
            "profile write must be acknowledged: {result:?}"
        );
        assert_eq!(*first_state.lock().unwrap(), 2);
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        let removed = server
            .object_server()
            .remove::<PrivatePerformanceHardware, _>("/io/github/orbiscontrol/Hardware")
            .await
            .unwrap();
        assert!(removed, "the Hardware1 object must really be removed");
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .unwrap();
        let lost = loop {
            if let WorkerEvent::Performance(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert_eq!(
            first_calls.load(Ordering::SeqCst),
            1,
            "a missing Hardware1 object must not receive the write"
        );
        assert!(lost.is_err(), "owner loss must report unavailable/error");
        drop(tx);
        worker.abort();
        let fresh_state = Arc::new(StdMutex::new(0u8));
        let fresh_calls = Arc::new(AtomicU32::new(0));
        let (_fresh_server, connection) = private_performance_peer(PrivatePerformanceHardware {
            state: fresh_state.clone(),
            calls: fresh_calls.clone(),
        })
        .await;
        let fresh = orbis_session_client::ZbusHardwarePerformanceSource::new(connection);
        assert!(fresh.set_performance(1).await.is_ok());
        assert_eq!(*fresh_state.lock().unwrap(), 1);
        assert_eq!(fresh_calls.load(Ordering::SeqCst), 1);
    }

    #[derive(Clone)]
    struct PrivateProductGpuHardware {
        state: Arc<StdMutex<ProductGpuMutationResult>>,
        calls: Arc<StdMutex<Vec<u32>>>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl PrivateProductGpuHardware {
        async fn set_product_gpu_mode(
            &self,
            requested_mode: u32,
        ) -> zbus::fdo::Result<ProductGpuMutationResult> {
            self.calls.lock().unwrap().push(requested_mode);
            let mut state = self.state.lock().unwrap();
            *state = ProductGpuMutationResult {
                requested_mode,
                current_mode: state.current_mode,
                queued_mode: requested_mode,
                outcome: 1,
                reboot_required: true,
            };
            Ok(*state)
        }

        async fn product_gpu_status(&self) -> zbus::fdo::Result<ProductGpuMutationResult> {
            Ok(*self.state.lock().unwrap())
        }
    }

    async fn private_product_gpu_peer(
        hardware: PrivateProductGpuHardware,
    ) -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/io/github/orbiscontrol/Hardware", hardware)
            .unwrap();
        let client = zbus::connection::Builder::unix_stream(client_stream).p2p();
        tokio::try_join!(server.build(), client.build()).unwrap()
    }

    /// Scripted Hardware1 peer: counts status reads, returns a fixed reply.
    struct ScriptedProductGpu {
        result: ProductGpuMutationResult,
        status_calls: AtomicU32,
    }

    #[async_trait::async_trait]
    impl HardwareProductGpuSource for ScriptedProductGpu {
        async fn set_product_gpu_mode(
            &self,
            _requested_mode: u32,
        ) -> Result<ProductGpuMutationResult, ProviderError> {
            Err(ProviderError::Unsupported("not under test".into()))
        }

        async fn product_gpu_status(&self) -> Result<ProductGpuMutationResult, ProviderError> {
            self.status_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.result)
        }
    }

    fn mock_runtime() -> ApplicationRuntime<
        GpuServices<MockProvider, MockProvider, MockProvider, MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
    > {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
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
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
        )
    }

    struct PrivatePerformanceSession {
        state: Arc<StdMutex<u8>>,
    }

    #[async_trait::async_trait]
    impl orbis_session_client::SessionPerformanceSource for PrivatePerformanceSession {
        async fn read_performance(
            &self,
        ) -> Result<orbis_session_protocol::PerformanceInfo, ProviderError> {
            Ok(orbis_session_protocol::PerformanceInfo {
                current: *self.state.lock().unwrap(),
                available_mask: 0b111,
            })
        }
    }

    fn mock_runtime_with_performance<P>(
        performance: Arc<P>,
    ) -> ApplicationRuntime<
        GpuServices<MockProvider, MockProvider, MockProvider, MockProvider>,
        AppService<MockProvider>,
        AppService<P>,
    >
    where
        P: orbis_providers::traits::PerformanceProvider + Send + Sync + 'static,
    {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        ApplicationRuntime::empty_for_testing(
            GpuServices::new(
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
                AppService::new(provider.clone()),
            ),
            AppService::new(provider.clone()),
            AppService::new(performance),
            AppService::new(provider.clone()),
            AppService::new(provider),
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
        )
    }

    #[tokio::test]
    async fn worker_loop_publishes_authoritative_product_gpu_status_event() {
        let source = Arc::new(ScriptedProductGpu {
            result: ProductGpuMutationResult {
                requested_mode: 1,
                current_mode: 1,
                queued_mode: 2,
                outcome: 1, // RebootRequired
                reboot_required: true,
            },
            status_calls: AtomicU32::new(0),
        });
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            Some(source.clone()),
        ));

        tx.send(WorkerCommand::RefreshProductGpuStatus)
            .expect("worker channel open");

        let mut observed = None;
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_secs(5), event_rx.recv()).await
        {
            if let WorkerEvent::ProductGpuStatusRefresh(result) = event {
                observed = Some(result.expect("scripted status read must succeed"));
                break;
            }
        }
        drop(tx);
        worker.abort();

        let reply = observed.expect("worker must publish ProductGpuStatusRefresh");
        // The exact wire triple reaches the presentation boundary unchanged:
        // current=1 (Standard observed), queued=2 (Ultimate deferred),
        // reboot_required — the AC-050 evidence set.
        assert_eq!(reply.current_mode, 1);
        assert_eq!(reply.queued_mode, 2);
        assert!(reply.reboot_required);
        assert_eq!(
            source.status_calls.load(Ordering::SeqCst),
            1,
            "one command must produce exactly one read-only status query"
        );
    }

    #[tokio::test]
    async fn worker_loop_reports_unsupported_status_fail_closed() {
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            None,
        ));

        tx.send(WorkerCommand::RefreshProductGpuStatus)
            .expect("worker channel open");

        let mut observed = None;
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_secs(5), event_rx.recv()).await
        {
            if let WorkerEvent::ProductGpuStatusRefresh(result) = event {
                observed = Some(result);
                break;
            }
        }
        drop(tx);
        worker.abort();

        let result = observed.expect("worker must publish ProductGpuStatusRefresh");
        assert!(
            matches!(result, Err(ProviderError::Unsupported(_))),
            "absent promotion must be an honest Unsupported, got {result:?}"
        );
    }

    #[tokio::test]
    async fn worker_loop_private_peer_mutation_readback_owner_loss_and_recovery() {
        let initial = ProductGpuMutationResult {
            requested_mode: 0,
            current_mode: 0,
            queued_mode: u32::MAX,
            outcome: 0,
            reboot_required: false,
        };
        let hardware = PrivateProductGpuHardware {
            state: Arc::new(StdMutex::new(initial)),
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let peer_state = hardware.state.clone();
        let peer_calls = hardware.calls.clone();
        let (server, connection) = private_product_gpu_peer(hardware).await;
        let source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(connection),
        );
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            Some(source.clone()),
        ));

        tx.send(WorkerCommand::SetProductGpuMode { raw: 2 })
            .unwrap();
        let mutation = loop {
            if let WorkerEvent::ProductGpu(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result.unwrap();
            }
        };
        assert_eq!(mutation.queued_mode, 2);
        assert!(mutation.reboot_required);
        assert_eq!(*peer_calls.lock().unwrap(), vec![2]);

        tx.send(WorkerCommand::RefreshProductGpuStatus).unwrap();
        let read_back = loop {
            if let WorkerEvent::ProductGpuStatusRefresh(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result.unwrap();
            }
        };
        assert_eq!(read_back.current_mode, 0);
        assert_eq!(read_back.queued_mode, 2);
        assert!(read_back.reboot_required);
        assert_eq!(*peer_state.lock().unwrap(), read_back);

        drop(server);
        tx.send(WorkerCommand::RefreshProductGpuStatus).unwrap();
        let lost = loop {
            if let WorkerEvent::ProductGpuStatusRefresh(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            lost.is_err(),
            "owner loss must not report success: {lost:?}"
        );

        drop(tx);
        worker.abort();
        let replacement = PrivateProductGpuHardware {
            state: Arc::new(StdMutex::new(initial)),
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let replacement_state = replacement.state.clone();
        let (_replacement_server, connection) = private_product_gpu_peer(replacement).await;
        let replacement = orbis_session_client::ZbusHardwareProductGpuSource::new(connection);
        let fresh = replacement.product_gpu_status().await.unwrap();
        assert_eq!(fresh, initial);
        assert_eq!(*replacement_state.lock().unwrap(), initial);
    }

    #[derive(Clone)]
    struct PrivateFanHardware {
        state: Arc<StdMutex<std::collections::BTreeMap<(u32, u8), FanCurveWire>>>,
        calls: Arc<StdMutex<Vec<(u32, u8, FanCurveWire)>>>,
        defaults: Arc<StdMutex<Vec<u32>>>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl PrivateFanHardware {
        async fn set_fan_curve(
            &self,
            profile: u32,
            fan: u8,
            curve: FanCurveWire,
        ) -> zbus::fdo::Result<u32> {
            self.calls
                .lock()
                .unwrap()
                .push((profile, fan, curve.clone()));
            self.state.lock().unwrap().insert((profile, fan), curve);
            Ok(profile)
        }

        async fn reset_fan_curves_to_defaults(&self, profile: u32) -> zbus::fdo::Result<u32> {
            self.defaults.lock().unwrap().push(profile);
            self.state
                .lock()
                .unwrap()
                .retain(|(stored_profile, _), _| *stored_profile != profile);
            Ok(profile)
        }
    }

    async fn private_fan_peer(
        hardware: PrivateFanHardware,
    ) -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/io/github/orbiscontrol/Hardware", hardware)
            .unwrap();
        let client = zbus::connection::Builder::unix_stream(client_stream).p2p();
        tokio::try_join!(server.build(), client.build()).unwrap()
    }

    fn fan_runtime(
        source: ZbusHardwareFanCurveSource,
    ) -> ApplicationRuntime<
        GpuServices<MockProvider, MockProvider, MockProvider, MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
    > {
        let provider = MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists"));
        let fan = Arc::new(orbis_session_client::SessionHardwareFanCurveProvider::new(
            MockProvider::new(provider.state()),
            source,
        ));
        ApplicationRuntime::empty_for_testing(
            GpuServices::new(
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
            ),
            AppService::new(Arc::new(MockProvider::new(provider.state()))),
            AppService::new(Arc::new(MockProvider::new(provider.state()))),
            AppService::new(fan),
            AppService::new(Arc::new(provider)),
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
        )
    }

    fn test_fan_curve() -> FanCurvePoints {
        FanCurvePoints {
            temps: [45, 50, 55, 60, 65, 70, 75, 80]
                .map(|v| orbis_core::newtypes::TemperatureC::new(v).unwrap()),
            pwms: [10, 20, 30, 40, 50, 60, 70, 80]
                .map(|v| orbis_core::newtypes::FanPwm::new(v).unwrap()),
        }
    }

    #[tokio::test]
    async fn worker_loop_private_peer_fan_cpu_gpu_reset_owner_loss_and_recovery() {
        use orbis_core::profile::AsusdFanProfile;
        let hardware = PrivateFanHardware {
            state: Arc::new(StdMutex::new(Default::default())),
            calls: Arc::new(StdMutex::new(Vec::new())),
            defaults: Arc::new(StdMutex::new(Vec::new())),
        };
        let owner_state = hardware.state.clone();
        let calls = hardware.calls.clone();
        let defaults = hardware.defaults.clone();
        let (server, connection) = private_fan_peer(hardware).await;
        let source = orbis_session_client::ZbusHardwareFanCurveSource::new(connection);
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker(fan_runtime(source), rx, move |event| {
            let _ = event_tx.send(event);
        }));

        for (fan, wire_id) in [(FanId::Cpu, 0), (FanId::Gpu, 1)] {
            tx.send(WorkerCommand::SetFanCurve {
                profile: AsusdFanProfile::Balanced,
                fan: fan.clone(),
                curve: test_fan_curve(),
            })
            .unwrap();
            let result = loop {
                if let WorkerEvent::FanCurve(result) =
                    tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                        .await
                        .unwrap()
                        .unwrap()
                {
                    break result;
                }
            };
            assert!(result.is_ok(), "fan worker outcome: {result:?}");
            let independently_read = owner_state
                .lock()
                .unwrap()
                .get(&(0, wire_id))
                .cloned()
                .expect("owner state written");
            assert_eq!(
                independently_read.temps,
                vec![45, 50, 55, 60, 65, 70, 75, 80]
            );
            assert_eq!(
                independently_read.pwms,
                vec![10, 20, 30, 40, 50, 60, 70, 80]
            );
        }
        assert_eq!(calls.lock().unwrap().len(), 2);

        tx.send(WorkerCommand::ResetFanCurvesToDefaults {
            profile: AsusdFanProfile::Balanced,
        })
        .unwrap();
        let reset = loop {
            if let WorkerEvent::FanCurveDefaults { result, .. } =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(reset.is_ok(), "reset worker outcome: {reset:?}");
        assert_eq!(*defaults.lock().unwrap(), vec![0]);
        assert!(
            owner_state.lock().unwrap().is_empty(),
            "reset state must be independently visible at owner"
        );

        let removed = server
            .object_server()
            .remove::<PrivateFanHardware, _>("/io/github/orbiscontrol/Hardware")
            .await
            .unwrap();
        assert!(removed);
        tx.send(WorkerCommand::SetFanCurve {
            profile: AsusdFanProfile::Balanced,
            fan: FanId::Cpu,
            curve: test_fan_curve(),
        })
        .unwrap();
        let lost = loop {
            if let WorkerEvent::FanCurve(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(lost.is_err(), "owner loss must be honest: {lost:?}");
        drop(tx);
        worker.abort();

        let fresh_hardware = PrivateFanHardware {
            state: Arc::new(StdMutex::new(Default::default())),
            calls: Arc::new(StdMutex::new(Vec::new())),
            defaults: Arc::new(StdMutex::new(Vec::new())),
        };
        let fresh_state = fresh_hardware.state.clone();
        let (_fresh_server, connection) = private_fan_peer(fresh_hardware).await;
        let fresh = orbis_session_client::ZbusHardwareFanCurveSource::new(connection);
        assert!(
            fresh
                .set_fan_curve(
                    0,
                    0,
                    FanCurveWire {
                        temps: vec![45, 50, 55, 60, 65, 70, 75, 80],
                        pwms: vec![10, 20, 30, 40, 50, 60, 70, 80]
                    }
                )
                .await
                .is_ok()
        );
        assert!(
            fresh_state.lock().unwrap().contains_key(&(0, 0)),
            "fresh owner works without implicit restore"
        );
    }

    #[derive(Clone)]
    struct PrivatePowerLimitHardware {
        values: Arc<StdMutex<std::collections::BTreeMap<u8, i32>>>,
        calls: Arc<StdMutex<Vec<(u8, i32)>>>,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl PrivatePowerLimitHardware {
        async fn set_power_limit(&self, field: u8, value: i32) -> zbus::fdo::Result<i32> {
            self.calls.lock().unwrap().push((field, value));
            self.values.lock().unwrap().insert(field, value);
            Ok(value)
        }
    }

    async fn private_power_limit_peer(
        hardware: PrivatePowerLimitHardware,
    ) -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/io/github/orbiscontrol/Hardware", hardware)
            .unwrap();
        let client = zbus::connection::Builder::unix_stream(client_stream).p2p();
        tokio::try_join!(server.build(), client.build()).unwrap()
    }

    struct DeterministicPowerLimitReads {
        values: Arc<StdMutex<std::collections::BTreeMap<PowerLimitField, i32>>>,
    }

    #[async_trait::async_trait]
    impl Provider for DeterministicPowerLimitReads {
        fn id(&self) -> &'static str {
            "test-power-limit-reads"
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("deterministic test reads")
        }
        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("missing {feature}")
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait::async_trait]
    impl PowerLimitProvider for DeterministicPowerLimitReads {
        async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
            let mut values = self.values.lock().unwrap();
            let fields = [
                (PowerLimitField::Spl, Unit::Watts),
                (PowerLimitField::GpuDynamicBoost, Unit::Watts),
            ]
            .into_iter()
            .map(|(field, unit)| {
                let value = *values.entry(field.clone()).or_insert(0);
                (
                    field,
                    PowerLimitValue::new(value, 0, 200, 1, Some(value), unit).unwrap(),
                )
            })
            .collect();
            Ok(PowerLimits { fields })
        }
        async fn set_power_limit(
            &self,
            _field: PowerLimitField,
            _value: i32,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported(
                "test read provider is read-only".into(),
            ))
        }
        async fn restore_defaults(&self) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported(
                "test read provider is read-only".into(),
            ))
        }
        fn validate_power_limit(&self, _field: &PowerLimitField, _value: i32) -> ValidationResult {
            ValidationResult::invalid("test read provider is read-only")
        }
    }

    fn power_limit_runtime(
        source: orbis_session_client::ZbusHardwarePowerLimitSource,
        read_values: Arc<StdMutex<std::collections::BTreeMap<PowerLimitField, i32>>>,
    ) -> ApplicationRuntime<
        GpuServices<MockProvider, MockProvider, MockProvider, MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
    > {
        let provider = MockProvider::new(build_state_arc("zephyrus-full").expect("profile exists"));
        let values = read_values;
        let limits = Arc::new(SessionHardwarePowerLimitProvider::new(
            DeterministicPowerLimitReads {
                values: values.clone(),
            },
            TestHardwarePowerLimitSource { source, values },
        ));
        ApplicationRuntime::empty_for_testing(
            GpuServices::new(
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
                AppService::new(Arc::new(MockProvider::new(provider.state()))),
            ),
            AppService::new(Arc::new(MockProvider::new(provider.state()))),
            AppService::new(Arc::new(MockProvider::new(provider.state()))),
            AppService::new(Arc::new(MockProvider::new(provider.state()))),
            AppService::new(Arc::new(provider)),
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
            orbis_core::capability::CapabilityStatus::Unsupported,
        )
        .with_power_limits(limits)
    }

    struct TestHardwarePowerLimitSource {
        source: orbis_session_client::ZbusHardwarePowerLimitSource,
        values: Arc<StdMutex<std::collections::BTreeMap<PowerLimitField, i32>>>,
    }

    #[async_trait::async_trait]
    impl HardwarePowerLimitSource for TestHardwarePowerLimitSource {
        async fn set_power_limit(&self, field: u8, value: i32) -> Result<i32, ProviderError> {
            let observed = self.source.set_power_limit(field, value).await?;
            let domain_field = match field {
                0 => PowerLimitField::Spl,
                4 => PowerLimitField::GpuDynamicBoost,
                _ => {
                    return Err(ProviderError::Unsupported(
                        "unexpected test wire field".into(),
                    ));
                }
            };
            self.values.lock().unwrap().insert(domain_field, observed);
            Ok(observed)
        }
    }

    #[tokio::test]
    async fn profile_change_replays_stored_limits_only_when_auto_apply_is_on() {
        use orbis_core::limits::PowerLimitField::Spl;
        let owner = PrivatePowerLimitHardware {
            values: Arc::new(StdMutex::new(Default::default())),
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let calls = owner.calls.clone();
        let (_server, connection) = private_power_limit_peer(owner).await;
        let reads = Arc::new(StdMutex::new([(Spl, 30)].into()));
        let runtime = power_limit_runtime(
            orbis_session_client::ZbusHardwarePowerLimitSource::new(connection),
            reads,
        );
        let initial = runtime
            .power_limits
            .as_ref()
            .unwrap()
            .power_limit_snapshot()
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let tracker = ProfileLimitsTracker::load_from_dir(dir.path().to_path_buf());
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(run_worker_inner(
            runtime,
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            None,
            tracker,
        ));

        async fn next_matching(
            rx: &mut tokio::sync::mpsc::UnboundedReceiver<WorkerEvent>,
            pick: impl Fn(&WorkerEvent) -> bool,
        ) -> WorkerEvent {
            loop {
                let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .expect("worker event")
                    .expect("worker alive");
                if pick(&event) {
                    return event;
                }
            }
        }

        tx.send(WorkerCommand::RefreshPerformance).unwrap();
        let initial_profile = match next_matching(&mut event_rx, |e| {
            matches!(e, WorkerEvent::ProfileLimits(_))
        })
        .await
        {
            WorkerEvent::ProfileLimits(view) => {
                assert!(!view.auto_apply && view.saved.is_empty());
                view.profile
            }
            _ => unreachable!(),
        };
        let other = [PerformanceProfile::Silent, PerformanceProfile::Turbo]
            .into_iter()
            .find(|p| *p != initial_profile)
            .unwrap();

        // Off: applied value is not remembered and a switch replays nothing.
        tx.send(WorkerCommand::SetPowerLimit {
            field: Spl,
            value: 45,
            snapshot_identity: initial.identity,
        })
        .unwrap();
        next_matching(&mut event_rx, |e| {
            matches!(e, WorkerEvent::PowerLimit { .. })
        })
        .await;
        tx.send(WorkerCommand::SetPerformance(other)).unwrap();
        next_matching(&mut event_rx, |e| {
            matches!(e, WorkerEvent::ProfileLimits(_))
        })
        .await;
        tx.send(WorkerCommand::SetPerformance(initial_profile))
            .unwrap();
        next_matching(&mut event_rx, |e| {
            matches!(e, WorkerEvent::ProfileLimits(_))
        })
        .await;
        assert_eq!(*calls.lock().unwrap(), vec![(0, 45)]);

        // On: the applied value is stored and replayed on returning to the profile.
        tx.send(WorkerCommand::SetProfileLimitsAutoApply { enabled: true })
            .unwrap();
        next_matching(
            &mut event_rx,
            |e| matches!(e, WorkerEvent::ProfileLimits(v) if v.auto_apply),
        )
        .await;
        tx.send(WorkerCommand::RefreshPowerLimits).unwrap();
        let snapshot = match next_matching(&mut event_rx, |e| {
            matches!(e, WorkerEvent::PowerLimitsRefresh(Ok(_)))
        })
        .await
        {
            WorkerEvent::PowerLimitsRefresh(Ok(snapshot)) => snapshot,
            _ => unreachable!(),
        };
        tx.send(WorkerCommand::SetPowerLimit {
            field: Spl,
            value: 50,
            snapshot_identity: snapshot.identity,
        })
        .unwrap();
        let stored = next_matching(
            &mut event_rx,
            |e| matches!(e, WorkerEvent::ProfileLimits(v) if !v.saved.is_empty()),
        )
        .await;
        assert!(matches!(stored, WorkerEvent::ProfileLimits(v) if v.saved == vec![(Spl, 50)]));
        tx.send(WorkerCommand::SetPerformance(other)).unwrap();
        next_matching(&mut event_rx, |e| {
            matches!(e, WorkerEvent::ProfileLimits(_))
        })
        .await;
        let before = calls.lock().unwrap().len();
        tx.send(WorkerCommand::SetPerformance(initial_profile))
            .unwrap();
        next_matching(&mut event_rx, |e| {
            matches!(
                e,
                WorkerEvent::PowerLimit {
                    result: Ok(ApplyResult::Applied),
                    ..
                }
            )
        })
        .await;
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), before + 1);
        assert_eq!(*calls.last().unwrap(), (0, 50));
    }

    #[tokio::test]
    async fn worker_loop_private_peer_power_spl_and_boost_owner_loss_and_recovery() {
        use orbis_core::limits::PowerLimitField::{GpuDynamicBoost, Spl};
        let owner = PrivatePowerLimitHardware {
            values: Arc::new(StdMutex::new(Default::default())),
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let owner_values = owner.values.clone();
        let calls = owner.calls.clone();
        let (server, connection) = private_power_limit_peer(owner).await;
        let reads = Arc::new(StdMutex::new([(Spl, 30), (GpuDynamicBoost, 0)].into()));
        let runtime = power_limit_runtime(
            orbis_session_client::ZbusHardwarePowerLimitSource::new(connection),
            reads.clone(),
        );
        let initial = runtime
            .power_limits
            .as_ref()
            .unwrap()
            .power_limit_snapshot()
            .await
            .unwrap();
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker(runtime, rx, move |event| {
            let _ = event_tx.send(event);
        }));

        let mut snapshot = initial.clone();
        for (field, value, wire) in [(Spl, 45, 0), (GpuDynamicBoost, 25, 4)] {
            tx.send(WorkerCommand::SetPowerLimit {
                field: field.clone(),
                value,
                snapshot_identity: snapshot.identity,
            })
            .unwrap();
            let refresh = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                .await
                .unwrap()
                .unwrap();
            let refreshed = match refresh {
                WorkerEvent::PowerLimitsRefresh(Ok(snapshot)) => snapshot,
                other => panic!("expected successful authoritative read-back first, got {other:?}"),
            };
            assert_eq!(refreshed.limits.get(&field).unwrap().value, value);
            assert_ne!(refreshed.identity, snapshot.identity);
            snapshot = refreshed;
            let applied = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                matches!(applied, WorkerEvent::PowerLimit { field: ref f, result: Ok(ApplyResult::Applied) } if *f == field),
                "apply event: {applied:?}"
            );
            assert_eq!(owner_values.lock().unwrap().get(&wire), Some(&value));
        }
        assert_eq!(*calls.lock().unwrap(), vec![(0, 45), (4, 25)]);

        let before = calls.lock().unwrap().len();
        tx.send(WorkerCommand::SetPowerLimit {
            field: Spl,
            value: 50,
            snapshot_identity: initial.identity,
        })
        .unwrap();
        let stale = loop {
            if let WorkerEvent::PowerLimit { result, .. } =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            matches!(stale, Err(ProviderError::Conflict(_))),
            "stale identity: {stale:?}"
        );
        assert_eq!(
            calls.lock().unwrap().len(),
            before,
            "stale snapshot must not mutate Hardware1"
        );
        assert!(
            event_rx.try_recv().is_err(),
            "stale identity must not publish a refresh"
        );

        let removed = server
            .object_server()
            .remove::<PrivatePowerLimitHardware, _>("/io/github/orbiscontrol/Hardware")
            .await
            .unwrap();
        assert!(removed);
        tx.send(WorkerCommand::SetPowerLimit {
            field: Spl,
            value: 55,
            snapshot_identity: snapshot.identity,
        })
        .unwrap();
        let lost = loop {
            if let WorkerEvent::PowerLimit { result, .. } =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(lost.is_err(), "removed owner must fail honestly: {lost:?}");

        drop(tx);
        worker.abort();
        let fresh = PrivatePowerLimitHardware {
            values: Arc::new(StdMutex::new(Default::default())),
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let fresh_values = fresh.values.clone();
        let fresh_calls = fresh.calls.clone();
        let (_fresh_server, connection) = private_power_limit_peer(fresh).await;
        let fresh_source = orbis_session_client::ZbusHardwarePowerLimitSource::new(connection);
        let fresh_runtime = power_limit_runtime(fresh_source, reads);
        let fresh_snapshot = fresh_runtime
            .power_limits
            .as_ref()
            .unwrap()
            .power_limit_snapshot()
            .await
            .unwrap();
        let (fresh_tx, fresh_rx) = command_channel();
        let (fresh_event_tx, mut fresh_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let fresh_worker = tokio::spawn(run_worker(fresh_runtime, fresh_rx, move |event| {
            let _ = fresh_event_tx.send(event);
        }));
        fresh_tx
            .send(WorkerCommand::SetPowerLimit {
                field: GpuDynamicBoost,
                value: 35,
                snapshot_identity: fresh_snapshot.identity,
            })
            .unwrap();
        loop {
            if let WorkerEvent::PowerLimit { result, .. } =
                tokio::time::timeout(Duration::from_secs(5), fresh_event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                assert!(
                    result.is_ok(),
                    "fresh owner should apply only requested field: {result:?}"
                );
                break;
            }
        }
        assert_eq!(*fresh_calls.lock().unwrap(), vec![(4, 35)]);
        assert_eq!(*fresh_values.lock().unwrap(), [(4, 35)].into());
        drop(fresh_tx);
        fresh_worker.abort();
    }

    // --- M2D: remaining ordinary-worker GPU-mode outcomes + restart/no-restore ---

    fn initial_product_gpu_state() -> ProductGpuMutationResult {
        ProductGpuMutationResult {
            requested_mode: 0,
            current_mode: 0,
            queued_mode: u32::MAX,
            outcome: 0,
            reboot_required: false,
        }
    }

    /// Scripted failure a private Hardware1 owner can return from one call.
    #[derive(Clone, Copy)]
    enum ScriptedGpuFailure {
        /// Authorization refusal (`org.freedesktop.DBus.Error.AccessDenied`).
        Denied,
        /// Half-applied write: the daemon reports a failure after a partial
        /// queue, leaving the authoritative state untouched.
        PartialWrite,
    }

    impl ScriptedGpuFailure {
        fn into_fdo(self, operation: &str) -> zbus::fdo::Error {
            match self {
                ScriptedGpuFailure::Denied => {
                    zbus::fdo::Error::AccessDenied(format!("scripted denial for {operation}"))
                }
                ScriptedGpuFailure::PartialWrite => zbus::fdo::Error::Failed(format!(
                    "ASUS GPU mode queue is partial after dgpu_disable was accepted: scripted for {operation}"
                )),
            }
        }
    }

    /// Private Hardware1 owner with scriptable write/read behaviour.
    #[derive(Clone)]
    struct ScriptedGpuHardware {
        state: Arc<StdMutex<ProductGpuMutationResult>>,
        set_calls: Arc<StdMutex<Vec<u32>>>,
        status_calls: Arc<AtomicU32>,
        set_failure: Option<ScriptedGpuFailure>,
        status_failure: Option<ScriptedGpuFailure>,
        /// Whether an accepted write queues the requested mode (reboot-required
        /// semantics) or leaves the authoritative state untouched (partial).
        queue_on_write: bool,
        /// Whether the read-only status handler never replies (a hung peer).
        ///
        /// Models a stuck asusd without needing a Tokio reactor on the zbus
        /// executor thread: the client-side read deadline must fail closed.
        hang_status: bool,
    }

    #[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
    impl ScriptedGpuHardware {
        async fn set_product_gpu_mode(
            &self,
            requested_mode: u32,
        ) -> zbus::fdo::Result<ProductGpuMutationResult> {
            self.set_calls.lock().unwrap().push(requested_mode);
            if let Some(failure) = self.set_failure {
                return Err(failure.into_fdo("SetProductGpuMode"));
            }
            let mut state = self.state.lock().unwrap();
            if self.queue_on_write {
                *state = ProductGpuMutationResult {
                    requested_mode,
                    current_mode: state.current_mode,
                    queued_mode: requested_mode,
                    outcome: 1,
                    reboot_required: true,
                };
            }
            Ok(*state)
        }

        async fn product_gpu_status(&self) -> zbus::fdo::Result<ProductGpuMutationResult> {
            self.status_calls.fetch_add(1, Ordering::SeqCst);
            if self.hang_status {
                std::future::pending::<()>().await;
            }
            if let Some(failure) = self.status_failure {
                return Err(failure.into_fdo("ProductGpuStatus"));
            }
            Ok(*self.state.lock().unwrap())
        }
    }

    async fn scripted_gpu_peer(
        hardware: ScriptedGpuHardware,
    ) -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/io/github/orbiscontrol/Hardware", hardware)
            .unwrap();
        let client = zbus::connection::Builder::unix_stream(client_stream).p2p();
        tokio::try_join!(server.build(), client.build()).unwrap()
    }

    #[tokio::test]
    async fn worker_loop_private_peer_gpu_denial_and_partial_write_are_honest_without_mutation() {
        // 1) The Hardware1 authorization evidence denies the queue request.
        let owner = ScriptedGpuHardware {
            state: Arc::new(StdMutex::new(initial_product_gpu_state())),
            set_calls: Arc::new(StdMutex::new(Vec::new())),
            status_calls: Arc::new(AtomicU32::new(0)),
            set_failure: Some(ScriptedGpuFailure::Denied),
            status_failure: None,
            queue_on_write: false,
            hang_status: false,
        };
        let owner_state = owner.state.clone();
        let set_calls = owner.set_calls.clone();
        let (_server, connection) = scripted_gpu_peer(owner).await;
        let client = connection.clone();
        let source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(connection),
        );
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            Some(source),
        ));

        tx.send(WorkerCommand::SetProductGpuMode { raw: 2 })
            .unwrap();
        let denied = loop {
            if let WorkerEvent::ProductGpu(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            matches!(denied, Err(ProviderError::PermissionDenied(_))),
            "denied write must not be presented as success: {denied:?}"
        );
        assert_eq!(*set_calls.lock().unwrap(), vec![2]);

        // Independent owner read-back: the denied write queued nothing.
        let read_back = orbis_session_client::ZbusHardwareProductGpuSource::new(client)
            .product_gpu_status()
            .await
            .unwrap();
        assert_eq!(read_back, initial_product_gpu_state());
        assert_eq!(*owner_state.lock().unwrap(), initial_product_gpu_state());
        drop(tx);
        worker.abort();

        // 2) A half-applied write: the daemon fails after recording the request
        //    but leaves the authoritative queue untouched, so the ordinary
        //    worker must publish an honest error and invent no queued mode.
        let partial = ScriptedGpuHardware {
            state: Arc::new(StdMutex::new(initial_product_gpu_state())),
            set_calls: Arc::new(StdMutex::new(Vec::new())),
            status_calls: Arc::new(AtomicU32::new(0)),
            set_failure: Some(ScriptedGpuFailure::PartialWrite),
            status_failure: None,
            queue_on_write: false,
            hang_status: false,
        };
        let partial_state = partial.state.clone();
        let partial_calls = partial.set_calls.clone();
        let (_partial_server, partial_connection) = scripted_gpu_peer(partial).await;
        let partial_client = partial_connection.clone();
        let partial_source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(partial_connection),
        );
        let (partial_tx, partial_rx) = command_channel();
        let (partial_event_tx, mut partial_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let partial_worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            partial_rx,
            move |event| {
                let _ = partial_event_tx.send(event);
            },
            None,
            Some(partial_source),
        ));
        partial_tx
            .send(WorkerCommand::SetProductGpuMode { raw: 2 })
            .unwrap();
        let partial_result = loop {
            if let WorkerEvent::ProductGpu(result) =
                tokio::time::timeout(Duration::from_secs(5), partial_event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            matches!(partial_result, Err(ProviderError::Dbus(_))),
            "partial write must surface as an honest error: {partial_result:?}"
        );
        assert_eq!(*partial_calls.lock().unwrap(), vec![2]);
        let partial_read_back =
            orbis_session_client::ZbusHardwareProductGpuSource::new(partial_client)
                .product_gpu_status()
                .await
                .unwrap();
        assert_eq!(partial_read_back, initial_product_gpu_state());
        assert_eq!(*partial_state.lock().unwrap(), initial_product_gpu_state());
        drop(partial_tx);
        partial_worker.abort();
    }

    #[tokio::test]
    async fn worker_loop_private_peer_gpu_status_denial_and_timeout_are_honest() {
        // Read-only status denied by the backend evidence.
        let denied = ScriptedGpuHardware {
            state: Arc::new(StdMutex::new(initial_product_gpu_state())),
            set_calls: Arc::new(StdMutex::new(Vec::new())),
            status_calls: Arc::new(AtomicU32::new(0)),
            set_failure: None,
            status_failure: Some(ScriptedGpuFailure::Denied),
            queue_on_write: true,
            hang_status: false,
        };
        let denied_calls = denied.status_calls.clone();
        let (_denied_server, denied_connection) = scripted_gpu_peer(denied).await;
        let denied_source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(denied_connection),
        );
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            Some(denied_source),
        ));
        tx.send(WorkerCommand::RefreshProductGpuStatus).unwrap();
        let denied_result = loop {
            if let WorkerEvent::ProductGpuStatusRefresh(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            matches!(denied_result, Err(ProviderError::PermissionDenied(_))),
            "denied status read must be honest: {denied_result:?}"
        );
        assert_eq!(denied_calls.load(Ordering::SeqCst), 1);
        drop(tx);
        worker.abort();

        // A hung owner: the production read deadline must fail closed instead of
        // stalling the sequential worker or fabricating a mode.
        let hung = ScriptedGpuHardware {
            state: Arc::new(StdMutex::new(initial_product_gpu_state())),
            set_calls: Arc::new(StdMutex::new(Vec::new())),
            status_calls: Arc::new(AtomicU32::new(0)),
            set_failure: None,
            status_failure: None,
            queue_on_write: true,
            hang_status: true,
        };
        let hung_calls = hung.status_calls.clone();
        let (_hung_server, hung_connection) = scripted_gpu_peer(hung).await;
        let hung_source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(hung_connection),
        );
        let (hung_tx, hung_rx) = command_channel();
        let (hung_event_tx, mut hung_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let hung_worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            hung_rx,
            move |event| {
                let _ = hung_event_tx.send(event);
            },
            None,
            Some(hung_source),
        ));
        hung_tx
            .send(WorkerCommand::RefreshProductGpuStatus)
            .unwrap();
        let timed_out = loop {
            if let WorkerEvent::ProductGpuStatusRefresh(result) =
                tokio::time::timeout(Duration::from_secs(10), hung_event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result;
            }
        };
        assert!(
            matches!(timed_out, Err(ProviderError::Timeout(_))),
            "hung owner must time out, never fabricate a mode: {timed_out:?}"
        );
        assert_eq!(hung_calls.load(Ordering::SeqCst), 1);
        drop(hung_tx);
        hung_worker.abort();
    }

    #[tokio::test]
    async fn worker_loop_private_peer_gpu_reboot_queue_then_replacement_owner_has_no_restore() {
        // First owner: an accepted queued mode (reboot-required) must round-trip
        // through the ordinary worker and match the owner's read-back exactly.
        let owner = ScriptedGpuHardware {
            state: Arc::new(StdMutex::new(initial_product_gpu_state())),
            set_calls: Arc::new(StdMutex::new(Vec::new())),
            status_calls: Arc::new(AtomicU32::new(0)),
            set_failure: None,
            status_failure: None,
            queue_on_write: true,
            hang_status: false,
        };
        let owner_state = owner.state.clone();
        let owner_set_calls = owner.set_calls.clone();
        let (server, connection) = scripted_gpu_peer(owner).await;
        let client = connection.clone();
        let source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(connection),
        );
        let (tx, rx) = command_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            rx,
            move |event| {
                let _ = event_tx.send(event);
            },
            None,
            Some(source),
        ));
        tx.send(WorkerCommand::SetProductGpuMode { raw: 2 })
            .unwrap();
        let reply = loop {
            if let WorkerEvent::ProductGpu(result) =
                tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result.unwrap();
            }
        };
        assert_eq!(reply.queued_mode, 2);
        assert_eq!(reply.current_mode, 0);
        assert!(reply.reboot_required);
        assert_eq!(*owner_set_calls.lock().unwrap(), vec![2]);
        let read_back = orbis_session_client::ZbusHardwareProductGpuSource::new(client)
            .product_gpu_status()
            .await
            .unwrap();
        assert_eq!(read_back, reply);
        assert_eq!(*owner_state.lock().unwrap(), reply);

        // Restart: the original owner is gone and a brand-new Hardware1 owner
        // starts from its own authoritative state. No restore/replay write may
        // be issued, and the old queued state must not survive the restart.
        drop(server);
        drop(tx);
        worker.abort();

        let replacement = ScriptedGpuHardware {
            state: Arc::new(StdMutex::new(initial_product_gpu_state())),
            set_calls: Arc::new(StdMutex::new(Vec::new())),
            status_calls: Arc::new(AtomicU32::new(0)),
            set_failure: None,
            status_failure: None,
            queue_on_write: true,
            hang_status: false,
        };
        let replacement_state = replacement.state.clone();
        let replacement_set_calls = replacement.set_calls.clone();
        let (_replacement_server, replacement_connection) = scripted_gpu_peer(replacement).await;
        let replacement_client = replacement_connection.clone();
        let replacement_source: Arc<dyn HardwareProductGpuSource> = Arc::new(
            orbis_session_client::ZbusHardwareProductGpuSource::new(replacement_connection),
        );
        let (replacement_tx, replacement_rx) = command_channel();
        let (replacement_event_tx, mut replacement_event_rx) =
            tokio::sync::mpsc::unbounded_channel();
        let replacement_worker = tokio::spawn(run_worker_with_product_gpu(
            mock_runtime(),
            replacement_rx,
            move |event| {
                let _ = replacement_event_tx.send(event);
            },
            None,
            Some(replacement_source),
        ));
        replacement_tx
            .send(WorkerCommand::RefreshProductGpuStatus)
            .unwrap();
        let fresh = loop {
            if let WorkerEvent::ProductGpuStatusRefresh(result) =
                tokio::time::timeout(Duration::from_secs(5), replacement_event_rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                break result.unwrap();
            }
        };
        assert_eq!(fresh, initial_product_gpu_state());
        assert_eq!(fresh.queued_mode, u32::MAX);
        assert!(!fresh.reboot_required);
        assert!(
            replacement_set_calls.lock().unwrap().is_empty(),
            "startup must not replay a persisted mode into the replacement owner"
        );
        let fresh_direct =
            orbis_session_client::ZbusHardwareProductGpuSource::new(replacement_client)
                .product_gpu_status()
                .await
                .unwrap();
        assert_eq!(fresh_direct, initial_product_gpu_state());
        assert_eq!(
            *replacement_state.lock().unwrap(),
            initial_product_gpu_state()
        );
        drop(replacement_tx);
        replacement_worker.abort();
    }
}
