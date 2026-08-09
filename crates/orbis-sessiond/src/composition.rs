//! Composition helper полного read-only пути: готовая UPower Connection →
//! source → provider → session server.

use std::sync::Arc;

use orbis_providers::traits::{BatteryProvider, PerformanceProvider};

use crate::hardwared::HardwarePerformanceClient;
use crate::server::{GpuCapabilities, build_session_server};
use crate::upower::{UPowerChargeLimitProvider, ZbusUPowerChargeLimitSource};

/// Построить session server поверх готовой UPower Connection.
///
/// - caller передаёт готовую UPower `Connection`, battery object path и
///   transport-configured session `Builder`;
/// - helper соединяет существующие production source/provider/server без
///   дублирования; не открывает system/session bus и не выбирает transport;
/// - UPower не читается во время construction — первое чтение происходит при
///   будущем session property Get;
/// - `gpu` — дополнительные read-only GPU capabilities (могут быть пустыми);
/// - `performance` — опциональный read-only Performance Mode provider;
/// - `hardware` — опциональный system-bus hardwared Performance client;
/// - возвращённую session Connection необходимо удерживать живой; переданная
///   UPower Connection удерживается provider внутри service graph.
pub async fn build_upower_session_server(
    session_builder: zbus::connection::Builder<'_>,
    upower_connection: zbus::Connection,
    battery_object_path: zbus::zvariant::OwnedObjectPath,
    gpu: GpuCapabilities,
    performance: Option<Arc<dyn PerformanceProvider>>,
    hardware: Option<Arc<dyn HardwarePerformanceClient>>,
) -> zbus::Result<zbus::Connection> {
    let source = ZbusUPowerChargeLimitSource::new(upower_connection, battery_object_path);
    let provider = UPowerChargeLimitProvider::new(source);
    let battery: Arc<dyn BatteryProvider> = Arc::new(provider);
    build_session_server(session_builder, battery, gpu, performance, hardware).await
}
