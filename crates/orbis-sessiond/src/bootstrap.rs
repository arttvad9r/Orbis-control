//! Real D-Bus bootstrap: открывает system/session connections и передаёт их
//! в существующий composition layer.

use std::sync::Arc;

use orbis_providers::error::ProviderError;

use crate::armoury::{ArmouryGpuProvider, SysfsArmouryGpuSource};
use crate::composition::build_upower_session_server;
use crate::fans::{AsusdFanCurveSource, ZbusAsusdFanCurveSource};
use crate::performance::{KernelPerformanceProvider, SysfsKernelPlatformProfileSource};
use crate::server::GpuCapabilities;
use crate::supergfxd::{SupergfxdGpuPowerProvider, ZbusSupergfxdGpuPowerSource};

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
    battery_native_path: String,
) -> zbus::Result<zbus::Connection> {
    let upower_connection = zbus::Connection::system().await?;
    let session_builder = zbus::connection::Builder::session()?;
    build_upower_session_server(
        session_builder,
        upower_connection,
        battery_object_path,
        battery_native_path,
        GpuCapabilities::default(),
        None,
        None,
    )
    .await
}

/// Открыть connections и собрать session server без обязательного startup
/// discovery батареи (UPower resilience).
///
/// - открывается одна system bus Connection для UPower (и asusd),
///   discovery выполняется **лениво** при первом battery read;
/// - через ту же Connection выполняется read-only discovery батареи
///   (`discover_battery`) ровно при каждом battery read; если UPower/battery
///   недоступен при startup — Session1 всё равно стартует, Performance/GPU/
///   Fan read работают, Battery возвращает честную ошибку;
/// - если UPower/battery появляется позже или UPower перезапускается —
///   следующий Battery read повторит discovery без restart sessiond;
/// - transient failure не кэшируется; никаких synthetic/default charge limits;
/// - открывается session bus для Orbis service;
/// - D-Bus startup failure сохраняется отдельно (`BootstrapError::Dbus`);
///   discovery failure НЕ является фатальной для старта;
/// - возвращённую session Connection необходимо удерживать живой; system
///   Connection переиспользуется provider'ом внутри service graph;
/// - helper не управляет lifecycle, reconnect и signal handling.
pub async fn connect_discovered_upower_session_server() -> Result<zbus::Connection, BootstrapError>
{
    let upower_connection = zbus::Connection::system().await?;
    let session_builder = zbus::connection::Builder::session()?;

    // Read-only GPU capabilities:
    // - power → supergfxd (переиспользуем ту же system connection);
    // - mux/access → kernel ASUS Armoury firmware-attributes (sysfs).
    let gpu_power: Arc<dyn orbis_providers::traits::GpuPowerProvider> = Arc::new(
        SupergfxdGpuPowerProvider::new(ZbusSupergfxdGpuPowerSource::new(upower_connection.clone())),
    );
    let gpu_mux: Arc<dyn orbis_providers::traits::GpuMuxProvider> =
        Arc::new(ArmouryGpuProvider::new(SysfsArmouryGpuSource::default()));
    let gpu_access: Arc<dyn orbis_providers::traits::GpuAccessProvider> =
        Arc::new(ArmouryGpuProvider::new(SysfsArmouryGpuSource::default()));

    let gpu = GpuCapabilities {
        power: Some(gpu_power),
        mux: Some(gpu_mux),
        access: Some(gpu_access),
    };

    // Read-only Performance Mode provider: symbolic kernel platform_profile ABI.
    let performance: Arc<dyn orbis_providers::traits::PerformanceProvider> = Arc::new(
        KernelPerformanceProvider::new(SysfsKernelPlatformProfileSource::default()),
    );

    // Read-only asusd fan curve source (profile-specific curves): переиспользуем
    // ту же system connection. Fan curve reads идут через sessiond, НЕ из GUI.
    let fan_curves: Arc<dyn AsusdFanCurveSource> =
        Arc::new(ZbusAsusdFanCurveSource::new(upower_connection.clone()));

    Ok(crate::composition::build_lazy_upower_session_server(
        session_builder,
        upower_connection,
        gpu,
        Some(performance),
        Some(fan_curves),
    )
    .await?)
}
