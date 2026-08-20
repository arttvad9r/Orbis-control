//! Non-mutating startup evidence for currently product-gated ASUS controls.
//!
//! This module is intentionally read-only. It never constructs a generic D-Bus
//! caller, never writes sysfs and never invokes a setter. It first checks the
//! D-Bus daemon ownership table so a stopped but activatable `asusd` is not
//! started merely to probe product writability.

use orbis_core::aura::AuraMode;
use orbis_providers::error::ProviderError;
use zbus::{Connection, fdo::DBusProxy, names::BusName};

use orbis_hardwared::aura::{AsusdAuraClient, ASUSD_AURA_DESTINATION, ZbusAsusdAuraClient};
use orbis_hardwared::panel::{PanelOverdriveReader, discover_panel_overdrive_reader};

/// Read-only result of a gated product mutation startup probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductPreflightStatus {
    /// Structural owner/state evidence required by the typed backend is present.
    Supported,
    /// The feature is structurally absent on this machine.
    Unsupported,
    /// A known owner/backend is not currently available.
    TemporarilyUnavailable,
    /// Read evidence exists but current permissions prevent observing it.
    PermissionDenied,
    /// Evidence was malformed or otherwise not trustworthy.
    Unknown,
}

fn classify_error(error: ProviderError) -> ProductPreflightStatus {
    match error {
        ProviderError::Unsupported(_) => ProductPreflightStatus::Unsupported,
        ProviderError::PermissionDenied(_) => ProductPreflightStatus::PermissionDenied,
        ProviderError::BackendUnavailable(_)
        | ProviderError::Timeout(_)
        | ProviderError::Dbus(_)
        | ProviderError::Io(_) => ProductPreflightStatus::TemporarilyUnavailable,
        ProviderError::InvalidRequest(_) | ProviderError::Internal(_) => {
            ProductPreflightStatus::Unknown
        }
    }
}

/// Query the D-Bus daemon ownership table without service activation.
async fn name_has_owner(
    connection: &Connection,
    service: &'static str,
) -> Result<bool, ProviderError> {
    let proxy = DBusProxy::new(connection)
        .await
        .map_err(|error| ProviderError::Dbus(format!("system D-Bus daemon proxy: {error}")))?;
    let bus_name = BusName::try_from(service).map_err(|error| {
        ProviderError::Internal(format!("invalid fixed D-Bus service {service}: {error}"))
    })?;
    proxy.name_has_owner(bus_name).await.map_err(|error| match error {
        zbus::fdo::Error::AccessDenied(message) => ProviderError::PermissionDenied(message),
        other => ProviderError::Dbus(format!("NameHasOwner({service}): {other}")),
    })
}

/// Prove Panel Overdrive structural readiness without mutation or asusd
/// activation. Both the authoritative kernel attribute and a currently-running
/// asusd owner are required because the typed mutation path intentionally uses
/// asusd as its sole writer and kernel sysfs only for read-back.
pub(crate) async fn preflight_panel_overdrive(
    connection: &Connection,
) -> ProductPreflightStatus {
    match name_has_owner(connection, "xyz.ljones.Asusd").await {
        Ok(true) => {}
        Ok(false) => return ProductPreflightStatus::TemporarilyUnavailable,
        Err(error) => return classify_error(error),
    }

    let reader = match discover_panel_overdrive_reader() {
        Ok(reader) => reader,
        Err(error) => return classify_error(error),
    };
    match reader.read_current_value().await {
        Ok(0 | 1) => ProductPreflightStatus::Supported,
        Ok(_) => ProductPreflightStatus::Unknown,
        Err(error) => classify_error(error),
    }
}

/// Prove Aura Static RGB config-level readiness without mutation or service
/// activation. `Static` must be explicitly advertised and the current typed
/// `LedModeData` property must be readable. This remains interactive-only
/// evidence: kernel Aura RGB state is write-only, so unattended hardware
/// confirmation is still impossible.
pub(crate) async fn preflight_aura_static_rgb(
    connection: &Connection,
) -> ProductPreflightStatus {
    match name_has_owner(connection, ASUSD_AURA_DESTINATION).await {
        Ok(true) => {}
        Ok(false) => return ProductPreflightStatus::TemporarilyUnavailable,
        Err(error) => return classify_error(error),
    }

    let client = ZbusAsusdAuraClient::new(connection.clone());
    let modes = match client.supported_basic_modes().await {
        Ok(modes) => modes,
        Err(error) => return classify_error(error),
    };
    if !modes.contains(&AuraMode::Static.to_u32()) {
        return ProductPreflightStatus::Unsupported;
    }

    match client.led_mode_data().await {
        Ok(_) => ProductPreflightStatus::Supported,
        Err(error) => classify_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_mapping_is_fail_closed() {
        assert_eq!(
            classify_error(ProviderError::Unsupported("test".into())),
            ProductPreflightStatus::Unsupported
        );
        assert_eq!(
            classify_error(ProviderError::PermissionDenied("test".into())),
            ProductPreflightStatus::PermissionDenied
        );
        assert_eq!(
            classify_error(ProviderError::BackendUnavailable("test".into())),
            ProductPreflightStatus::TemporarilyUnavailable
        );
        assert_eq!(
            classify_error(ProviderError::Internal("test".into())),
            ProductPreflightStatus::Unknown
        );
    }

    #[test]
    fn source_has_no_mutation_or_process_surface() {
        let source = include_str!("product_preflight.rs");
        for forbidden in [
            ["set_", "current_value("].concat(),
            ["set_", "led_mode_data("].concat(),
            ["std::fs::", "write"].concat(),
            ["Command", "::new"].concat(),
        ] {
            assert!(!source.contains(&forbidden), "unexpected preflight mutation: {forbidden}");
        }
        assert!(source.contains("name_has_owner"));
        assert!(source.contains("supported_basic_modes"));
        assert!(source.contains("read_current_value"));
    }
}
