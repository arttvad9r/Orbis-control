//! # orbis-hardwared — production system-bus service.
//!
//! Подключается к system bus, создаёт production [`HardwareService`] (polkit
//! authorizer + writer с фиксированными kernel paths), регистрирует
//! `/io/github/orbiscontrol/Hardware`, занимает
//! `io.github.orbiscontrol.Hardware` (readiness для `Type=dbus`) и ждёт
//! SIGINT/SIGTERM.
//!
//! - при startup НИКАКИХ sysfs writes;
//! - единственная capability: `SetPerformanceProfile`;
//! - reconnect/restart policy принадлежит внешнему supervisor/systemd.

use std::error::Error;

use orbis_hardwared::{
    Authorizer, BATTERY_POLKIT_ACTION, DBUS_NAME, DBUS_OBJECT_PATH, GPU_POLKIT_ACTION,
    HardwareService, PolkitAuthorizer,
    battery::{AsusdBatteryMutationBackend, ZbusAsusdBatteryClient, discover_effective_reader},
    supergfxd::{SupergfxdMutationBackend, ZbusSupergfxdMutationClient},
};

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,orbis_hardwared=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    let connection = zbus::connection::Builder::system()?.build().await?;

    let authorizer: Box<dyn Authorizer> = Box::new(PolkitAuthorizer::new(connection.clone()));
    let battery_client = ZbusAsusdBatteryClient::new(connection.clone());
    let effective_reader = discover_effective_reader()?;
    let battery_backend = AsusdBatteryMutationBackend::new(battery_client, effective_reader);
    let battery_authorizer: Box<dyn Authorizer> = Box::new(PolkitAuthorizer::with_action(
        connection.clone(),
        BATTERY_POLKIT_ACTION,
    ));
    let gpu_backend =
        SupergfxdMutationBackend::new(ZbusSupergfxdMutationClient::new(connection.clone()));
    let gpu_authorizer: Box<dyn Authorizer> = Box::new(PolkitAuthorizer::with_action(
        connection.clone(),
        GPU_POLKIT_ACTION,
    ));
    let service = HardwareService::with_battery_and_gpu_backends(
        authorizer,
        Box::new(battery_backend),
        battery_authorizer,
        Box::new(gpu_backend),
        gpu_authorizer,
    );
    connection
        .object_server()
        .at(DBUS_OBJECT_PATH, service)
        .await?;

    // Readiness для Type=dbus наступает после получения BusName. Если имя уже
    // занято другим peer-ом — zbus::Error::NameTaken, startup завершится.
    connection.request_name(DBUS_NAME).await?;

    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }

    Ok(())
}
