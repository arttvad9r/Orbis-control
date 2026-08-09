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

use orbis_hardwared::{Authorizer, DBUS_NAME, DBUS_OBJECT_PATH, HardwareService, PolkitAuthorizer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let connection = zbus::connection::Builder::system()?.build().await?;

    let authorizer: Box<dyn Authorizer> = Box::new(PolkitAuthorizer::new(connection.clone()));
    let service = HardwareService::new(authorizer);
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
