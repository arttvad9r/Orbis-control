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
//! - raw GPU/Aura mutations remain hard-disabled in composition;
//! - Panel/Aura startup preflight is read-only evidence; Panel selects its
//!   typed backend only when the exact authoritative reader is discovered;
//! - startup performs no hardware writes and does not activate asusd merely to probe writability;
//! - reconnect/restart policy belongs to systemd.

mod product_preflight;

use std::error::Error;

use async_trait::async_trait;
use orbis_hardwared::{
    AURA_POLKIT_ACTION, BATTERY_POLKIT_ACTION, BOOT_SOUND_POLKIT_ACTION, DBUS_NAME,
    DBUS_OBJECT_PATH, FAN_POLKIT_ACTION, GPU_POLKIT_ACTION, HardwareService,
    KEYBOARD_BACKLIGHT_POLKIT_ACTION, PANEL_POLKIT_ACTION, PRODUCT_GPU_POLKIT_ACTION,
    PolkitAuthorizer,
    asus_gpu_mode::{AsusGpuMutationBackend, AsusdGpuMutationClient},
    aura::{AsusdAuraStaticRgbMutationBackend, ZbusAsusdAuraClient},
    battery::{
        AsusdBatteryClient, AsusdBatteryMutationBackend, BatteryEffectiveReader,
        BatteryMutationBackend, BatteryMutationReadback, BatteryMutationStatus,
        ZbusAsusdBatteryClient, discover_effective_reader,
    },
    fans::{AsusdFanCurveMutationBackend, ZbusAsusdFanCurveClient},
    firmware::{BootSoundMutationBackend, SysfsBootSoundIo},
    keyboard_backlight::{SysfsKeyboardBacklightIo, SysfsKeyboardBacklightMutationBackend},
    panel::{
        AsusdPanelOverdriveMutationBackend, PanelOverdriveMutationBackend,
        PanelOverdriveMutationReadback, PanelOverdriveMutationStatus,
        ZbusAsusdPanelOverdriveClient, discover_panel_overdrive_reader,
    },
    supergfxd::{MutationObservation, SupergfxdMutationOperation},
};
use orbis_providers::{error::ProviderError, supergfxd::SupergfxdMode};

const PRODUCT_MUTATION_DISABLED: &str =
    "mutation is disabled until the Orbis product contract and release evidence are proven";
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

    // Product-gated paths are probed read-only before their narrow mutation
    // backends are exposed. Aura confirmation remains config-level, not a
    // claim that the write-only hardware LED state was independently read back.
    let panel_preflight = product_preflight::preflight_panel_overdrive(&connection).await;
    let aura_preflight = product_preflight::preflight_aura_static_rgb(&connection).await;
    tracing::info!(
        ?panel_preflight,
        ?aura_preflight,
        "product mutation startup preflight complete; Aura writes use config-level confirmation"
    );

    let battery_backend = build_battery_backend(&connection).await;
    let panel_backend: Box<dyn PanelOverdriveMutationBackend> =
        match discover_panel_overdrive_reader() {
            Ok(reader) => Box::new(AsusdPanelOverdriveMutationBackend::new(
                ZbusAsusdPanelOverdriveClient::new(connection.clone()),
                reader,
            )),
            Err(error) => {
                tracing::warn!(?error, "panel overdrive mutation backend unavailable");
                Box::new(DisabledPanelOverdriveMutationBackend)
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
        Box::new(AsusdFanCurveMutationBackend::new(
            ZbusAsusdFanCurveClient::new(connection.clone()),
        )),
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
        Box::new(SysfsKeyboardBacklightMutationBackend::new(
            SysfsKeyboardBacklightIo::default(),
        )),
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
    )
    .with_product_gpu_backend(
        Box::new(AsusGpuMutationBackend::new(AsusdGpuMutationClient::new(
            connection.clone(),
        ))),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            PRODUCT_GPU_POLKIT_ACTION,
        )),
    )
    .with_boot_sound(
        Box::new(BootSoundMutationBackend::new(SysfsBootSoundIo::default())),
        Box::new(PolkitAuthorizer::with_action(
            connection.clone(),
            BOOT_SOUND_POLKIT_ACTION,
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
        let panel = DisabledPanelOverdriveMutationBackend;
        assert_eq!(
            panel.mutation_status(),
            PanelOverdriveMutationStatus::Unsupported
        );
        assert!(matches!(
            panel.set_panel_overdrive(true).await,
            Err(ProviderError::Unsupported(_))
        ));
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
            aura::{AuraMutationStatus, aura_mutation_wire},
            keyboard_backlight::{
                KeyboardBacklightMutationStatus, keyboard_backlight_mutation_wire,
            },
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
