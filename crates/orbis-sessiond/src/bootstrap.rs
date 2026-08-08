//! Real D-Bus bootstrap: открывает system/session connections и передаёт их
//! в существующий composition layer.

use orbis_providers::error::ProviderError;

use crate::composition::build_upower_session_server;

/// Ошибка bootstrap: сохраняет класс ошибки отдельно для D-Bus startup и
/// для battery discovery/provider.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    /// D-Bus/bootstrap error (system/session connection, builder, server).
    #[error("dbus bootstrap: {0}")]
    Dbus(#[from] zbus::Error),
    /// Battery discovery/provider error.
    #[error("battery discovery: {0}")]
    Discovery(#[from] ProviderError),
}

/// Открыть реальные D-Bus connections и собрать read-only session server.
///
/// - открывается system bus для UPower;
/// - открывается session bus для Orbis service (transport-configured
///   `Builder::session()`);
/// - `battery_object_path` задаёт caller;
/// - UPower properties не читаются до первого session property Get;
/// - возвращённую session Connection необходимо удерживать живой: внутри
///   service graph остаётся UPower Connection;
/// - helper не управляет lifecycle, reconnect и signal handling.
pub async fn connect_upower_session_server(
    battery_object_path: zbus::zvariant::OwnedObjectPath,
) -> zbus::Result<zbus::Connection> {
    let upower_connection = zbus::Connection::system().await?;
    let session_builder = zbus::connection::Builder::session()?;
    build_upower_session_server(session_builder, upower_connection, battery_object_path).await
}

/// Открыть connections, обнаружить системную батарею и собрать session server.
///
/// - открывается одна system bus Connection для UPower;
/// - через ту же Connection выполняется read-only discovery батареи
///   (`discover_battery_object_path`), ровно один раз при startup;
/// - найденный path и та же UPower Connection передаются в
///   существующий composition layer;
/// - открывается session bus для Orbis service;
/// - D-Bus startup failure и discovery failure сохраняются раздельно
///   (`BootstrapError::Dbus` / `BootstrapError::Discovery`);
/// - возвращённую session Connection необходимо удерживать живой; UPower
///   Connection переиспользуется provider'ом внутри service graph;
/// - helper не управляет lifecycle, reconnect и signal handling.
pub async fn connect_discovered_upower_session_server() -> Result<zbus::Connection, BootstrapError>
{
    let upower_connection = zbus::Connection::system().await?;
    let battery_object_path =
        crate::discovery::discover_battery_object_path(&upower_connection).await?;
    let session_builder = zbus::connection::Builder::session()?;
    Ok(
        build_upower_session_server(session_builder, upower_connection, battery_object_path)
            .await?,
    )
}
