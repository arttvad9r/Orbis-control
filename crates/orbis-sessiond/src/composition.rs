//! Composition helper полного read-only пути: готовая UPower Connection →
//! source → provider → session server.

use std::sync::Arc;

use orbis_providers::traits::{BatteryProvider, PerformanceProvider};

use crate::server::{GpuCapabilities, build_session_server};
use crate::upower::{
    CombinedChargeLimitSource, SysfsBatteryEndThresholdSource, UPowerChargeLimitProvider,
    ZbusUPowerChargeLimitSource,
};

/// Построить session server поверх готовой UPower Connection.
///
/// - caller передаёт готовую UPower `Connection`, battery object path и native
///   power-supply name;
///   transport-configured session `Builder`;
/// - helper соединяет существующие production source/provider/server без
///   дублирования; не открывает system/session bus и не выбирает transport;
/// - UPower не читается во время construction — первое чтение происходит при
///   будущем session property Get;
/// - `gpu` — дополнительные read-only GPU capabilities (могут быть пустыми);
/// - `performance` — опциональный read-only Performance Mode provider;
/// - возвращённую session Connection необходимо удерживать живой; переданная
///   UPower Connection удерживается provider внутри service graph.
pub async fn build_upower_session_server(
    session_builder: zbus::connection::Builder<'_>,
    upower_connection: zbus::Connection,
    battery_object_path: zbus::zvariant::OwnedObjectPath,
    battery_native_path: String,
    gpu: GpuCapabilities,
    performance: Option<Arc<dyn PerformanceProvider>>,
) -> zbus::Result<zbus::Connection> {
    let upower_source = ZbusUPowerChargeLimitSource::new(upower_connection, battery_object_path);
    let effective_source = SysfsBatteryEndThresholdSource::from_native_path(&battery_native_path)
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    let source = CombinedChargeLimitSource::new(upower_source, effective_source);
    let provider = UPowerChargeLimitProvider::new(source);
    let battery: Arc<dyn BatteryProvider> = Arc::new(provider);
    build_session_server(session_builder, battery, gpu, performance).await
}
