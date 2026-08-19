//! # orbis-hardwared — production system-bus service.
//!
//! Подключается к system bus, создаёт production [`HardwareService`] (polkit
//! authorizer + writer с фиксированными kernel paths), регистрирует
//! `/io/github/orbiscontrol/Hardware`, занимает
//! `io.github.orbiscontrol.Hardware` (readiness для `Type=dbus`) и ждёт
//! SIGINT/SIGTERM.
//!
//! - при startup НИКАКИХ hardware writes;
//! - Battery mutation включается только при наличии effective threshold reader;
//! - raw GPU SetMode намеренно отключён до доказанной product-level semantics;
//! - fan mutation намеренно hard-disabled до исправления известных write/reset contracts;
//! - reconnect/restart policy принадлежит внешнему supervisor/systemd.

use std::error::Error;

use async_trait::async_trait;
use orbis_core::{fan::FanId, profile::AsusdFanProfile};
use orbis_hardwared::{
    AURA_POLKIT_ACTION, BATTERY_POLKIT_ACTION, DBUS_NAME, DBUS_OBJECT_PATH, FAN_POLKIT_ACTION,
    GPU_POLKIT_ACTION, HardwareService, KEYBOARD_BACKLIGHT_POLKIT_ACTION, PANEL_POLKIT_ACTION,
    PolkitAuthorizer,
    aura::{AsusdAuraStaticRgbMutationBackend, ZbusAsusdAuraClient},
    battery::{
        AsusdBatteryMutationBackend, BatteryMutationBackend, BatteryMutationReadback,
        BatteryMutationStatus, ZbusAsusdBatteryClient, discover_effective_reader,
    },
    fans::{
        FanCurveDefaultsReadback, FanCurveMutationOperation, FanCurveMutationReadback,
        FanCurvePoints, FanMutationStatus,
    },
    panel::{
        AsusdPanelOverdriveMutationBackend, PanelOverdriveMutationBackend,
        PanelOverdriveMutationReadback, PanelOverdriveMutationStatus,
        ZbusAsusdPanelOverdriveClient, discover_panel_overdrive_reader,
    },
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

/// Production hard stop for fan mutations.
///
/// The asusd compatibility setter currently cannot safely preserve the stored
/// `CurveData.enabled` field, and upstream factory-default reset can fail before
/// restoring the previous performance profile. Polkit is also default-deny,
/// but a local policy override must not be enough to reach those unsafe writes.
struct DisabledFanMutationBackend;

const FAN_MUTATION_DISABLED: &str =
    "fan mutation is disabled until enabled-state preservation and factory-reset restoration are fixed";

#[async_trait]
impl FanCurveMutationOperation for DisabledFanMutationBackend {
    async fn set_fan_curve(
        &self,
        _profile: AsusdFanProfile,
        _fan: &FanId,
        _curve: &FanCurvePoints,
    ) -> Result<FanCurveMutationReadback, ProviderError> {
        Err(ProviderError::Unsupported(FAN_MUTATION_DISABLED.into()))
    }

    async fn mutation_status(&self) -> FanMutationStatus {
        FanMutationStatus::Unsupported
    }

    async fn reset_curves_to_defaults(
        &self,
        _profile: AsusdFanProfile,
    ) -> Result<FanCurveDefaultsReadback, ProviderError> {
        Err(ProviderError::Unsupported(FAN_MUTATION_DISABLED.into()))
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

    fn mutation_status(&self) -> BatteryMutationStatus {
        match &self.reason {
            DisabledBatteryReason::Unsupported(_) => BatteryMutationStatus::Unsupported,
            DisabledBatteryReason::Unavailable(_) => BatteryMutationStatus::TemporarilyUnavailable,
            DisabledBatteryReason::PermissionDenied(_) => BatteryMutationStatus::PermissionDenied,
        }
    }
}

/// Why Panel Overdrive mutation was disabled during startup discovery.
///
/// Keep this classification separate so a temporary discovery failure never
/// masquerades as permanent hardware/capability Unsupported.
enum DisabledPanelOverdriveReason {
    Unsupported(String),
    Unavailable(String),
    PermissionDenied(String),
}

/// Capability-local fallback used when no readable authoritative
/// `panel_overdrive` attribute exists. Keeping a typed disabled backend makes
/// `SetPanelOverdrive` fail locally without making the entire Hardware1
/// service unavailable.
struct DisabledPanelOverdriveMutationBackend {
    reason: DisabledPanelOverdriveReason,
}

impl DisabledPanelOverdriveMutationBackend {
    fn from_discovery_error(error: ProviderError) -> Self {
        let reason = match error {
            ProviderError::Unsupported(message) => {
                DisabledPanelOverdriveReason::Unsupported(message)
            }
            ProviderError::PermissionDenied(message) => {
                DisabledPanelOverdriveReason::PermissionDenied(message)
            }
            ProviderError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                DisabledPanelOverdriveReason::PermissionDenied(format!(
                    "panel_overdrive discovery permission denied: {error}"
                ))
            }
            other => DisabledPanelOverdriveReason::Unavailable(format!(
                "panel_overdrive discovery failed: {other}"
            )),
        };
        Self { reason }
    }
}

#[async_trait]
impl PanelOverdriveMutationBackend for DisabledPanelOverdriveMutationBackend {
    async fn set_panel_overdrive(
        &self,
        _enabled: bool,
    ) -> Result<PanelOverdriveMutationReadback, ProviderError> {
        match &self.reason {
            DisabledPanelOverdriveReason::Unsupported(message) => {
                Err(ProviderError::Unsupported(message.clone()))
            }
            DisabledPanelOverdriveReason::Unavailable(message) => {
                Err(ProviderError::BackendUnavailable(message.clone()))
            }
            DisabledPanelOverdriveReason::PermissionDenied(message) => {
                Err(ProviderError::PermissionDenied(message.clone()))
            }
        }
    }

    fn mutation_status(&self) -> PanelOverdriveMutationStatus {
        match &self.reason {
            DisabledPanelOverdriveReason::Unsupported(_) => {
                PanelOverdriveMutationStatus::Unsupported
            }
            DisabledPanelOverdriveReason::Unavailable(_) => {
                PanelOverdriveMutationStatus::TemporarilyUnavailable
            }
            DisabledPanelOverdriveReason::PermissionDenied(_) => {
                PanelOverdriveMutationStatus::PermissionDenied
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
    // kernel read-back must not prevent unrelated Hardware1 capabilities.
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

    // Known fan write/reset correctness bugs are stronger than a policy-level
    // warning: production Hardware1 never constructs the live asusd fan writer.
    let fan_backend = DisabledFanMutationBackend;

    // Panel Overdrive support is capability-local. Failure to discover its
    // authoritative kernel read-back must not prevent unrelated Hardware1
    // capabilities. Product policy remains default-deny pending owner evidence.
    let panel_backend: Box<dyn PanelOverdriveMutationBackend> =
        match discover_panel_overdrive_reader() {
            Ok(reader) => Box::new(AsusdPanelOverdriveMutationBackend::new(
                ZbusAsusdPanelOverdriveClient::new(connection.clone()),
                reader,
            )),
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "panel_overdrive attribute unavailable; Panel Overdrive mutation will remain capability-local"
                );
                Box::new(DisabledPanelOverdriveMutationBackend::from_discovery_error(
                    error,
                ))
            }
        };

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
    )
    .with_panel(
        panel_backend,
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            PANEL_POLKIT_ACTION,
        )),
    )
    .with_keyboard_backlight(
        Box::new(
            orbis_hardwared::keyboard_backlight::SysfsKeyboardBacklightMutationBackend::default(),
        ),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            KEYBOARD_BACKLIGHT_POLKIT_ACTION,
        )),
    )
    .with_aura_static_rgb(
        Box::new(AsusdAuraStaticRgbMutationBackend::new(
            ZbusAsusdAuraClient::new(connection.clone()),
        )),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            AURA_POLKIT_ACTION,
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
    async fn disabled_fan_is_hard_stopped_in_production_composition() {
        let backend = DisabledFanMutationBackend;
        assert_eq!(backend.mutation_status().await, FanMutationStatus::Unsupported);
        let error = backend
            .reset_curves_to_defaults(AsusdFanProfile::Balanced)
            .await
            .expect_err("disabled fan reset must never reach asusd");
        assert!(matches!(error, ProviderError::Unsupported(_)));
    }

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

    #[test]
    fn disabled_battery_mutation_status_preserves_typed_reason() {
        let unsupported = DisabledBatteryMutationBackend::from_discovery_error(
            ProviderError::Unsupported("no effective threshold source".into()),
        );
        assert_eq!(
            unsupported.mutation_status(),
            BatteryMutationStatus::Unsupported
        );

        let denied = DisabledBatteryMutationBackend::from_discovery_error(ProviderError::Io(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        ));
        assert_eq!(
            denied.mutation_status(),
            BatteryMutationStatus::PermissionDenied
        );

        let unavailable = DisabledBatteryMutationBackend::from_discovery_error(ProviderError::Io(
            std::io::Error::other("temporary read failure"),
        ));
        assert_eq!(
            unavailable.mutation_status(),
            BatteryMutationStatus::TemporarilyUnavailable
        );
    }

    #[test]
    fn battery_mutation_wire_roundtrip_is_total() {
        use orbis_hardwared::battery::battery_mutation_wire;
        for status in [
            BatteryMutationStatus::Supported,
            BatteryMutationStatus::Unsupported,
            BatteryMutationStatus::TemporarilyUnavailable,
            BatteryMutationStatus::PermissionDenied,
            BatteryMutationStatus::Unknown,
        ] {
            let wire = battery_mutation_wire::to_wire(status);
            assert_eq!(battery_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(battery_mutation_wire::from_wire(99), None);
    }

    #[tokio::test]
    async fn disabled_panel_preserves_unsupported_discovery() {
        let backend = DisabledPanelOverdriveMutationBackend::from_discovery_error(
            ProviderError::Unsupported("no panel_overdrive ABI".into()),
        );
        let error = backend
            .set_panel_overdrive(true)
            .await
            .expect_err("unsupported must remain unsupported");
        assert!(matches!(error, ProviderError::Unsupported(_)));
    }

    #[tokio::test]
    async fn disabled_panel_preserves_permission_failure() {
        let backend =
            DisabledPanelOverdriveMutationBackend::from_discovery_error(ProviderError::Io(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            ));
        let error = backend
            .set_panel_overdrive(true)
            .await
            .expect_err("permission failure must remain distinct");
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn disabled_panel_maps_other_discovery_failures_to_unavailable() {
        let backend = DisabledPanelOverdriveMutationBackend::from_discovery_error(
            ProviderError::Io(std::io::Error::other("temporary read failure")),
        );
        let error = backend
            .set_panel_overdrive(true)
            .await
            .expect_err("temporary failure must remain unavailable");
        assert!(matches!(error, ProviderError::BackendUnavailable(_)));
    }

    #[test]
    fn disabled_panel_mutation_status_preserves_typed_reason() {
        let unsupported = DisabledPanelOverdriveMutationBackend::from_discovery_error(
            ProviderError::Unsupported("no panel_overdrive ABI".into()),
        );
        assert_eq!(
            unsupported.mutation_status(),
            PanelOverdriveMutationStatus::Unsupported
        );

        let denied =
            DisabledPanelOverdriveMutationBackend::from_discovery_error(ProviderError::Io(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            ));
        assert_eq!(
            denied.mutation_status(),
            PanelOverdriveMutationStatus::PermissionDenied
        );

        let unavailable = DisabledPanelOverdriveMutationBackend::from_discovery_error(
            ProviderError::Io(std::io::Error::other("temporary read failure")),
        );
        assert_eq!(
            unavailable.mutation_status(),
            PanelOverdriveMutationStatus::TemporarilyUnavailable
        );
    }

    #[test]
    fn panel_mutation_wire_roundtrip_is_total() {
        use orbis_hardwared::panel::panel_mutation_wire;
        for status in [
            PanelOverdriveMutationStatus::Supported,
            PanelOverdriveMutationStatus::Unsupported,
            PanelOverdriveMutationStatus::TemporarilyUnavailable,
            PanelOverdriveMutationStatus::PermissionDenied,
            PanelOverdriveMutationStatus::Unknown,
        ] {
            let wire = panel_mutation_wire::to_wire(status);
            assert_eq!(panel_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(panel_mutation_wire::from_wire(99), None);
    }
}
