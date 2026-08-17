//! # orbis-ui worker
//!
//! Независимый от Slint последовательный async-worker для команд Performance
//! Mode, GPU Mode и Battery Charge Limit.
//!
//! - worker владеет `AppService<P>` и command receiver;
//! - команды обрабатываются строго по порядку (одна за другой, общий FIFO для
//!   всех типов команд); соседние queued `SetChargeLimit` coalesce до последнего
//!   значения группы (одна группа -> одно событие);
//! - полный результат `AppService` передаётся внешнему event sink без
//!   преобразований (ни строк, ни UI-типов, ни banner-текстов);
//! - worker не знает о UI-типах и свойствах окна (подключение event sink к
//!   событийному циклу интерфейса — следующий микрошаг);
//! - worker не принимает решений о Confirmation/Logout/Reboot, не меняет
//!   флаг `confirmed` и не выполняет clamp/округление percent;
//! - в production-коде worker не создаёт runtime, не вызывает `spawn` и не
//!   порождает потоки;
//! - после закрытия всех command senders `recv()` возвращает `None` и worker
//!   завершается.

use crate::composition::{
    ApplicationRuntime, BatteryServiceRuntime, GpuServicesRuntime, PerformanceServiceRuntime,
};
use orbis_application::{
    ChargeLimitCommandOutcome, GpuCommandOutcome, PerformanceCommandOutcome, PerformanceState,
    SetChargeLimitError, SetGpuModeError, SetPerformanceError,
};
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Типизированная команда worker-а (Performance Mode / GPU Mode / Battery).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerCommand {
    /// Установить профиль производительности.
    SetPerformance(PerformanceProfile),
    /// Установить GPU Mode.
    ///
    /// `confirmed` передаётся в `AppService`/provider без изменения: worker не
    /// решает, требуется ли Confirmation/Logout/Reboot.
    SetGpuMode {
        /// Запрашиваемый режим.
        mode: GpuMode,
        /// Флаг подтверждения пользователя.
        confirmed: bool,
    },
    /// Установить Battery Charge Limit.
    ///
    /// `percent` передаётся в `AppService`/provider без изменения: worker не
    /// выполняет clamp, округление или проверку шага 5.
    SetChargeLimit {
        /// Целевой процент лимита зарядки.
        percent: u8,
    },
    /// Authoritative read-only refresh Battery Charge Limit.
    ///
    /// Выполняет `AppService::charge_limit()` (новый provider read, без
    /// mutation). Обычная ordered команда/барьер: не coalesce-ится и не
    /// участвует в adjacent `SetChargeLimit` coalescing.
    RefreshChargeLimit,
    /// Authoritative read-only refresh всех трёх GPU hardware capabilities.
    ///
    /// Каждый concept читается независимо: failure одного не блокирует
    /// остальные. Никакого aggregate `gpu_state()` / fake GpuMode.
    RefreshGpuCapabilities,
    /// Authoritative read-only refresh Performance Mode (current + available).
    ///
    /// Выполняет `AppService::performance_state()` через отдельный real
    /// Performance read service. Обычная ordered команда/барьер: не
    /// coalesce-ится.
    RefreshPerformance,
    /// Re-probe все пять capabilities и whole-swap authoritative registry
    /// snapshot. Никаких writes. Software failure в probe pipeline
    /// сохраняет previous snapshot и не публикует partially-built registry.
    RefreshCapabilities,
}

/// Событие результата команды.
///
/// Переносит полный `Result` из `orbis_application` без преобразований:
/// `CommandOutcome` (ApplyResult + authoritative state) либо `CommandError`
/// (`Command` / `ReadBack` с сохранённым ApplyResult).
#[derive(Debug)]
pub enum WorkerEvent {
    /// Полный результат команды Performance.
    Performance(Result<PerformanceCommandOutcome, SetPerformanceError>),
    /// Полный результат команды GPU Mode.
    Gpu(Result<GpuCommandOutcome, SetGpuModeError>),
    /// Полный результат команды Battery Charge Limit.
    ChargeLimit(Result<ChargeLimitCommandOutcome, SetChargeLimitError>),
    /// Результат authoritative read-only refresh Battery Charge Limit.
    ///
    /// `Ok(ChargeLimit)` — фактическое authoritative значение provider;
    /// `Err(ProviderError)` — read недоступен (worker не подставляет mock/default).
    ChargeLimitRefresh(Result<orbis_core::battery::ChargeLimit, ProviderError>),
    /// Результат authoritative read-only refresh dGPU power state.
    GpuPowerRefresh(Result<GpuPowerState, ProviderError>),
    /// Результат authoritative read-only refresh physical MUX state.
    GpuMuxRefresh(Result<GpuMuxState, ProviderError>),
    /// Результат authoritative read-only refresh dGPU access policy.
    GpuAccessRefresh(Result<GpuAccessPolicy, ProviderError>),
    /// Результат authoritative read-only refresh Performance Mode.
    ///
    /// `Ok(PerformanceState)` — фактический authoritative current + available
    /// из отдельного real Performance read service; `Err(ProviderError)` — read
    /// недоступен (worker не подставляет mock/default).
    PerformanceRefresh(Result<PerformanceState, ProviderError>),
    /// Lifecycle event: capability registry snapshot replaced whole-swap.
    /// `Ok(generation)` — новая authoritative publication; `Err(String)` —
    /// software probe failure оставила authoritative snapshot без изменений.
    RegistryChange(Result<u64, orbis_capabilities::ProbeError>),
}

/// Создать command channel для worker.
///
/// `UnboundedSender::send()` синхронный и неблокирующий, FIFO-порядок
/// сохраняется; для первого Performance-среза не нужен bounded channel.
pub fn command_channel() -> (
    UnboundedSender<WorkerCommand>,
    UnboundedReceiver<WorkerCommand>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

/// Последовательный worker для Performance Mode, GPU Mode, Battery Charge Limit
/// и read-only GPU hardware capabilities.
///
/// - `runtime` — единая application composition boundary с группированными
///   GPU, Battery и Performance services;
/// - `receiver` — команды в порядке получения;
/// - `emit` — event sink, вызывается ровно один раз на каждую выполненную
///   команду или coalesced Battery-группу.
///
/// Queue coalescing Battery-команд: несколько подряд стоящих в очереди
/// `SetChargeLimit` объединяются — выполняется только последний percent
/// соседней группы; одна группа создаёт один `WorkerEvent::ChargeLimit`.
/// Performance/GPU/Refresh-команда является границей группы (не объединяется).
///
/// Invariants: одновременно выполняется не более одной команды; следующая
/// команда начинается только после завершения предыдущей (включая её
/// authoritative read-back внутри `AppService`); события выдаются в порядке
/// команд; stale results внутри одного worker невозможны. GPU capability reads
/// (power/mux/access) выполняются независимо: failure одного не блокирует
/// остальные.
///
pub async fn run_worker<G, B, R, F>(
    mut runtime: ApplicationRuntime<G, B, R>,
    mut receiver: UnboundedReceiver<WorkerCommand>,
    mut emit: F,
) where
    G: GpuServicesRuntime + 'static,
    B: BatteryServiceRuntime + 'static,
    R: PerformanceServiceRuntime + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    // Команда, дочитанная при drain соседней Battery-группы (граница группы),
    // чтобы не потерять её при coalescing.
    let mut deferred_command: Option<WorkerCommand> = None;

    loop {
        // `runtime` is owned exclusively here. Per command we borrow the
        // service fields by mut-reference; the borrow checker ensures we do
        // not keep those references alive across `replace_capabilities`.
        let command = match deferred_command.take() {
            Some(cmd) => cmd,
            None => match receiver.recv().await {
                Some(cmd) => cmd,
                None => return, // канал закрыт и deferred пуст — штатное завершение
            },
        };

        if matches!(command, WorkerCommand::RefreshCapabilities) {
            // Capability re-probe. Releases any previous per-command borrows
            // before mutating `runtime.capabilities`.
            let next_generation = runtime.capabilities().generation() + 1;
            let result = run_capability_refresh(&mut runtime, next_generation).await;
            match result {
                Ok(snapshot) => {
                    let generation = snapshot.generation();
                    runtime.replace_capabilities(snapshot);
                    emit(WorkerEvent::RegistryChange(Ok(generation)));
                }
                Err(error) => {
                    emit(WorkerEvent::RegistryChange(Err(error)));
                }
            }
            continue;
        }

        let gpu = &mut runtime.gpu;
        let battery = &mut runtime.battery;
        let performance = &mut runtime.performance;

        let event = match command {
            WorkerCommand::SetPerformance(profile) => {
                WorkerEvent::Performance(performance.set_performance(profile).await)
            }
            WorkerCommand::SetGpuMode { mode, confirmed } => {
                WorkerEvent::Gpu(gpu.set_gpu_mode(mode, confirmed).await)
            }
            WorkerCommand::SetChargeLimit { percent } => {
                // Coalescing соседних Battery-команд: выполняется только
                // последний percent соседней группы.
                let mut latest_percent = percent;
                loop {
                    match receiver.try_recv() {
                        Ok(WorkerCommand::SetChargeLimit { percent: next }) => {
                            latest_percent = next;
                        }
                        Ok(other) => {
                            // Граница группы (Performance/GPU/Refresh): сохранить
                            // команду для следующей итерации.
                            deferred_command = Some(other);
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => break,
                    }
                }
                WorkerEvent::ChargeLimit(battery.set_charge_limit(latest_percent).await)
            }
            WorkerCommand::RefreshChargeLimit => {
                // Authoritative read-only refresh: обычная ordered команда,
                // не coalesce-ится и является границей для соседних
                // SetChargeLimit-групп.
                WorkerEvent::ChargeLimitRefresh(battery.charge_limit().await)
            }
            WorkerCommand::RefreshGpuCapabilities => {
                // Три независимых authoritative read: failure одного concept
                // не блокирует остальные; emit-ится три события.
                let (power, mux, access) = gpu.refresh_gpu_capabilities().await;
                emit(WorkerEvent::GpuPowerRefresh(power));
                emit(WorkerEvent::GpuMuxRefresh(mux));
                emit(WorkerEvent::GpuAccessRefresh(access));
                continue;
            }
            WorkerCommand::RefreshPerformance => {
                // Authoritative read-only refresh через отдельный real
                // Performance read service; worker не подставляет mock/default.
                WorkerEvent::PerformanceRefresh(performance.performance_state().await)
            }
            WorkerCommand::RefreshCapabilities => unreachable!("handled above"),
        };
        emit(event);
    }
}

/// Re-probe all five capabilities and return a deterministic snapshot.
///
/// This helper exists so that the main `run_worker` dispatch can release the
/// service-field borrows before touching `runtime.capabilities`.
///
/// The whole-swap policy is preserved by construction: any
/// `ProbeError::Internal` or `ProbeError::ContractViolation` aborts the refresh
/// cycle without producing a partial snapshot, and the caller is responsible
/// for keeping the previous snapshot authoritative.
async fn run_capability_refresh<G, B, R>(
    runtime: &mut ApplicationRuntime<G, B, R>,
    next_generation: u64,
) -> Result<orbis_capabilities::CapabilityRegistrySnapshot, orbis_capabilities::ProbeError>
where
    G: GpuServicesRuntime,
    B: BatteryServiceRuntime,
    R: PerformanceServiceRuntime,
{
    use orbis_capabilities::CapabilityRegistryBuilder;

    let checked_at = std::time::SystemTime::now();
    let mut builder = CapabilityRegistryBuilder::new(next_generation, checked_at);

    // Performance probe: use the trait method that returns fully typed capability
    let performance = runtime.performance.probe_performance().await?;
    builder
        .add(orbis_core::FeatureId::Performance, performance)
        .map_err(|err| orbis_capabilities::ProbeError::ContractViolation(err.to_string()))?;

    // Battery probe: use the trait method that returns fully typed capability
    let battery = runtime.battery.probe_capability().await?;
    builder
        .add(orbis_core::FeatureId::ChargeLimit, battery)
        .map_err(|err| orbis_capabilities::ProbeError::ContractViolation(err.to_string()))?;

    // GPU probes: power, mux, access
    let gpu_entries = runtime.gpu.probe_primitives().await;
    for (feature, capability) in gpu_entries {
        builder
            .add(feature, capability)
            .map_err(|err| orbis_capabilities::ProbeError::ContractViolation(err.to_string()))?;
    }

    builder
        .build()
        .map_err(|err| orbis_capabilities::ProbeError::ContractViolation(err.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_application::{AppService, CommandError};
    use orbis_core::action::{ActionRequirement, ApplyResult};
    use orbis_core::battery::ChargeLimit;
    use orbis_core::battery::ChargeLimitBounds;
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
    use orbis_core::identity::BackendIdentity;
    use orbis_core::newtypes::Percent;
    use orbis_core::profile::PerformanceProfile;
    use orbis_providers::error::{ProviderError, ValidationResult};
    use orbis_providers::mock::{MockErrorMode, MockProvider};
    use orbis_providers::traits::{
        BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, GpuProvider,
        PerformanceProvider, Provider, ProviderHealth,
    };
    use orbis_test_support::devices::build_state_arc;
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::{ApplicationRuntime, WorkerCommand, WorkerEvent, command_channel, run_worker};
    use crate::composition::GpuServices;
    use orbis_capabilities::CapabilityRegistryBuilder;

    type Services = (
        AppService<MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
        AppService<MockProvider>,
    );

    fn services() -> Services {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state));
        // Один Arc<MockProvider> для всех сервисов: старые тесты проверяют
        // единый provider state; split в production использует разные backends.
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider);
        (
            main_service,
            battery_service,
            gpu_power_service,
            gpu_mux_service,
            gpu_access_service,
            performance_service,
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_worker_with_services<M, B, P, X, A, R, F>(
        main_service: AppService<M>,
        battery_service: AppService<B>,
        gpu_power_service: AppService<P>,
        gpu_mux_service: AppService<X>,
        gpu_access_service: AppService<A>,
        performance_service: AppService<R>,
        receiver: UnboundedReceiver<WorkerCommand>,
        emit: F,
    ) where
        M: GpuProvider + Send + Sync + 'static,
        B: BatteryProvider + Send + Sync + 'static,
        P: GpuPowerProvider + Send + Sync + 'static,
        X: GpuMuxProvider + Send + Sync + 'static,
        A: GpuAccessProvider + Send + Sync + 'static,
        R: PerformanceProvider + Send + Sync + 'static,
        F: FnMut(WorkerEvent) + Send + 'static,
    {
        let snapshot = CapabilityRegistryBuilder::new(1, std::time::SystemTime::now())
            .build()
            .expect("empty registry snapshot must build");
        run_worker(
            ApplicationRuntime::new_with_snapshot(
                GpuServices::new(
                    main_service,
                    gpu_power_service,
                    gpu_mux_service,
                    gpu_access_service,
                ),
                battery_service,
                performance_service,
                snapshot,
            ),
            receiver,
            emit,
        )
        .await;
    }

    #[tokio::test]
    async fn executes_command() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::Performance(Ok(outcome)) => {
                assert_eq!(outcome.state.current, PerformanceProfile::Silent);
                assert_eq!(outcome.state.available.len(), 3);
            }
            other => panic!("ожидался Ok(Performance), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn fifo_order() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send1");
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Turbo))
            .expect("send2");

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");

        match (first, second) {
            (WorkerEvent::Performance(Ok(a)), WorkerEvent::Performance(Ok(b))) => {
                assert_eq!(a.state.current, PerformanceProfile::Silent);
                assert_eq!(b.state.current, PerformanceProfile::Turbo);
            }
            other => panic!("ожидались два Ok, получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn closing_sender_finishes_worker() {
        let (tx, rx) = command_channel();
        drop(tx); // закрыть все senders до запуска

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                |_event| {},
            )
            .await;
        });

        // worker должен завершиться нормально (recv -> None), без зависания/panic
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish after sender close")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn command_error_not_lost() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::Performance(Err(CommandError::Command(
                ProviderError::BackendUnavailable(_),
            ))) => {}
            other => panic!("ожидался Err(Command(BackendUnavailable)), получено: {other:?}"),
        }

        // provider state не изменился
        state.write().await.error_mode = MockErrorMode::None;
        assert_eq!(
            provider.current_profile().await.unwrap(),
            PerformanceProfile::Balanced
        );

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn send_before_worker_readiness() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // Отправить команды ДО запуска worker: UnboundedSender::send синхронный.
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send1");
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Turbo))
            .expect("send2");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");
        match (first, second) {
            (WorkerEvent::Performance(Ok(a)), WorkerEvent::Performance(Ok(b))) => {
                assert_eq!(a.state.current, PerformanceProfile::Silent);
                assert_eq!(b.state.current, PerformanceProfile::Turbo);
            }
            other => panic!("ожидались два Ok, получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // Scripted GPU/Performance provider (только для теста confirmed)
    // -----------------------------------------------------------------------

    /// Тестовый провайдер: фиксирует последние полученные `GpuMode` и `confirmed`
    /// без изменения; остальные методы возвращают детерминированные значения.
    ///
    /// Реализует те же traits, что и production `run_worker` требует от `P`
    /// (`PerformanceProvider + GpuProvider + BatteryProvider`); Performance и
    /// Battery реализации нужны только из-за общего bound.
    struct ScriptedProvider {
        last: tokio::sync::RwLock<Option<(GpuMode, bool)>>,
        profile: tokio::sync::RwLock<PerformanceProfile>,
    }

    impl ScriptedProvider {
        fn new() -> Self {
            Self {
                last: tokio::sync::RwLock::new(None),
                profile: tokio::sync::RwLock::new(PerformanceProfile::Balanced),
            }
        }

        async fn last(&self) -> Option<(GpuMode, bool)> {
            *self.last.read().await
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn id(&self) -> &'static str {
            "scripted-worker"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-worker")
        }

        fn timeout(&self) -> Duration {
            Duration::from_millis(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-worker: {feature} недоступен")
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
            Ok(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])
        }

        async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
            Ok(*self.profile.read().await)
        }

        async fn set_profile(
            &self,
            profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            *self.profile.write().await = profile;
            Ok(ApplyResult::Applied)
        }

        async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    #[async_trait]
    impl GpuProvider for ScriptedProvider {
        async fn requested_mode(&self) -> Result<GpuMode, ProviderError> {
            Ok(self
                .last
                .read()
                .await
                .map(|(mode, _)| mode)
                .unwrap_or(GpuMode::Standard))
        }

        async fn set_mode(
            &self,
            mode: GpuMode,
            confirmed: bool,
        ) -> Result<ApplyResult, ProviderError> {
            *self.last.write().await = Some((mode, confirmed));
            Ok(ApplyResult::Applied)
        }

        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            Ok(GpuMuxState::Integrated)
        }

        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            Ok(GpuAccessPolicy::Unblocked)
        }

        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            Ok(GpuPowerState::Active)
        }

        fn requirement_for(&self, _mode: GpuMode) -> ActionRequirement {
            ActionRequirement::None
        }

        fn validate_mode(&self, _mode: GpuMode) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    #[async_trait]
    impl GpuPowerProvider for ScriptedProvider {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            Ok(GpuPowerState::Active)
        }
    }

    #[async_trait]
    impl GpuMuxProvider for ScriptedProvider {
        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            Ok(GpuMuxState::Integrated)
        }
    }

    #[async_trait]
    impl GpuAccessProvider for ScriptedProvider {
        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            Ok(GpuAccessPolicy::Unblocked)
        }
    }

    #[async_trait]
    impl BatteryProvider for ScriptedProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            Ok(ChargeLimit::new(
                true,
                Some(Percent::new(80).expect("const")),
                Some(Percent::new(80).expect("const")),
                Some(
                    ChargeLimitBounds::new(
                        Percent::new(40).expect("const"),
                        Percent::new(100).expect("const"),
                        1,
                    )
                    .expect("valid"),
                ),
            )
            .expect("valid"))
        }

        async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
            Ok(ApplyResult::Applied)
        }

        async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
            Ok(ApplyResult::Applied)
        }

        fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    // -----------------------------------------------------------------------
    // GPU worker-тесты
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn executes_gpu_applied_command() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: false,
        })
        .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::Gpu(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.requested, GpuMode::Optimized);
                assert_eq!(outcome.state.mux, GpuMuxState::Integrated);
                assert_eq!(outcome.state.access_policy, GpuAccessPolicy::Blocked);
                assert_eq!(outcome.state.power_state, GpuPowerState::Active);
            }
            other => panic!("ожидался Ok(Gpu), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn gpu_ultimate_pending_not_lost() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        // Начальное applied GPU state через публичный API до команды.
        let initial = main_service.gpu_state().await.expect("initial gpu state");

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Ultimate,
            confirmed: true,
        })
        .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::Gpu(Ok(outcome)) => {
                assert_eq!(
                    outcome.result,
                    ApplyResult::Pending {
                        requirement: ActionRequirement::Reboot
                    }
                );
                assert_eq!(
                    outcome.result.requirement(),
                    Some(ActionRequirement::Reboot)
                );
                assert_eq!(outcome.state.requested, GpuMode::Ultimate);
                assert_eq!(outcome.state.requirement, ActionRequirement::Reboot);
                // Applied состояние не меняется до reboot.
                assert_eq!(outcome.state.mux, initial.mux);
                assert_eq!(outcome.state.access_policy, initial.access_policy);
                assert_eq!(outcome.state.power_state, initial.power_state);
            }
            other => panic!("ожидался Ok(Gpu) с Pending/Reboot, получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn mixed_commands_preserve_fifo_order() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // Отправить все команды до запуска worker: порядок канала фиксирован.
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send1");
        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: false,
        })
        .expect("send2");
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Turbo))
            .expect("send3");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");
        let third = result_rx.recv().await.expect("event3");
        match (first, second, third) {
            (
                WorkerEvent::Performance(Ok(a)),
                WorkerEvent::Gpu(Ok(g)),
                WorkerEvent::Performance(Ok(b)),
            ) => {
                assert_eq!(a.state.current, PerformanceProfile::Silent);
                assert_eq!(g.state.requested, GpuMode::Optimized);
                assert_eq!(b.state.current, PerformanceProfile::Turbo);
            }
            other => panic!("ожидался порядок Perf(Gpu(Perf)), получено: {other:?}"),
        }

        // Финальное authoritative provider state.
        assert_eq!(
            provider.current_profile().await.unwrap(),
            PerformanceProfile::Turbo
        );
        assert_eq!(provider.requested_mode().await.unwrap(), GpuMode::Optimized);

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn gpu_command_error_not_lost() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        // Начальное GPU state через публичный provider API до ошибки.
        let before = (
            provider.requested_mode().await.unwrap(),
            GpuProvider::mux_state(provider.as_ref()).await.unwrap(),
            GpuProvider::access_policy(provider.as_ref()).await.unwrap(),
            GpuProvider::power_state(provider.as_ref()).await.unwrap(),
        );

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: false,
        })
        .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::Gpu(Err(CommandError::Command(ProviderError::BackendUnavailable(_)))) => {}
            other => panic!("ожидался Err(Command(BackendUnavailable)), получено: {other:?}"),
        }

        // provider state не изменился
        state.write().await.error_mode = MockErrorMode::None;
        assert_eq!(provider.requested_mode().await.unwrap(), before.0);
        assert_eq!(
            GpuProvider::mux_state(provider.as_ref()).await.unwrap(),
            before.1
        );
        assert_eq!(
            GpuProvider::access_policy(provider.as_ref()).await.unwrap(),
            before.2
        );
        assert_eq!(
            GpuProvider::power_state(provider.as_ref()).await.unwrap(),
            before.3
        );

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn gpu_confirmed_flag_passed_unmodified() {
        let provider = Arc::new(ScriptedProvider::new());
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Standard,
            confirmed: false,
        })
        .expect("send1");
        let first = result_rx.recv().await.expect("event1");
        assert!(matches!(first, WorkerEvent::Gpu(Ok(_))));
        assert_eq!(provider.last().await, Some((GpuMode::Standard, false)));

        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: true,
        })
        .expect("send2");
        let second = result_rx.recv().await.expect("event2");
        assert!(matches!(second, WorkerEvent::Gpu(Ok(_))));
        assert_eq!(provider.last().await, Some((GpuMode::Optimized, true)));

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // Battery worker-тесты
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn executes_charge_limit_command() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.configured_percent.map(|p| p.get()), Some(40));
                assert!(outcome.state.enabled);
                let b = outcome.state.bounds.expect("mock bounds");
                assert_eq!(b.min.get(), 40);
                assert_eq!(b.max.get(), 100);
                assert_eq!(b.step, 1);
            }
            other => panic!("ожидался Ok(ChargeLimit), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn charge_limit_value_passed_unmodified() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        // 83 не кратно 5: worker/application не должны округлять или проверять шаг.
        tx.send(WorkerCommand::SetChargeLimit { percent: 83 })
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.configured_percent.map(|p| p.get()), Some(83));
            }
            other => panic!("ожидался Ok(ChargeLimit), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn performance_gpu_and_charge_preserve_fifo_order() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // Отправить все команды до запуска worker: порядок канала фиксирован.
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send1");
        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: false,
        })
        .expect("send2");
        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send3");
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Turbo))
            .expect("send4");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");
        let third = result_rx.recv().await.expect("event3");
        let fourth = result_rx.recv().await.expect("event4");
        match (first, second, third, fourth) {
            (
                WorkerEvent::Performance(Ok(a)),
                WorkerEvent::Gpu(Ok(g)),
                WorkerEvent::ChargeLimit(Ok(c)),
                WorkerEvent::Performance(Ok(b)),
            ) => {
                assert_eq!(a.state.current, PerformanceProfile::Silent);
                assert_eq!(g.state.requested, GpuMode::Optimized);
                assert_eq!(c.state.configured_percent.map(|p| p.get()), Some(40));
                assert_eq!(b.state.current, PerformanceProfile::Turbo);
            }
            other => panic!("ожидался порядок Perf(Gpu(Charge(Perf)), получено: {other:?}"),
        }

        // Финальное authoritative provider state через публичные traits.
        assert_eq!(
            provider.current_profile().await.unwrap(),
            PerformanceProfile::Turbo
        );
        assert_eq!(provider.requested_mode().await.unwrap(), GpuMode::Optimized);
        assert_eq!(
            provider
                .charge_limit()
                .await
                .unwrap()
                .configured_percent
                .map(|p| p.get()),
            Some(40)
        );

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn charge_limit_command_error_not_lost() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        // Начальный ChargeLimit через публичный provider API до ошибки.
        let before = provider.charge_limit().await.expect("initial charge limit");

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Err(CommandError::Command(
                ProviderError::BackendUnavailable(_),
            ))) => {}
            other => panic!("ожидался Err(Command(BackendUnavailable)), получено: {other:?}"),
        }

        // ChargeLimit не изменился (percent не стал 40).
        state.write().await.error_mode = MockErrorMode::None;
        let after = provider
            .charge_limit()
            .await
            .expect("charge limit unchanged");
        assert_eq!(after, before);
        assert_ne!(after.configured_percent.map(|p| p.get()), Some(40));

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // Coalescing worker-тесты
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn consecutive_charge_commands_coalesce_to_latest() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // Соседняя группа Battery-команд до запуска worker: порядок канала
        // фиксирован, coalescing выполняется внутри worker.
        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send1");
        tx.send(WorkerCommand::SetChargeLimit { percent: 45 })
            .expect("send2");
        tx.send(WorkerCommand::SetChargeLimit { percent: 50 })
            .expect("send3");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.configured_percent.map(|p| p.get()), Some(50));
            }
            other => panic!("ожидался Ok(ChargeLimit), получено: {other:?}"),
        }

        // Закрыть sender: worker завершается, дополнительных событий нет.
        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
        match result_rx.try_recv() {
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {}
            other => panic!("ожидалось отсутствие дополнительных событий, получено: {other:?}"),
        }
    }

    #[tokio::test]
    async fn charge_coalescing_preserves_non_charge_boundary() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send1");
        tx.send(WorkerCommand::SetChargeLimit { percent: 45 })
            .expect("send2");
        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: false,
        })
        .expect("send3");
        tx.send(WorkerCommand::SetChargeLimit { percent: 50 })
            .expect("send4");
        tx.send(WorkerCommand::SetChargeLimit { percent: 55 })
            .expect("send5");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");
        let third = result_rx.recv().await.expect("event3");
        match (first, second, third) {
            (
                WorkerEvent::ChargeLimit(Ok(c1)),
                WorkerEvent::Gpu(Ok(g)),
                WorkerEvent::ChargeLimit(Ok(c2)),
            ) => {
                assert_eq!(c1.state.configured_percent.map(|p| p.get()), Some(45));
                assert_eq!(g.state.requested, GpuMode::Optimized);
                assert_eq!(c2.state.configured_percent.map(|p| p.get()), Some(55));
            }
            other => panic!("ожидался порядок Charge(Gpu(Charge), получено: {other:?}"),
        }

        // Финальное authoritative provider state через публичные traits.
        assert_eq!(provider.requested_mode().await.unwrap(), GpuMode::Optimized);
        assert_eq!(
            provider
                .charge_limit()
                .await
                .unwrap()
                .configured_percent
                .map(|p| p.get()),
            Some(55)
        );

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn charge_coalescing_preserves_performance_boundary() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send1");
        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send2");
        tx.send(WorkerCommand::SetChargeLimit { percent: 45 })
            .expect("send3");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");
        let third = result_rx.recv().await.expect("event3");
        match (first, second, third) {
            (
                WorkerEvent::ChargeLimit(Ok(c1)),
                WorkerEvent::Performance(Ok(p)),
                WorkerEvent::ChargeLimit(Ok(c2)),
            ) => {
                assert_eq!(c1.state.configured_percent.map(|p| p.get()), Some(40));
                assert_eq!(p.state.current, PerformanceProfile::Silent);
                assert_eq!(c2.state.configured_percent.map(|p| p.get()), Some(45));
            }
            other => panic!("ожидался порядок Charge(Perf(Charge), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // RefreshChargeLimit worker-тесты
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn refresh_charge_limit_reads_authoritative_state() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        // Прямое изменение provider вне worker: refresh должен вернуть
        // актуальное значение, а не предыдущее UI-состояние.
        provider
            .set_charge_limit(60)
            .await
            .expect("set charge limit");

        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::RefreshChargeLimit).expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimitRefresh(Ok(limit)) => {
                assert_eq!(limit.configured_percent.map(|p| p.get()), Some(60));
                assert!(limit.enabled);
            }
            other => panic!("ожидался Ok(ChargeLimitRefresh), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn refresh_charge_limit_error_is_preserved() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());

        // Ошибка backend: refresh должен вернуть ошибку, а не mock default.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::RefreshChargeLimit).expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimitRefresh(Err(ProviderError::BackendUnavailable(_))) => {}
            other => panic!("ожидался Err(BackendUnavailable), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn refresh_charge_limit_is_barrier_not_coalesced() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // SetChargeLimit(40), SetChargeLimit(45) — одна adjacent группа -> 45;
        // RefreshChargeLimit — обычная ordered команда/барьер;
        // SetChargeLimit(50) — новая группа -> 50.
        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send1");
        tx.send(WorkerCommand::SetChargeLimit { percent: 45 })
            .expect("send2");
        tx.send(WorkerCommand::RefreshChargeLimit).expect("send3");
        tx.send(WorkerCommand::SetChargeLimit { percent: 50 })
            .expect("send4");

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                services().0,
                services().1,
                services().2,
                services().3,
                services().4,
                services().5,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        let first = result_rx.recv().await.expect("event1");
        let second = result_rx.recv().await.expect("event2");
        let third = result_rx.recv().await.expect("event3");
        match (first, second, third) {
            (
                WorkerEvent::ChargeLimit(Ok(c1)),
                WorkerEvent::ChargeLimitRefresh(Ok(_)),
                WorkerEvent::ChargeLimit(Ok(c2)),
            ) => {
                // Первая adjacent группа coalesce-ится до 45 (last-wins).
                assert_eq!(c1.state.configured_percent.map(|p| p.get()), Some(45));
                // После Refresh-барьера новая группа выполняется отдельно.
                assert_eq!(c2.state.configured_percent.map(|p| p.get()), Some(50));
            }
            other => panic!("ожидался порядок Charge(Refresh(Charge), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // Regression: routing между main (Performance/GPU) и battery provider
    // -----------------------------------------------------------------------

    /// Тестовый provider только для Performance/GPU: реализует именно те traits,
    /// которые `run_worker` требует от `M`. Если worker (неверно) направит сюда
    /// Battery-команду — компиляции бы не было; на runtime счётчики это видно.
    struct MainOnlyProvider {
        perf_calls: Arc<AtomicUsize>,
        gpu_calls: Arc<AtomicUsize>,
        profile: tokio::sync::RwLock<PerformanceProfile>,
    }

    impl MainOnlyProvider {
        fn new() -> (Arc<Self>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let provider = Arc::new(Self {
                perf_calls: Arc::new(AtomicUsize::new(0)),
                gpu_calls: Arc::new(AtomicUsize::new(0)),
                profile: tokio::sync::RwLock::new(PerformanceProfile::Balanced),
            });
            (
                provider.clone(),
                provider.perf_calls.clone(),
                provider.gpu_calls.clone(),
            )
        }
    }

    #[async_trait]
    impl Provider for MainOnlyProvider {
        fn id(&self) -> &'static str {
            "main-only"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("main-only")
        }

        fn timeout(&self) -> Duration {
            Duration::from_millis(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("main-only: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl PerformanceProvider for MainOnlyProvider {
        async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
            Ok(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])
        }

        async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
            Ok(*self.profile.read().await)
        }

        async fn set_profile(
            &self,
            profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            self.perf_calls.fetch_add(1, Ordering::SeqCst);
            *self.profile.write().await = profile;
            Ok(ApplyResult::Applied)
        }

        async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    #[async_trait]
    impl GpuProvider for MainOnlyProvider {
        async fn requested_mode(&self) -> Result<GpuMode, ProviderError> {
            Ok(GpuMode::Standard)
        }

        async fn set_mode(
            &self,
            mode: GpuMode,
            _confirmed: bool,
        ) -> Result<ApplyResult, ProviderError> {
            self.gpu_calls.fetch_add(1, Ordering::SeqCst);
            let _ = mode;
            Ok(ApplyResult::Applied)
        }

        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            Ok(GpuMuxState::Integrated)
        }

        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            Ok(GpuAccessPolicy::Unblocked)
        }

        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            Ok(GpuPowerState::Active)
        }

        fn requirement_for(&self, _mode: GpuMode) -> ActionRequirement {
            ActionRequirement::None
        }

        fn validate_mode(&self, _mode: GpuMode) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    /// Тестовый provider только для Battery: реализует именно traits, которые
    /// `run_worker` требует от `B`. Если worker направит сюда Performance/GPU —
    /// это было бы compile error; runtime счётчик refresh/set виден.
    struct BatteryOnlyProvider {
        refresh_calls: Arc<AtomicUsize>,
        set_calls: Arc<AtomicUsize>,
        limit: tokio::sync::RwLock<u8>,
    }

    impl BatteryOnlyProvider {
        fn new() -> (Arc<Self>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let provider = Arc::new(Self {
                refresh_calls: Arc::new(AtomicUsize::new(0)),
                set_calls: Arc::new(AtomicUsize::new(0)),
                limit: tokio::sync::RwLock::new(80),
            });
            (
                provider.clone(),
                provider.refresh_calls.clone(),
                provider.set_calls.clone(),
            )
        }
    }

    #[async_trait]
    impl Provider for BatteryOnlyProvider {
        fn id(&self) -> &'static str {
            "battery-only"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("battery-only")
        }

        fn timeout(&self) -> Duration {
            Duration::from_millis(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("battery-only: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl BatteryProvider for BatteryOnlyProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            self.refresh_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ChargeLimit::new(
                true,
                Some(Percent::new(*self.limit.read().await).expect("const")),
                Some(Percent::new(*self.limit.read().await).expect("const")),
                None,
            )
            .expect("valid"))
        }

        async fn set_charge_limit(&self, percent: u8) -> Result<ApplyResult, ProviderError> {
            self.set_calls.fetch_add(1, Ordering::SeqCst);
            *self.limit.write().await = percent;
            Ok(ApplyResult::Applied)
        }

        async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }

        fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    #[async_trait]
    impl GpuPowerProvider for BatteryOnlyProvider {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            Ok(GpuPowerState::Active)
        }
    }

    #[async_trait]
    impl GpuMuxProvider for BatteryOnlyProvider {
        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            Ok(GpuMuxState::Integrated)
        }
    }

    #[async_trait]
    impl GpuAccessProvider for BatteryOnlyProvider {
        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            Ok(GpuAccessPolicy::Unblocked)
        }
    }

    #[async_trait]
    impl PerformanceProvider for BatteryOnlyProvider {
        async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
            Ok(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])
        }

        async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
            Ok(PerformanceProfile::Balanced)
        }

        async fn set_profile(
            &self,
            _profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Err(ProviderError::Unsupported("read-only".into()))
        }

        async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
            ValidationResult::invalid("read-only")
        }
    }

    #[tokio::test]
    async fn routes_commands_to_distinct_providers() {
        let (main_provider, main_perf_calls, gpu_calls) = MainOnlyProvider::new();
        let (performance_provider, performance_calls, _) = MainOnlyProvider::new();
        let (battery_provider, refresh_calls, set_calls) = BatteryOnlyProvider::new();

        let main_service = AppService::new(main_provider);
        let battery_service = AppService::new(battery_provider.clone());
        let gpu_power_service = AppService::new(battery_provider.clone());
        let gpu_mux_service = AppService::new(battery_provider.clone());
        let gpu_access_service = AppService::new(battery_provider.clone());
        let performance_service = AppService::new(performance_provider);

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
            .expect("send1");
        tx.send(WorkerCommand::SetGpuMode {
            mode: GpuMode::Optimized,
            confirmed: false,
        })
        .expect("send2");
        tx.send(WorkerCommand::RefreshChargeLimit).expect("send3");
        tx.send(WorkerCommand::SetChargeLimit { percent: 60 })
            .expect("send4");

        // 4 события в FIFO-порядке. Performance должен вернуть authoritative
        // state отдельного performance provider, а не main provider.
        match result_rx.recv().await.expect("performance event") {
            WorkerEvent::Performance(Ok(outcome)) => {
                assert_eq!(outcome.state.current, PerformanceProfile::Silent);
                assert_eq!(outcome.state.available.len(), 3);
            }
            other => panic!("ожидался Ok(Performance), получено: {other:?}"),
        }
        for _ in 0..3 {
            let _ = result_rx.recv().await.expect("event");
        }

        // Правильная маршрутизация: GPU -> main; Performance -> performance;
        // Refresh/Set -> battery.
        assert_eq!(main_perf_calls.load(Ordering::SeqCst), 0);
        assert_eq!(performance_calls.load(Ordering::SeqCst), 1);
        assert_eq!(gpu_calls.load(Ordering::SeqCst), 1);
        // RefreshChargeLimit -> 1 вызов charge_limit(); SetChargeLimit -> 1 вызов
        // set_charge_limit() + обязательный authoritative read-back через
        // charge_limit() внутри AppService, поэтому refresh_calls == 2.
        assert_eq!(refresh_calls.load(Ordering::SeqCst), 2);
        assert_eq!(set_calls.load(Ordering::SeqCst), 1);

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // RefreshGpuCapabilities: независимость concepts
    // -----------------------------------------------------------------------

    struct CapabilityProvider {
        power: Result<GpuPowerState, ProviderError>,
        mux: Result<GpuMuxState, ProviderError>,
        access: Result<GpuAccessPolicy, ProviderError>,
    }

    impl Provider for CapabilityProvider {
        fn id(&self) -> &'static str {
            "capability"
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("capability")
        }
        fn timeout(&self) -> Duration {
            Duration::from_millis(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("capability: {feature} недоступен")
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl GpuPowerProvider for CapabilityProvider {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            match &self.power {
                Ok(v) => Ok(*v),
                Err(e) => Err(ProviderError::Dbus(e.to_string())),
            }
        }
    }
    #[async_trait]
    impl GpuMuxProvider for CapabilityProvider {
        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            match &self.mux {
                Ok(v) => Ok(*v),
                Err(e) => Err(ProviderError::Dbus(e.to_string())),
            }
        }
    }
    #[async_trait]
    impl GpuAccessProvider for CapabilityProvider {
        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            match &self.access {
                Ok(v) => Ok(*v),
                Err(e) => Err(ProviderError::Dbus(e.to_string())),
            }
        }
    }

    #[tokio::test]
    async fn gpu_capability_refresh_independent_failures() {
        // power — error; mux/access — success. Refresh должен дать 3 события:
        // power Err, mux Ok, access Ok (failure одного не блокирует остальные).
        let cap = Arc::new(CapabilityProvider {
            power: Err(ProviderError::Dbus("power down".into())),
            mux: Ok(GpuMuxState::Discrete),
            access: Ok(GpuAccessPolicy::Blocked),
        });
        // main/battery — MockProvider (для run_worker bound; в этом тесте не
        // используются); GPU capability сервисы — CapabilityProvider.
        let (main_service, battery_service, _, _, _, performance_service) = services();
        let gpu_power_service = AppService::new(cap.clone());
        let gpu_mux_service = AppService::new(cap.clone());
        let gpu_access_service = AppService::new(cap);

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::RefreshGpuCapabilities)
            .expect("send");

        let mut saw_power_err = false;
        let mut saw_mux_ok = false;
        let mut saw_access_ok = false;
        for _ in 0..3 {
            match result_rx.recv().await.expect("event") {
                WorkerEvent::GpuPowerRefresh(Err(ProviderError::Dbus(_))) => saw_power_err = true,
                WorkerEvent::GpuPowerRefresh(Ok(_)) => {}
                WorkerEvent::GpuMuxRefresh(Ok(GpuMuxState::Discrete)) => saw_mux_ok = true,
                WorkerEvent::GpuMuxRefresh(_) => {}
                WorkerEvent::GpuAccessRefresh(Ok(GpuAccessPolicy::Blocked)) => saw_access_ok = true,
                WorkerEvent::GpuAccessRefresh(_) => {}
                _ => panic!("неожиданное событие"),
            }
        }
        assert!(saw_power_err, "power error не доставлен");
        assert!(saw_mux_ok, "mux Ok не доставлен");
        assert!(saw_access_ok, "access Ok не доставлен");

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    // -----------------------------------------------------------------------
    // RefreshPerformance: отдельный real Performance read service
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn refresh_performance_reads_authoritative_state() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        // Прямое изменение provider вне worker: refresh должен вернуть
        // актуальное значение, а не предыдущее UI-состояние.
        provider
            .set_profile(PerformanceProfile::Turbo)
            .await
            .expect("set profile");

        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider);
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::RefreshPerformance).expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::PerformanceRefresh(Ok(state)) => {
                assert_eq!(state.current, PerformanceProfile::Turbo);
                assert_eq!(state.available.len(), 3);
            }
            other => panic!("ожидался Ok(PerformanceRefresh), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn refresh_performance_error_is_preserved() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider);

        // Ошибка backend: refresh должен вернуть ошибку, а не mock default.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::RefreshPerformance).expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::PerformanceRefresh(Err(ProviderError::BackendUnavailable(_))) => {}
            other => panic!("ожидался Err(BackendUnavailable), получено: {other:?}"),
        }

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }

    #[tokio::test]
    async fn refresh_performance_does_not_mutate() {
        let provider = Arc::new(MockProvider::new(
            build_state_arc("zephyrus-full").expect("profile exists"),
        ));
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider.clone());
        let gpu_power_service = AppService::new(provider.clone());
        let gpu_mux_service = AppService::new(provider.clone());
        let gpu_access_service = AppService::new(provider.clone());
        let performance_service = AppService::new(provider.clone());
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker_with_services(
                main_service,
                battery_service,
                gpu_power_service,
                gpu_mux_service,
                gpu_access_service,
                performance_service,
                rx,
                move |event| {
                    let _ = result_tx.send(event);
                },
            )
            .await;
        });

        tx.send(WorkerCommand::RefreshPerformance).expect("send");
        let event = result_rx.recv().await.expect("event");
        assert!(matches!(event, WorkerEvent::PerformanceRefresh(Ok(_))));

        // Refresh — read-only: current profile не изменён.
        assert_eq!(
            provider.current_profile().await.unwrap(),
            PerformanceProfile::Balanced
        );

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker should finish")
            .expect("worker must not panic");
    }
}
