//! Bootstrap helper для read-only session D-Bus server.

use std::sync::Arc;

use orbis_providers::traits::BatteryProvider;
use orbis_session_protocol::{BUS_NAME, OBJECT_PATH};

use crate::service::SessionService;

/// Построить session D-Bus server поверх подготовленного `Builder`.
///
/// - caller предоставляет transport-configured Builder (session/system/P2P),
///   helper не выбирает transport и не открывает bus самостоятельно;
/// - helper создаёт `SessionService` из переданного provider (provider не
///   читается до первого D-Bus property call);
/// - helper регистрирует protocol bus name (`BUS_NAME`) и object path
///   (`OBJECT_PATH`);
/// - ошибки `Builder::name`/`serve_at`/`build` передаются вызывающему коду как
///   `zbus::Error` (без retry/fallback/panic);
/// - возвращённую Connection необходимо удерживать живой для обслуживания
///   запросов.
pub async fn build_session_server(
    builder: zbus::connection::Builder<'_>,
    battery: Arc<dyn BatteryProvider>,
) -> zbus::Result<zbus::Connection> {
    let service = SessionService::new(battery);
    let builder = builder.name(BUS_NAME)?;
    let builder = builder.serve_at(OBJECT_PATH, service)?;
    builder.build().await
}
