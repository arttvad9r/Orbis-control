//! Bootstrap helper для read-only session D-Bus server.

use std::sync::Arc;

use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};
use orbis_session_protocol::{BUS_NAME, OBJECT_PATH};

use crate::hardwared::HardwarePerformanceClient;
use crate::service::SessionService;

/// Дополнительные read-only GPU capabilities для session server.
#[derive(Default)]
pub struct GpuCapabilities {
    /// Read-only dGPU runtime power provider.
    pub power: Option<Arc<dyn GpuPowerProvider>>,
    /// Read-only physical MUX provider.
    pub mux: Option<Arc<dyn GpuMuxProvider>>,
    /// Read-only dGPU access policy provider.
    pub access: Option<Arc<dyn GpuAccessProvider>>,
}

/// Построить session D-Bus server поверх подготовленного `Builder`.
///
/// - caller предоставляет transport-configured Builder (session/system/P2P),
///   helper не выбирает transport и не открывает bus самостоятельно;
/// - helper создаёт `SessionService` из переданных providers (providers не
///   читаются до первого D-Bus property call);
/// - helper регистрирует protocol bus name (`BUS_NAME`) и object path
///   (`OBJECT_PATH`);
/// - ошибки `Builder::name`/`serve_at`/`build` передаются вызывающему коду как
///   `zbus::Error` (без retry/fallback/panic);
/// - возвращённую Connection необходимо удерживать живой для обслуживания
///   запросов.
pub async fn build_session_server(
    builder: zbus::connection::Builder<'_>,
    battery: Arc<dyn BatteryProvider>,
    gpu: GpuCapabilities,
    performance: Option<Arc<dyn PerformanceProvider>>,
    hardware: Option<Arc<dyn HardwarePerformanceClient>>,
) -> zbus::Result<zbus::Connection> {
    let mut service = SessionService::new(battery);
    if let Some(p) = gpu.power {
        service = service.with_gpu_power(p);
    }
    if let Some(m) = gpu.mux {
        service = service.with_gpu_mux(m);
    }
    if let Some(a) = gpu.access {
        service = service.with_gpu_access(a);
    }
    if let Some(p) = performance {
        service = service.with_performance(p);
    }
    if let Some(h) = hardware {
        service = service.with_hardware(h);
    }
    let builder = builder.name(BUS_NAME)?;
    let builder = builder.serve_at(OBJECT_PATH, service)?;
    builder.build().await
}
