//! # orbis-hardwared — production system-bus service.
//!
//! Подключается к system bus, создаёт production [`HardwareService`],
//! регистрирует `/io/github/orbiscontrol/Hardware`, занимает
//! `io.github.orbiscontrol.Hardware` (readiness для `Type=dbus`) и ждёт
//! SIGINT/SIGTERM.
//!
//! Production mutation policy is intentionally narrower than the set of typed
//! backend implementations present in this crate:
//! - Performance is enabled through its validated Hardware1 path;
//! - Battery is enabled only after non-activating asusd-owner + effective-threshold preflight;
//! - GPU/Fan/Panel/Keyboard/Aura mutations are hard-disabled in composition;
//! - Panel/Aura startup preflight is read-only evidence only and never promotes product policy;
//! - startup performs no hardware writes and does not activate asusd merely to probe writability;
//! - reconnect/restart policy belongs to systemd.

mod product_preflight;

use std::error::Error;

use async_trait::async_trait;
use orbis_core::{aura::AuraRgb, fan::FanId, profile::AsusdFanProfile};
use orbis_hardwared::{
    AURA_POLKIT_ACTION, BATTERY_POLKIT_ACTION, DBUS_NAME, DBUS_OBJECT_PATH, FAN_POLKIT_ACTION,
    GPU_POLKIT_ACTION, HardwareService, KEYBOARD_BACKLIGHT_POLKIT_ACTION, PANEL_POLKIT_ACTION,
    PolkitAuthorizer,
    aura::{AuraMutationStatus, AuraStaticRgbMutationBackend, AuraStaticRgbMutationReadback},
    battery::{
        AsusdBatteryClient, AsusdBatteryMutationBackend, BatteryEffectiveReader,
        BatteryMutationBackend, BatteryMutationReadback, BatteryMutationStatus,
        ZbusAsusdBatteryClient, discover_effective_reader,
    },
    fans::{
        FanCurveDefaultsReadback, FanCurveMutationOperation, FanCurveMutationReadback,
        FanCurvePoints, FanMutationStatus,
    },
    keyboard_backlight::{
        KeyboardBacklightMutationBackend, KeyboardBacklightMutationReadback,
        KeyboardBacklightMutationStatus,
    },
    panel::{
        PanelOverdriveMutationBackend, PanelOverdriveMutationReadback, PanelOverdriveMutationStatus,
    },
    supergfxd::{MutationObservation, SupergfxdMutationOperation},
};
use orbis_providers::{error::ProviderError, supergfxd::SupergfxdMode};

const PRODUCT_MUTATION_DISABLED: &str =
    "mutation is disabled until the Orbis product contract and release evidence are proven";
const FAN_MUTATION_DISABLED: &str = "fan mutation is disabled until enabled-state preservation and factory-reset restoration are fixed";

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

struct DisabledFanMutationBackend;

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

struct DisabledPanelOverdriveMutationBackend;

#[async_trait]
impl PanelOverdriveMutationBackend for DisabledPanelOverdriveMutationBackend {
    async fn set_panel_overdrive(
        &self,
        _enabled: bool,
    ) -> Result<PanelOverdriveMutationReadback, ProviderError> {
        Err(ProviderError::Unsupported(PRODUCT_MUTATION_DISABLED.into()))
    }

    fn mutation_status(&self) -> PanelOverdriveMutationStatus {
        PanelOverdriveMutationStatus::Unsupported
    }
}

struct DisabledKeyboardBacklightMutationBackend;

#[async_trait]
impl KeyboardBacklightMutationBackend for DisabledKeyboardBacklightMutationBackend {
    async fn set_brightness(
        &self,
        _level: u8,
    ) -> Result<KeyboardBacklightMutationReadback, ProviderError> {
        Err(ProviderError::Unsupported(PRODUCT_MUTATION_DISABLED.into()))
    }

    fn mutation_status(&self) -> KeyboardBacklightMutationStatus {
        KeyboardBacklightMutationStatus::Unsupported
    }
}

struct DisabledAuraStaticRgbMutationBackend;

#[async_trait]
impl AuraStaticRgbMutationBackend for DisabledAuraStaticRgbMutationBackend {
    async fn set_static_rgb(
        &self,
        _rgb: AuraRgb,
    ) -> Result<AuraStaticRgbMutationReadback, ProviderError> {
        Err(ProviderError::Unsupported(PRODUCT_MUTATION_DISABLED.into()))
    }

    fn mutation_status(&self) -> AuraMutationStatus {
        AuraMutationStatus::Unsupported
    }
}

enum DisabledBatteryReason {
    Unsupported(String),
    Unavailable(String),
    PermissionDenied(String),
}

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
                    "battery mutation preflight permission denied: {error}"
                ))
            }
            other => DisabledBatteryReason::Unavailable(format!(
                "battery mutation preflight failed: {other}"
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

/// Query only the D-Bus daemon's current ownership table.
///
/// `NameHasOwner` does not activate a stopped service, unlike constructing an
/// asusd proxy and reading a property. A stopped-but-activatable asusd therefore
/// keeps Battery mutation unavailable until hardwared refresh/restart instead of
/// being started as a side effect of capability probing.
async fn build_battery_backend(connection: &zbus::Connection) -> Box<dyn BatteryMutationBackend> {
    let effective_reader = match discover_effective_reader() {
        Ok(reader) => reader,
        Err(error) => {
            tracing::warn!(?error, "battery effective threshold discovery failed");
            return Box::new(DisabledBatteryMutationBackend::from_discovery_error(error));
        }
    };

    // Construction performs no I/O; the liveness probe is a pure daemon
    // ownership-table query shared with the runtime status re-check (#107).
    match ZbusAsusdBatteryClient::new(connection.clone())
        .asusd_owned()
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            let error = ProviderError::BackendUnavailable(
                "asusd is not currently running; Battery mutation remains disabled".into(),
            );
            tracing::warn!(?error, "battery asusd owner preflight failed closed");
            return Box::new(DisabledBatteryMutationBackend::from_discovery_error(error));
        }
        Err(error) => {
            tracing::warn!(?error, "battery asusd owner preflight failed");
            return Box::new(DisabledBatteryMutationBackend::from_discovery_error(error));
        }
    }

    if let Err(error) = effective_reader.read_effective_threshold().await {
        tracing::warn!(?error, "battery effective read-back preflight failed");
        return Box::new(DisabledBatteryMutationBackend::from_discovery_error(error));
    }

    // Construction performs no I/O. The first actual asusd property access is
    // part of an authorized mutation/read-back path, not startup probing.
    let asusd = ZbusAsusdBatteryClient::new(connection.clone());
    Box::new(AsusdBatteryMutationBackend::new(asusd, effective_reader))
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

    // Product-gated paths are probed read-only so release diagnostics can
    // distinguish "implementation structurally available" from "product policy
    // approved". These results never select a mutation backend in this build.
    let panel_preflight = product_preflight::preflight_panel_overdrive(&connection).await;
    let aura_preflight = product_preflight::preflight_aura_static_rgb(&connection).await;
    tracing::info!(
        ?panel_preflight,
        ?aura_preflight,
        "product mutation startup preflight complete; Panel/Aura writes remain release-disabled"
    );

    let battery_backend = build_battery_backend(&connection).await;

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
        Box::new(DisabledFanMutationBackend),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            FAN_POLKIT_ACTION,
        )),
    )
    .with_panel(
        Box::new(DisabledPanelOverdriveMutationBackend),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            PANEL_POLKIT_ACTION,
        )),
    )
    .with_keyboard_backlight(
        Box::new(DisabledKeyboardBacklightMutationBackend),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            KEYBOARD_BACKLIGHT_POLKIT_ACTION,
        )),
    )
    .with_aura_static_rgb(
        Box::new(DisabledAuraStaticRgbMutationBackend),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            AURA_POLKIT_ACTION,
        )),
    );

    connection
        .object_server()
        .at(DBUS_OBJECT_PATH, service)
        .await?;

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
    async fn unvalidated_product_mutations_are_hard_disabled() {
        let fan = DisabledFanMutationBackend;
        assert_eq!(fan.mutation_status().await, FanMutationStatus::Unsupported);
        assert!(matches!(
            fan.reset_curves_to_defaults(AsusdFanProfile::Balanced)
                .await,
            Err(ProviderError::Unsupported(_))
        ));

        let panel = DisabledPanelOverdriveMutationBackend;
        assert_eq!(
            panel.mutation_status(),
            PanelOverdriveMutationStatus::Unsupported
        );
        assert!(matches!(
            panel.set_panel_overdrive(true).await,
            Err(ProviderError::Unsupported(_))
        ));

        let keyboard = DisabledKeyboardBacklightMutationBackend;
        assert_eq!(
            keyboard.mutation_status(),
            KeyboardBacklightMutationStatus::Unsupported
        );
        assert!(matches!(
            keyboard.set_brightness(1).await,
            Err(ProviderError::Unsupported(_))
        ));

        let aura = DisabledAuraStaticRgbMutationBackend;
        assert_eq!(aura.mutation_status(), AuraMutationStatus::Unsupported);
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

    #[test]
    fn product_disabled_status_wire_values_remain_total() {
        use orbis_hardwared::{
            aura::aura_mutation_wire, keyboard_backlight::keyboard_backlight_mutation_wire,
            panel::panel_mutation_wire,
        };

        assert_eq!(
            aura_mutation_wire::from_wire(aura_mutation_wire::to_wire(
                AuraMutationStatus::Unsupported
            )),
            Some(AuraMutationStatus::Unsupported)
        );
        assert_eq!(
            keyboard_backlight_mutation_wire::from_wire(keyboard_backlight_mutation_wire::to_wire(
                KeyboardBacklightMutationStatus::Unsupported
            )),
            Some(KeyboardBacklightMutationStatus::Unsupported)
        );
        assert_eq!(
            panel_mutation_wire::from_wire(panel_mutation_wire::to_wire(
                PanelOverdriveMutationStatus::Unsupported
            )),
            Some(PanelOverdriveMutationStatus::Unsupported)
        );
    }
}
