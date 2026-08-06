//! # orbis-ui worker
//!
//! Независимый от Slint последовательный async-worker для команд Performance
//! Mode и GPU Mode.
//!
//! - worker владеет `AppService<P>` и command receiver;
//! - команды обрабатываются строго по порядку (одна за другой, общий FIFO для
//!   всех типов команд);
//! - полный результат `AppService` передаётся внешнему event sink без
//!   преобразований (ни строк, ни UI-типов, ни banner-текстов);
//! - worker не знает о UI-типах и свойствах окна (подключение event sink к
//!   событийному циклу интерфейса — следующий микрошаг);
//! - worker не принимает решений о Confirmation/Logout/Reboot и не меняет
//!   флаг `confirmed`;
//! - в production-коде worker не создаёт runtime, не вызывает `spawn` и не
//!   порождает потоки;
//! - после закрытия всех command senders `recv()` возвращает `None` и worker
//!   завершается.

use orbis_application::{
    AppService, GpuCommandOutcome, PerformanceCommandOutcome, SetGpuModeError, SetPerformanceError,
};
use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::traits::{GpuProvider, PerformanceProvider};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Типизированная команда worker-а (Performance Mode / GPU Mode).
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

/// Последовательный worker для Performance Mode и GPU Mode.
///
/// - `service` — владеемый `AppService<P>`;
/// - `receiver` — команды в порядке получения;
/// - `emit` — event sink, вызывается ровно один раз на каждую команду.
///
/// Invariants: одновременно выполняется не более одной команды; следующая
/// команда начинается только после завершения предыдущей (включая её
/// authoritative read-back внутри `AppService`); события выдаются в порядке
/// команд; stale results внутри одного worker невозможны.
pub async fn run_worker<P, F>(
    service: AppService<P>,
    mut receiver: UnboundedReceiver<WorkerCommand>,
    mut emit: F,
) where
    P: PerformanceProvider + GpuProvider + Send + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    while let Some(command) = receiver.recv().await {
        let event = match command {
            WorkerCommand::SetPerformance(profile) => {
                WorkerEvent::Performance(service.set_performance(profile).await)
            }
            WorkerCommand::SetGpuMode { mode, confirmed } => {
                WorkerEvent::Gpu(service.set_gpu_mode(mode, confirmed).await)
            }
        };
        emit(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_application::{AppService, CommandError};
    use orbis_core::action::{ActionRequirement, ApplyResult};
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
    use orbis_core::identity::BackendIdentity;
    use orbis_core::profile::PerformanceProfile;
    use orbis_providers::error::{ProviderError, ValidationResult};
    use orbis_providers::mock::{MockErrorMode, MockProvider};
    use orbis_providers::traits::{GpuProvider, PerformanceProvider, Provider, ProviderHealth};
    use orbis_test_support::devices::build_state_arc;

    use super::{WorkerCommand, WorkerEvent, command_channel, run_worker};

    fn service() -> AppService<MockProvider> {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        AppService::new(Arc::new(MockProvider::new(state)))
    }

    #[tokio::test]
    async fn executes_command() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(service(), rx, move |event| {
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
            run_worker(service(), rx, move |event| {
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
            run_worker(service(), rx, |_event| {}).await;
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
        let app_service = AppService::new(provider.clone());

        // Ошибка до команды: мутация не должна примениться.
        state.write().await.error_mode = MockErrorMode::BackendDown;

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(app_service, rx, move |event| {
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
            run_worker(service(), rx, move |event| {
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
    /// (`PerformanceProvider + GpuProvider`); `PerformanceProvider` нужен только
    /// из-за общего bound.
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

    // -----------------------------------------------------------------------
    // GPU worker-тесты
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn executes_gpu_applied_command() {
        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(service(), rx, move |event| {
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
        let app_service = AppService::new(provider.clone());

        // Начальное applied GPU state через публичный API до команды.
        let initial = app_service.gpu_state().await.expect("initial gpu state");

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(app_service, rx, move |event| {
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
        let app_service = AppService::new(provider.clone());

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
            run_worker(app_service, rx, move |event| {
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
        let app_service = AppService::new(provider.clone());

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
            run_worker(app_service, rx, move |event| {
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
        let app_service = AppService::new(provider.clone());

        let (tx, rx) = command_channel();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let worker = tokio::spawn(async move {
            run_worker(app_service, rx, move |event| {
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
}
