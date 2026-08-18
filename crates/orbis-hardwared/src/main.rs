//! # orbis-hardwared — production system-bus service.
//!
//! Подключается к system bus, создаёт production [`HardwareService`] (polkit
//! authorizer + writer с фиксированными kernel paths), регистрирует
//! `/io/github/orbiscontrol/Hardware`, занимает
//! `io.github.orbiscontrol.Hardware` (readiness для `Type=dbus`) и ждёт
//! SIGINT/SIGTERM.
//!
//! - при startup НИКАКИХ hardware writes;
//! - Performance и fan backends независимы от Battery discovery;
//! - Battery mutation включается только при наличии effective threshold reader;
//! - raw GPU SetMode намеренно отключён до доказанной product-level semantics;
//! - reconnect/restart policy принадлежит внешнему supervisor/systemd.

use std::error::Error;

use async_trait::async_trait;
use orbis_hardwared::{
    Authorizer, BATTERY_POLKIT_ACTION, DBUS_NAME, DBUS_OBJECT_PATH, FAN_POLKIT_ACTION,
    GPU_POLKIT_ACTION, HardwareService, PolkitAuthorizer,
    battery::{AsusdBatteryMutationBackend, ZbusAsusdBatteryClient, discover_effective_reader},
    fans::{AsusdFanCurveMutationBackend, ZbusAsusdFanCurveClient},
    supergfxd::{MutationObservation, SupergfxdMutationOperation},
};
use orbis_providers::{error::ProviderError, supergfxd::SupergfxdMode};

/// Production guardrail: Hardware1 keeps its stable method surface, but raw
/// supergfxd SetMode is not a proven product-level Orbis mutation contract yet.
/// Authorized callers therefore receive NotSupported without touching hardware.
struct DisabledGpuMutationBackend;

#[async_trait]
impl SupergfxdMutationOperation for DisabledGpuMutationBackend {
    async fn request_mode(
        &self,
        _requested: SupergfxdMode,
    ) -> Result<MutationObservation, ProviderError> {
        Err(ProviderError::Unsupported(
            "GPU mutation is disabled until Orbis product-level GPU semantics are proven".into(),
        ))
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,orbis_hardwared=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    let connection = zbus::connection::Builder::system()?.build().await?;

    let fan_backend =
        AsusdFanCurveMutationBackend::new(ZbusAsusdFanCurveClient::new(connection.clone()));

    // Battery support is capability-local. Failure to discover its effective
    // kernel read-back must not prevent Performance/Fan Hardware1 startup.
    let service = match discover_effective_reader() {
        Ok(effective_reader) => {
            let battery_backend = AsusdBatteryMutationBackend::new(
                ZbusAsusdBatteryClient::new(connection.clone()),
                effective_reader,
            );

            HardwareService::with_battery_gpu_and_fan_backends(
                Box::new(PolkitAuthorizer::new(connection.clone())),
                Box::new(battery_backend),
                Box::new(PolkitAuthorizer::with_action(
                    connection.clone(),
                    BATTERY_POLKIT_ACTION,
                )),
                Box::new(DisabledGpuMutationBackend),
                Box::new(PolkitAuthorizer::with_action(
                    connection.clone(),
                    GPU_POLKIT_ACTION,
                )),
                Box::new(fan_backend),
                Box::new(PolkitAuthorizer::with_action(
                    connection.clone(),
                    FAN_POLKIT_ACTION,
                )),
            )
        }
        Err(error) => {
            tracing::warn!(
                ?error,
                "battery effective threshold unavailable; starting Hardware1 without Battery mutation"
            );

            HardwareService::with_fan_backend(
                Box::new(PolkitAuthorizer::new(connection.clone())),
                Box::new(fan_backend),
                Box::new(PolkitAuthorizer::with_action(
                    connection.clone(),
                    FAN_POLKIT_ACTION,
                )),
            )
        }
    };

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
