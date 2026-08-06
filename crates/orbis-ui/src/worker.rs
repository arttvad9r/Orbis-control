//! # orbis-ui worker
//!
//! Независимый от Slint последовательный async-worker для команд Performance
//! Mode.
//!
//! - worker владеет `AppService<P>` и command receiver;
//! - команды обрабатываются строго по порядку (одна за другой);
//! - полный результат `AppService` передаётся внешнему event sink без
//!   преобразований (ни строк, ни UI-типов, ни banner-текстов);
//! - worker не знает о UI-типах и свойствах окна (подключение event sink к
//!   событийному циклу интерфейса — следующий микрошаг);
//! - в production-коде worker не создаёт runtime, не вызывает `spawn` и не
//!   порождает потоки;
//! - после закрытия всех command senders `recv()` возвращает `None` и worker
//!   завершается.

use orbis_application::{AppService, PerformanceCommandOutcome, SetPerformanceError};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::traits::PerformanceProvider;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Типизированная команда Performance Mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerCommand {
    /// Установить профиль производительности.
    SetPerformance(PerformanceProfile),
}

/// Событие результата команды.
///
/// Переносит полный `Result` из `orbis_application` без преобразований:
/// `CommandOutcome` (ApplyResult + authoritative PerformanceState) либо
/// `CommandError` (`Command` / `ReadBack` с сохранённым ApplyResult).
#[derive(Debug)]
pub enum WorkerEvent {
    /// Полный результат команды Performance.
    Performance(Result<PerformanceCommandOutcome, SetPerformanceError>),
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

/// Последовательный worker для Performance Mode.
///
/// - `service` — владеемый `AppService<P>`;
/// - `receiver` — команды в порядке получения;
/// - `emit` — event sink, вызывается ровно один раз на каждую команду.
///
/// Invariants: одновременно выполняется не более одной команды; следующая
/// команда начинается только после завершения предыдущей; события выдаются в
/// порядке команд; stale results внутри одного worker невозможны.
pub async fn run_worker<P, F>(
    service: AppService<P>,
    mut receiver: UnboundedReceiver<WorkerCommand>,
    mut emit: F,
) where
    P: PerformanceProvider + Send + Sync + 'static,
    F: FnMut(WorkerEvent) + Send + 'static,
{
    while let Some(command) = receiver.recv().await {
        let event = match command {
            WorkerCommand::SetPerformance(profile) => {
                WorkerEvent::Performance(service.set_performance(profile).await)
            }
        };
        emit(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use orbis_application::{AppService, CommandError};
    use orbis_core::profile::PerformanceProfile;
    use orbis_providers::error::ProviderError;
    use orbis_providers::mock::{MockErrorMode, MockProvider};
    use orbis_providers::traits::PerformanceProvider;
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
}
