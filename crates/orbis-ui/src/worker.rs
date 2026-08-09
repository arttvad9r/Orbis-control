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

use orbis_application::{
    AppService, ChargeLimitCommandOutcome, GpuCommandOutcome, PerformanceCommandOutcome,
    SetChargeLimitError, SetGpuModeError, SetPerformanceError,
};
use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{BatteryProvider, GpuProvider, PerformanceProvider};
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

/// Последовательный worker для Performance Mode, GPU Mode и Battery Charge
/// Limit.
///
/// - `main_service` — владеемый `AppService<M>` для Performance/GPU;
/// - `battery_service` — владеемый `AppService<B>` для Battery Charge Limit;
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
/// команд; stale results внутри одного worker невозможны.
pub async fn run_worker<M, B, F>(
    main_service: AppService<M>,
    battery_service: AppService<B>,
    mut receiver: UnboundedReceiver<WorkerCommand>,
    mut emit: F,
) where
    M: PerformanceProvider + GpuProvider + Send + Sync + 'static,
    B: BatteryProvider + Send + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    // Команда, дочитанная при drain соседней Battery-группы (граница группы),
    // чтобы не потерять её при coalescing.
    let mut deferred_command: Option<WorkerCommand> = None;

    loop {
        let command = match deferred_command.take() {
            Some(cmd) => cmd,
            None => match receiver.recv().await {
                Some(cmd) => cmd,
                None => return, // канал закрыт и deferred пуст — штатное завершение
            },
        };

        let event = match command {
            WorkerCommand::SetPerformance(profile) => {
                WorkerEvent::Performance(main_service.set_performance(profile).await)
            }
            WorkerCommand::SetGpuMode { mode, confirmed } => {
                WorkerEvent::Gpu(main_service.set_gpu_mode(mode, confirmed).await)
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
                WorkerEvent::ChargeLimit(battery_service.set_charge_limit(latest_percent).await)
            }
            WorkerCommand::RefreshChargeLimit => {
                // Authoritative read-only refresh: обычная ordered команда,
                // не coalesce-ится и является границей для соседних
                // SetChargeLimit-групп.
                WorkerEvent::ChargeLimitRefresh(battery_service.charge_limit().await)
            }
        };
        emit(event);
    }
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
        BatteryProvider, GpuProvider, PerformanceProvider, Provider, ProviderHealth,
    };
    use orbis_test_support::devices::build_state_arc;

    use super::{WorkerCommand, WorkerEvent, command_channel, run_worker};

    fn services() -> (AppService<MockProvider>, AppService<MockProvider>) {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state));
        // Один Arc<MockProvider> для двух AppService: старые тесты проверяют
        // единый provider state; split в production использует разные backends.
        let main_service = AppService::new(provider.clone());
        let battery_service = AppService::new(provider);
        (main_service, battery_service)
    }

    #[tokio::test]
    async fn executes_command() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
            run_worker(services().0, services().1, rx, |_event| {}).await;
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

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
    impl BatteryProvider for ScriptedProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            Ok(ChargeLimit::new(
                true,
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
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

        // Начальное applied GPU state через публичный API до команды.
        let initial = main_service.gpu_state().await.expect("initial gpu state");

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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

        // Начальное GPU state через публичный provider API до ошибки.
        let before = (
            provider.requested_mode().await.unwrap(),
            provider.mux_state().await.unwrap(),
            provider.access_policy().await.unwrap(),
            provider.power_state().await.unwrap(),
        );

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
        assert_eq!(provider.mux_state().await.unwrap(), before.1);
        assert_eq!(provider.access_policy().await.unwrap(), before.2);
        assert_eq!(provider.power_state().await.unwrap(), before.3);

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

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
            .await;
        });

        tx.send(WorkerCommand::SetChargeLimit { percent: 40 })
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.percent.map(|p| p.get()), Some(40));
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
            .await;
        });

        // 83 не кратно 5: worker/application не должны округлять или проверять шаг.
        tx.send(WorkerCommand::SetChargeLimit { percent: 83 })
            .expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.percent.map(|p| p.get()), Some(83));
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
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
                assert_eq!(c.state.percent.map(|p| p.get()), Some(40));
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
                .percent
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

        // Начальный ChargeLimit через публичный provider API до ошибки.
        let before = provider.charge_limit().await.expect("initial charge limit");

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
        assert_ne!(after.percent.map(|p| p.get()), Some(40));

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
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
            .await;
        });

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimit(Ok(outcome)) => {
                assert_eq!(outcome.result, ApplyResult::Applied);
                assert_eq!(outcome.state.percent.map(|p| p.get()), Some(50));
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
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
                assert_eq!(c1.state.percent.map(|p| p.get()), Some(45));
                assert_eq!(g.state.requested, GpuMode::Optimized);
                assert_eq!(c2.state.percent.map(|p| p.get()), Some(55));
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
                .percent
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
                assert_eq!(c1.state.percent.map(|p| p.get()), Some(40));
                assert_eq!(p.state.current, PerformanceProfile::Silent);
                assert_eq!(c2.state.percent.map(|p| p.get()), Some(45));
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
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
            .await;
        });

        tx.send(WorkerCommand::RefreshChargeLimit).expect("send");

        let event = result_rx.recv().await.expect("event");
        match event {
            WorkerEvent::ChargeLimitRefresh(Ok(limit)) => {
                assert_eq!(limit.percent.map(|p| p.get()), Some(60));
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

        // Ошибка backend: refresh должен вернуть ошибку, а не mock default.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
            run_worker(services().0, services().1, rx, move |event| {
                let _ = result_tx.send(event);
            })
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
                assert_eq!(c1.state.percent.map(|p| p.get()), Some(45));
                // После Refresh-барьера новая группа выполняется отдельно.
                assert_eq!(c2.state.percent.map(|p| p.get()), Some(50));
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

    #[tokio::test]
    async fn routes_commands_to_distinct_providers() {
        let (main_provider, perf_calls, gpu_calls) = MainOnlyProvider::new();
        let (battery_provider, refresh_calls, set_calls) = BatteryOnlyProvider::new();

        let main_service = AppService::new(main_provider);
        let battery_service = AppService::new(battery_provider);

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(main_service, battery_service, rx, move |event| {
                let _ = result_tx.send(event);
            })
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

        // 4 события в FIFO-порядке.
        for _ in 0..4 {
            let _ = result_rx.recv().await.expect("event");
        }

        // Правильная маршрутизация: Performance/GPU -> main; Refresh/Set -> battery.
        assert_eq!(perf_calls.load(Ordering::SeqCst), 1);
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
}
