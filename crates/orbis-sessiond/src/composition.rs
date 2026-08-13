//! Composition helper полного read-only пути: готовая UPower Connection →
//! source → provider → session server.

use std::sync::Arc;

use orbis_providers::traits::{BatteryProvider, PerformanceProvider};

use crate::server::{GpuCapabilities, build_session_server};
use crate::upower::{
    AsusdBatteryChargeLimitProvider, SysfsBatteryEndThresholdSource, ZbusAsusdConfiguredSource,
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
    let effective_source = SysfsBatteryEndThresholdSource::from_native_path(&battery_native_path)
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    let asusd_source = ZbusAsusdConfiguredSource::new(upower_connection.clone());
    build_upower_session_server_with_effective_source(
        session_builder,
        upower_connection,
        battery_object_path,
        asusd_source,
        effective_source,
        gpu,
        performance,
    )
    .await
}

/// Вариант composition helper с injected effective-threshold source.
///
/// Production bootstrap использует [`build_upower_session_server`] и реальный
/// sysfs source. Injection нужен для hermetic P2P/integration tests, где
/// `/sys/class/power_supply` недоступен и не должен быть mock-ирован через
/// реальную файловую систему.
pub async fn build_upower_session_server_with_effective_source<E>(
    session_builder: zbus::connection::Builder<'_>,
    upower_connection: zbus::Connection,
    battery_object_path: zbus::zvariant::OwnedObjectPath,
    asusd_source: impl crate::upower::AsusdConfiguredSource + 'static,
    effective_source: E,
    gpu: GpuCapabilities,
    performance: Option<Arc<dyn PerformanceProvider>>,
) -> zbus::Result<zbus::Connection>
where
    E: crate::upower::BatteryEffectiveSource + 'static,
{
    let upower_source = ZbusUPowerChargeLimitSource::new(upower_connection, battery_object_path);
    let provider =
        AsusdBatteryChargeLimitProvider::new(upower_source, asusd_source, effective_source);
    let battery: Arc<dyn BatteryProvider> = Arc::new(provider);
    build_session_server(session_builder, battery, gpu, performance).await
}
