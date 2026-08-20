//! Typed client adapter for narrow Hardware1 product controls.
//!
//! This module does not infer writability from device presence. Each control
//! reads the daemon's explicit mutation-status evidence first, while mutation
//! methods preserve Hardware1 read-back semantics. Production hardwared may
//! deliberately report `Unsupported` even when a typed implementation exists;
//! callers must respect that status and keep the UI disabled.

use orbis_core::action::ApplyResult;
use orbis_core::aura::AuraRgb;
use orbis_hardwared::aura::{
    AURA_OUTCOME_CONFIG_CONFIRMED, AuraMutationStatus, AuraMutationResult, aura_mutation_wire,
};
use orbis_hardwared::keyboard_backlight::{
    KeyboardBacklightMutationStatus, keyboard_backlight_mutation_wire,
};
use orbis_hardwared::panel::{PanelOverdriveMutationStatus, panel_mutation_wire};
use orbis_hardwared::Hardware1Proxy;
use orbis_providers::error::ProviderError;
use zbus::proxy::CacheProperties;

/// Common product-facing availability classification for one Hardware1 write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareControlWriteStatus {
    /// Hardware1 reports a proven mutation backend.
    Supported,
    /// Product/backend contract explicitly disables or lacks the mutation.
    Unsupported,
    /// Backend is known but temporarily unavailable.
    TemporarilyUnavailable,
    /// Backend exists but current authorization evidence denies access.
    PermissionDenied,
    /// Evidence cannot be classified safely.
    Unknown,
}

impl HardwareControlWriteStatus {
    /// Whether UI mutation may be offered at this evidence layer.
    ///
    /// A caller still must validate the concrete requested value and handle
    /// operation-time authorization/read-back failures.
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// Config-confirmed Aura Static RGB observation.
///
/// Hardware state is intentionally not claimed because the kernel Aura path is
/// write-only. [`ApplyResult::Accepted`] preserves that distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraStaticRgbObservation {
    /// Requested primary RGB.
    pub requested: AuraRgb,
    /// Config-level RGB read back from asusd.
    pub observed: AuraRgb,
    /// Always [`ApplyResult::Accepted`] for a validated Hardware1 response.
    pub result: ApplyResult,
}

/// Narrow Hardware1 client over an externally-owned system-bus connection.
///
/// Construction performs no I/O. Every status/read/write call builds a proxy
/// with property caching disabled and performs a fresh D-Bus operation.
#[derive(Clone)]
pub struct HardwareControlsClient {
    connection: zbus::Connection,
}

impl HardwareControlsClient {
    /// Construct without opening a bus or performing a D-Bus call.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<Hardware1Proxy<'_>, ProviderError> {
        Hardware1Proxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)
    }

    /// Read effective Panel Overdrive mutation status from Hardware1.
    pub async fn panel_overdrive_write_status(
        &self,
    ) -> Result<HardwareControlWriteStatus, ProviderError> {
        let raw = self
            .proxy()
            .await?
            .panel_mutation_status()
            .await
            .map_err(zbus_error_to_provider)?;
        let status = panel_mutation_wire::from_wire(raw).ok_or_else(|| {
            ProviderError::Internal(format!(
                "Hardware1 returned unknown panel mutation status wire value {raw}"
            ))
        })?;
        Ok(map_panel_status(status))
    }

    /// Read effective keyboard-backlight mutation status from Hardware1.
    pub async fn keyboard_backlight_write_status(
        &self,
    ) -> Result<HardwareControlWriteStatus, ProviderError> {
        let raw = self
            .proxy()
            .await?
            .keyboard_backlight_mutation_status()
            .await
            .map_err(zbus_error_to_provider)?;
        let status = keyboard_backlight_mutation_wire::from_wire(raw).ok_or_else(|| {
            ProviderError::Internal(format!(
                "Hardware1 returned unknown keyboard mutation status wire value {raw}"
            ))
        })?;
        Ok(map_keyboard_status(status))
    }

    /// Read effective Aura Static RGB mutation status from Hardware1.
    pub async fn aura_static_rgb_write_status(
        &self,
    ) -> Result<HardwareControlWriteStatus, ProviderError> {
        let raw = self
            .proxy()
            .await?
            .aura_mutation_status()
            .await
            .map_err(zbus_error_to_provider)?;
        let status = aura_mutation_wire::from_wire(raw).ok_or_else(|| {
            ProviderError::Internal(format!(
                "Hardware1 returned unknown Aura mutation status wire value {raw}"
            ))
        })?;
        Ok(map_aura_status(status))
    }

    /// Request Panel Overdrive through Hardware1 and require exact observed
    /// read-back before returning success.
    pub async fn set_panel_overdrive(&self, enabled: bool) -> Result<bool, ProviderError> {
        let observed = self
            .proxy()
            .await?
            .set_panel_overdrive(enabled)
            .await
            .map_err(zbus_error_to_provider)?;
        let observed = match observed {
            0 => false,
            1 => true,
            other => {
                return Err(ProviderError::Internal(format!(
                    "Hardware1 panel read-back is not boolean: {other}"
                )));
            }
        };
        if observed != enabled {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 panel read-back mismatch: requested={enabled}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    /// Request keyboard brightness through Hardware1 and require the returned
    /// authoritative read-back to match exactly.
    pub async fn set_keyboard_backlight(&self, level: u8) -> Result<u8, ProviderError> {
        let observed = self
            .proxy()
            .await?
            .set_keyboard_backlight(level)
            .await
            .map_err(zbus_error_to_provider)?;
        if observed != level {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 keyboard read-back mismatch: requested={level}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    /// Request Aura Static RGB and preserve config-confirmed-vs-hardware-applied
    /// semantics. A successful result is `Accepted`, never `Applied`.
    pub async fn set_aura_static_rgb(
        &self,
        rgb: AuraRgb,
    ) -> Result<AuraStaticRgbObservation, ProviderError> {
        let wire = self
            .proxy()
            .await?
            .set_aura_static_rgb(rgb.r, rgb.g, rgb.b)
            .await
            .map_err(zbus_error_to_provider)?;
        validate_aura_observation(rgb, wire)
    }
}

fn map_panel_status(status: PanelOverdriveMutationStatus) -> HardwareControlWriteStatus {
    match status {
        PanelOverdriveMutationStatus::Supported => HardwareControlWriteStatus::Supported,
        PanelOverdriveMutationStatus::Unsupported => HardwareControlWriteStatus::Unsupported,
        PanelOverdriveMutationStatus::TemporarilyUnavailable => {
            HardwareControlWriteStatus::TemporarilyUnavailable
        }
        PanelOverdriveMutationStatus::PermissionDenied => {
            HardwareControlWriteStatus::PermissionDenied
        }
        PanelOverdriveMutationStatus::Unknown => HardwareControlWriteStatus::Unknown,
    }
}

fn map_keyboard_status(status: KeyboardBacklightMutationStatus) -> HardwareControlWriteStatus {
    match status {
        KeyboardBacklightMutationStatus::Supported => HardwareControlWriteStatus::Supported,
        KeyboardBacklightMutationStatus::Unsupported => HardwareControlWriteStatus::Unsupported,
        KeyboardBacklightMutationStatus::TemporarilyUnavailable => {
            HardwareControlWriteStatus::TemporarilyUnavailable
        }
        KeyboardBacklightMutationStatus::PermissionDenied => {
            HardwareControlWriteStatus::PermissionDenied
        }
        KeyboardBacklightMutationStatus::Unknown => HardwareControlWriteStatus::Unknown,
    }
}

fn map_aura_status(status: AuraMutationStatus) -> HardwareControlWriteStatus {
    match status {
        AuraMutationStatus::Supported => HardwareControlWriteStatus::Supported,
        AuraMutationStatus::Unsupported => HardwareControlWriteStatus::Unsupported,
        AuraMutationStatus::TemporarilyUnavailable => {
            HardwareControlWriteStatus::TemporarilyUnavailable
        }
        AuraMutationStatus::PermissionDenied => HardwareControlWriteStatus::PermissionDenied,
        AuraMutationStatus::Unknown => HardwareControlWriteStatus::Unknown,
    }
}

fn validate_aura_observation(
    requested: AuraRgb,
    wire: AuraMutationResult,
) -> Result<AuraStaticRgbObservation, ProviderError> {
    let returned_requested = AuraRgb {
        r: wire.requested_r,
        g: wire.requested_g,
        b: wire.requested_b,
    };
    let observed = AuraRgb {
        r: wire.observed_r,
        g: wire.observed_g,
        b: wire.observed_b,
    };
    if returned_requested != requested || observed != requested {
        return Err(ProviderError::BackendUnavailable(format!(
            "Hardware1 Aura config read-back mismatch: requested={requested:?}, returned_requested={returned_requested:?}, observed={observed:?}"
        )));
    }
    if wire.outcome != AURA_OUTCOME_CONFIG_CONFIRMED {
        return Err(ProviderError::Internal(format!(
            "Hardware1 Aura returned unknown outcome {}",
            wire.outcome
        )));
    }
    Ok(AuraStaticRgbObservation {
        requested,
        observed,
        result: ApplyResult::Accepted,
    })
}

fn zbus_error_to_provider(error: zbus::Error) -> ProviderError {
    if let zbus::Error::FDO(boxed) = &error {
        return match &**boxed {
            zbus::fdo::Error::NotSupported(message) => {
                ProviderError::Unsupported(message.clone())
            }
            zbus::fdo::Error::AccessDenied(message) => {
                ProviderError::PermissionDenied(message.clone())
            }
            zbus::fdo::Error::InvalidArgs(message) => {
                ProviderError::InvalidRequest(message.clone())
            }
            _ => ProviderError::Dbus(error.to_string()),
        };
    }
    ProviderError::Dbus(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_preserves_product_disabled_state() {
        assert_eq!(
            map_panel_status(PanelOverdriveMutationStatus::Unsupported),
            HardwareControlWriteStatus::Unsupported
        );
        assert_eq!(
            map_keyboard_status(KeyboardBacklightMutationStatus::PermissionDenied),
            HardwareControlWriteStatus::PermissionDenied
        );
        assert_eq!(
            map_aura_status(AuraMutationStatus::TemporarilyUnavailable),
            HardwareControlWriteStatus::TemporarilyUnavailable
        );
    }

    #[test]
    fn aura_config_confirmation_never_becomes_applied() {
        let rgb = AuraRgb { r: 10, g: 20, b: 30 };
        let wire = AuraMutationResult {
            requested_r: 10,
            requested_g: 20,
            requested_b: 30,
            observed_r: 10,
            observed_g: 20,
            observed_b: 30,
            outcome: AURA_OUTCOME_CONFIG_CONFIRMED,
        };
        let observation = validate_aura_observation(rgb, wire).unwrap();
        assert_eq!(observation.result, ApplyResult::Accepted);
        assert!(!observation.result.is_applied());
    }

    #[test]
    fn mismatching_aura_payload_is_not_success() {
        let rgb = AuraRgb { r: 10, g: 20, b: 30 };
        let wire = AuraMutationResult {
            requested_r: 10,
            requested_g: 20,
            requested_b: 30,
            observed_r: 11,
            observed_g: 20,
            observed_b: 30,
            outcome: AURA_OUTCOME_CONFIG_CONFIRMED,
        };
        assert!(validate_aura_observation(rgb, wire).is_err());
    }

    #[test]
    fn adapter_has_no_generic_hardware_or_process_surface() {
        let source = include_str!("hardware_controls.rs");
        for forbidden in [
            ["std::fs::", "write"].concat(),
            ["Command", "::new"].concat(),
            ["set_", "fan_curve"].concat(),
            ["set_", "gpu_mode"].concat(),
        ] {
            assert!(!source.contains(&forbidden), "unexpected surface: {forbidden}");
        }
    }
}
