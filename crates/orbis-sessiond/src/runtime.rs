//! Production lifecycle: запускает discovered bootstrap и удерживает D-Bus
//! service живым до завершения процесса.

use crate::bootstrap::BootstrapError;

/// Ошибка lifecycle: сохраняет bootstrap failure и signal failure раздельно.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Ошибка bootstrap (D-Bus startup или battery discovery).
    #[error("bootstrap: {0}")]
    Bootstrap(#[from] BootstrapError),
    /// Ошибка ожидания shutdown signal.
    #[error("signal: {0}")]
    Signal(#[from] std::io::Error),
}

/// Запустить discovered sessiond и удерживать service живым до shutdown signal.
///
/// - вызывается существующий `connect_discovered_upower_session_server`
///   ровно один раз;
/// - возвращённая session Connection остаётся в scope helper и не
///   освобождается до завершения ожидания (без `mem::forget`, spawn,
///   detached tasks, retry/reconnect);
/// - ожидание первого из shutdown signals: SIGINT или SIGTERM
///   (`tokio::signal::unix` + `tokio::select!`), корректно для systemd --user;
/// - после первого signal функция завершается `Ok(())`;
/// - reconnect/restart policy принадлежит внешнему supervisor/systemd.
pub async fn run_discovered_sessiond() -> Result<(), RuntimeError> {
    let _session_connection = crate::bootstrap::connect_discovered_upower_session_server().await?;

    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }

    Ok(())
}
