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
    BATTERY_POLKIT_ACTION, DBUS_NAME, DBUS_OBJECT_PATH, FAN_POLKIT_ACTION, GPU_POLKIT_ACTION,
    HardwareService, PolkitAuthorizer,
    battery::{
        AsusdBatteryMutationBackend, BatteryMutationBackend, BatteryMutationReadback,
        ZbusAsusdBatteryClient, discover_effective_reader,
    },
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

/// Why Battery mutation was disabled during startup discovery.
///
/// Keep this classification separate so a temporary discovery failure never
/// masquerades as permanent hardware/capability Unsupported.
enum DisabledBatteryReason {
    Unsupported(String),
    Unavailable(String),
    PermissionDenied(String),
}

/// Capability-local fallback used when no usable effective battery threshold
/// source exists. Keeping a typed disabled backend makes `SetChargeLimit` fail
/// locally without making the entire Hardware1 service unavailable.
struct DisabledBatteryMutationBackend {
    reason: DisabledBatteryReason,
}

impl DisabledBatteryMutationBackend {
    fn from_discovery_error(error: ProviderError) -> Self {
        let reason = match error {
            ProviderError::Unsupported(message) => DisabledBatteryReason::Unsupported(message),
            ProviderError::PermissionDenied(message) => {
                DisabledBatteryReason::PermissionDenied(message)
            }
            ProviderError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                DisabledBatteryReason::PermissionDenied(format!(
                    "battery effective threshold discovery permission denied: {error}"
                ))
            }
            other => DisabledBatteryReason::Unavailable(format!(
                "battery effective threshold discovery failed: {other}"
            )),
        };
        Self { reason }
    }
}

#[async_trait]
impl BatteryMutationBackend for DisabledBatteryMutationBackend {
    async fn set_charge_limit(
        &self,
        _percent: u8,
    ) -> Result<BatteryMutationReadback, ProviderError> {
        match &self.reason {
            DisabledBatteryReason::Unsupported(message) => {
                Err(ProviderError::Unsupported(message.clone()))
            }
            DisabledBatteryReason::Unavailable(message) => {
                Err(ProviderError::BackendUnavailable(message.clone()))
            }
            DisabledBatteryReason::PermissionDenied(message) => {
                Err(ProviderError::PermissionDenied(message.clone()))
            }
        }
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

    // Battery support is capability-local. Failure to discover its effective
    // kernel read-back must not prevent Performance/Fan Hardware1 startup.
    let battery_backend: Box<dyn BatteryMutationBackend> = match discover_effective_reader() {
        Ok(effective_reader) => Box::new(AsusdBatteryMutationBackend::new(
            ZbusAsusdBatteryClient::new(connection.clone()),
            effective_reader,
        )),
        Err(error) => {
            tracing::warn!(
                ?error,
                "battery effective threshold unavailable; Battery mutation will remain capability-local"
            );
            Box::new(DisabledBatteryMutationBackend::from_discovery_error(error))
        }
    };

    let fan_backend =
        AsusdFanCurveMutationBackend::new(ZbusAsusdFanCurveClient::new(connection.clone()));

    let service = HardwareService::with_battery_gpu_and_fan_backends(
        Box::new(PolkitAuthorizer::new(connection.clone())),
        battery_backend,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_battery_preserves_unsupported_discovery() {
        let backend = DisabledBatteryMutationBackend::from_discovery_error(
            ProviderError::Unsupported("no effective threshold source".into()),
        );
        let error = backend
            .set_charge_limit(80)
            .await
            .expect_err("unsupported must remain unsupported");
        assert!(matches!(error, ProviderError::Unsupported(_)));
    }

    #[tokio::test]
    async fn disabled_battery_preserves_permission_failure() {
        let backend = DisabledBatteryMutationBackend::from_discovery_error(ProviderError::Io(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        ));
        let error = backend
            .set_charge_limit(80)
            .await
            .expect_err("permission failure must remain distinct");
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn disabled_battery_maps_other_discovery_failures_to_unavailable() {
        let backend = DisabledBatteryMutationBackend::from_discovery_error(ProviderError::Io(
            std::io::Error::other("temporary read failure"),
        ));
        let error = backend
            .set_charge_limit(80)
            .await
            .expect_err("temporary failure must remain unavailable");
        assert!(matches!(error, ProviderError::BackendUnavailable(_)));
    }
}
