//! Composition helper полного read-only пути: готовая UPower Connection →
//! source → provider → session server.

use std::sync::Arc;

use orbis_providers::traits::BatteryProvider;

use crate::server::build_session_server;
use crate::upower::{ChargeLimitBounds, UPowerChargeLimitProvider, ZbusUPowerChargeLimitSource};

/// Построить read-only session server поверх готовой UPower Connection.
///
/// - caller передаёт готовую UPower `Connection`, battery object path, bounds и
///   transport-configured session `Builder`;
/// - helper соединяет существующие production source/provider/server без
///   дублирования; не открывает system/session bus и не выбирает transport;
/// - UPower не читается во время construction — первое чтение происходит при
///   будущем session property Get;
/// - возвращённую session Connection необходимо удерживать живой; переданная
///   UPower Connection удерживается provider внутри service graph.
pub async fn build_upower_session_server(
    session_builder: zbus::connection::Builder<'_>,
    upower_connection: zbus::Connection,
    battery_object_path: zbus::zvariant::OwnedObjectPath,
    bounds: ChargeLimitBounds,
) -> zbus::Result<zbus::Connection> {
    let source = ZbusUPowerChargeLimitSource::new(upower_connection, battery_object_path);
    let provider = UPowerChargeLimitProvider::new(source, bounds);
    let battery: Arc<dyn BatteryProvider> = Arc::new(provider);
    build_session_server(session_builder, battery).await
}
