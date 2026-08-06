//! Real D-Bus bootstrap: открывает system/session connections и передаёт их
//! в существующий composition layer.

use crate::composition::build_upower_session_server;
use crate::upower::ChargeLimitBounds;

/// Открыть реальные D-Bus connections и собрать read-only session server.
///
/// - открывается system bus для UPower;
/// - открывается session bus для Orbis service (transport-configured
///   `Builder::session()`);
/// - `battery_object_path` и `bounds` задаёт caller;
/// - UPower properties не читаются до первого session property Get;
/// - возвращённую session Connection необходимо удерживать живой: внутри
///   service graph остаётся UPower Connection;
/// - helper не управляет lifecycle, reconnect и signal handling.
pub async fn connect_upower_session_server(
    battery_object_path: zbus::zvariant::OwnedObjectPath,
    bounds: ChargeLimitBounds,
) -> zbus::Result<zbus::Connection> {
    let upower_connection = zbus::Connection::system().await?;
    let session_builder = zbus::connection::Builder::session()?;
    build_upower_session_server(
        session_builder,
        upower_connection,
        battery_object_path,
        bounds,
    )
    .await
}
