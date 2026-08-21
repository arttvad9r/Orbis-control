//! Narrow Hardware1 client for independently gated product controls.
//!
//! The UI must never infer writability from sysfs/service presence. Hardware1
//! publishes an explicit mutation-status wire for Panel Overdrive, keyboard
//! brightness and Aura Static RGB. This adapter preserves that evidence and
//! validates mutation read-back without exposing a generic D-Bus call surface.

use std::time::Duration;

use orbis_core::action::ApplyResult;
use orbis_core::aura::AuraRgb;
use orbis_providers::error::ProviderError;
use zbus::proxy::CacheProperties;

const CALL_TIMEOUT: Duration = Duration::from_secs(2);

// Hardware1 AuraMutationResult is a D-Bus structure with this exact ordered
// signature. A tuple keeps this UI adapter independent from daemon crate types
// and avoids adding a direct serde dependency to orbis-ui.
type AuraMutationWire = (u8, u8, u8, u8, u8, u8, u32);

#[zbus::proxy(
    interface = "io.github.orbiscontrol.Hardware1",
    default_service = "io.github.orbiscontrol.Hardware",
    default_path = "/io/github/orbiscontrol/Hardware"
)]
trait HardwareProductControls {
    fn panel_mutation_status(&self) -> zbus::Result<u8>;
    fn set_panel_overdrive(&self, enabled: bool) -> zbus::Result<u8>;

    fn keyboard_backlight_mutation_status(&self) -> zbus::Result<u8>;
    fn set_keyboard_backlight(&self, level: u8) -> zbus::Result<u8>;

    fn aura_mutation_status(&self) -> zbus::Result<u8>;
    fn set_aura_static_rgb(&self, r: u8, g: u8, b: u8) -> zbus::Result<AuraMutationWire>;
}

const AURA_OUTCOME_CONFIG_CONFIRMED: u32 = 0;

/// Effective write evidence published by Hardware1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductWriteStatus {
    Supported,
    Unsupported,
    TemporarilyUnavailable,
    PermissionDenied,
    Unknown,
}

impl ProductWriteStatus {
    pub(crate) fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }

    pub(crate) fn short_label(self) -> &'static str {
        match self {
            Self::Supported => "write ready",
            Self::Unsupported => "write disabled",
            Self::TemporarilyUnavailable => "write unavailable",
            Self::PermissionDenied => "write denied",
            Self::Unknown => "write unknown",
        }
    }
}

/// Config-confirmed Aura observation. Hardware state is not readable, so a
/// validated Hardware1 reply remains `Accepted`, never `Applied`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuraConfigObservation {
    pub(crate) requested: AuraRgb,
    pub(crate) observed: AuraRgb,
    pub(crate) result: ApplyResult,
}

/// Narrow client over one externally-owned system-bus connection.
#[derive(Clone)]
pub(crate) struct HardwareProductControlClient {
    connection: zbus::Connection,
}

impl HardwareProductControlClient {
    pub(crate) fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn connect_system() -> Result<Self, ProviderError> {
        let connection = tokio::time::timeout(CALL_TIMEOUT, zbus::Connection::system())
            .await
            .map_err(|_| ProviderError::Timeout("Hardware1 system-bus connect timed out".into()))?
            .map_err(zbus_error_to_provider)?;
        Ok(Self::new(connection))
    }

    async fn proxy(&self) -> Result<HardwareProductControlsProxy<'_>, ProviderError> {
        HardwareProductControlsProxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)
    }

    pub(crate) async fn panel_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed("panel mutation status", proxy.panel_mutation_status()).await?;
        decode_status(raw, "panel")
    }

    pub(crate) async fn keyboard_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed(
            "keyboard mutation status",
            proxy.keyboard_backlight_mutation_status(),
        )
        .await?;
        decode_status(raw, "keyboard")
    }

    pub(crate) async fn aura_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed("Aura mutation status", proxy.aura_mutation_status()).await?;
        decode_status(raw, "Aura")
    }

    pub(crate) async fn set_panel_overdrive(&self, enabled: bool) -> Result<bool, ProviderError> {
        require_supported(self.panel_status().await?, "Panel Overdrive")?;
        let proxy = self.proxy().await?;
        let raw = timed(
            "Panel Overdrive mutation",
            proxy.set_panel_overdrive(enabled),
        )
        .await?;
        let observed = match raw {
            0 => false,
            1 => true,
            other => {
                return Err(ProviderError::Internal(format!(
                    "Hardware1 Panel Overdrive returned non-boolean read-back {other}"
                )));
            }
        };
        if observed != enabled {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 Panel Overdrive read-back mismatch: requested={enabled}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    pub(crate) async fn set_keyboard_backlight(&self, level: u8) -> Result<u8, ProviderError> {
        require_supported(self.keyboard_status().await?, "Keyboard Backlight")?;
        let proxy = self.proxy().await?;
        let observed = timed(
            "keyboard backlight mutation",
            proxy.set_keyboard_backlight(level),
        )
        .await?;
        if observed != level {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 keyboard read-back mismatch: requested={level}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    #[allow(dead_code)]
    pub(crate) async fn set_aura_static_rgb(
        &self,
        requested: AuraRgb,
    ) -> Result<AuraConfigObservation, ProviderError> {
        require_supported(self.aura_status().await?, "Aura Static RGB")?;
        let proxy = self.proxy().await?;
        let wire = timed(
            "Aura Static RGB mutation",
            proxy.set_aura_static_rgb(requested.r, requested.g, requested.b),
        )
        .await?;

        let returned_requested = AuraRgb {
            r: wire.0,
            g: wire.1,
            b: wire.2,
        };
        let observed = AuraRgb {
            r: wire.3,
            g: wire.4,
            b: wire.5,
        };
        if returned_requested != requested || observed != requested {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 Aura config read-back mismatch: requested={requested:?}, returned={returned_requested:?}, observed={observed:?}"
            )));
        }
        if wire.6 != AURA_OUTCOME_CONFIG_CONFIRMED {
            return Err(ProviderError::Internal(format!(
                "Hardware1 Aura returned unknown outcome {}",
                wire.6
            )));
        }

        Ok(AuraConfigObservation {
            requested,
            observed,
            result: ApplyResult::Accepted,
        })
    }
}

async fn timed<T>(
    label: &'static str,
    future: impl std::future::Future<Output = zbus::Result<T>>,
) -> Result<T, ProviderError> {
    tokio::time::timeout(CALL_TIMEOUT, future)
        .await
        .map_err(|_| ProviderError::Timeout(format!("{label} timed out")))?
        .map_err(zbus_error_to_provider)
}

fn decode_status(raw: u8, feature: &str) -> Result<ProductWriteStatus, ProviderError> {
    match raw {
        0 => Ok(ProductWriteStatus::Supported),
        1 => Ok(ProductWriteStatus::Unsupported),
        2 => Ok(ProductWriteStatus::TemporarilyUnavailable),
        3 => Ok(ProductWriteStatus::PermissionDenied),
        4 => Ok(ProductWriteStatus::Unknown),
        other => Err(ProviderError::Internal(format!(
            "Hardware1 {feature} mutation status returned unknown wire value {other}"
        ))),
    }
}

fn require_supported(status: ProductWriteStatus, feature: &str) -> Result<(), ProviderError> {
    match status {
        ProductWriteStatus::Supported => Ok(()),
        ProductWriteStatus::Unsupported => Err(ProviderError::Unsupported(format!(
            "{feature} mutation is product/backend disabled"
        ))),
        ProductWriteStatus::TemporarilyUnavailable => Err(ProviderError::BackendUnavailable(
            format!("{feature} mutation is temporarily unavailable"),
        )),
        ProductWriteStatus::PermissionDenied => Err(ProviderError::PermissionDenied(format!(
            "{feature} mutation permission denied"
        ))),
        ProductWriteStatus::Unknown => Err(ProviderError::BackendUnavailable(format!(
            "{feature} mutation readiness is unknown"
        ))),
    }
}

fn zbus_error_to_provider(error: zbus::Error) -> ProviderError {
    if let zbus::Error::FDO(boxed) = &error {
        return match &**boxed {
            zbus::fdo::Error::NotSupported(message) => ProviderError::Unsupported(message.clone()),
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
    fn stable_status_wire_is_strict() {
        assert_eq!(
            decode_status(0, "test").unwrap(),
            ProductWriteStatus::Supported
        );
        assert_eq!(
            decode_status(1, "test").unwrap(),
            ProductWriteStatus::Unsupported
        );
        assert_eq!(
            decode_status(4, "test").unwrap(),
            ProductWriteStatus::Unknown
        );
        assert!(decode_status(5, "test").is_err());
    }

    #[test]
    fn only_supported_status_passes_mutation_gate() {
        assert!(require_supported(ProductWriteStatus::Supported, "test").is_ok());
        assert!(require_supported(ProductWriteStatus::Unsupported, "test").is_err());
        assert!(require_supported(ProductWriteStatus::PermissionDenied, "test").is_err());
        assert!(require_supported(ProductWriteStatus::Unknown, "test").is_err());
    }

    #[test]
    fn aura_semantics_remain_config_confirmed_only() {
        assert!(!ApplyResult::Accepted.is_applied());
    }

    #[test]
    fn client_has_no_generic_process_filesystem_gpu_or_fan_surface() {
        let source = include_str!("hardware_controls_backend.rs");
        let forbidden = [
            ["std::fs::", "write"].concat(),
            ["Command", "::new"].concat(),
            ["set_", "gpu_mode"].concat(),
            ["set_", "fan_curve"].concat(),
        ];
        for token in forbidden {
            assert!(
                !source.contains(&token),
                "unexpected control surface: {token}"
            );
        }
    }
}
