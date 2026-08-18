//! Typed fan-curve factory-default mutation boundary.
//!
//! Factory reset is profile-wide and preserves the lossless ASUS profile
//! identity (Balanced/Performance/Quiet/LowPower). The production provider
//! below talks directly from the original GUI process to Hardware1, so
//! hardwared can authorize the original caller with polkit. No sessiond
//! privileged-deputy path is introduced.

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::profile::AsusdFanProfile;
use zbus::proxy::CacheProperties;

use crate::error::ProviderError;

#[zbus::proxy(
    interface = "io.github.orbiscontrol.Hardware1",
    default_service = "io.github.orbiscontrol.Hardware",
    default_path = "/io/github/orbiscontrol/Hardware"
)]
trait Hardware1FanDefaults {
    fn reset_fan_curves_to_defaults(&self, profile: u32) -> zbus::Result<u32>;
}

/// Privileged factory-default reset for all supported fan curves of one ASUS
/// profile.
#[async_trait]
pub trait FanCurveDefaultsMutationProvider: Send + Sync {
    /// Restore the platform factory fan curves for `profile`.
    ///
    /// Implementations must not report success before the privileged backend
    /// has completed its authoritative post-reset verification.
    async fn reset_fan_curves_to_defaults(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<ApplyResult, ProviderError>;
}

/// Production direct-Hardware1 provider.
///
/// The connection is supplied by the application composition root. Creating
/// this value performs no D-Bus I/O and does not open another bus connection.
pub struct Hardware1FanDefaultsProvider {
    connection: zbus::Connection,
}

impl Hardware1FanDefaultsProvider {
    /// Construct from the application's existing system-bus connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

fn dbus_error(error: zbus::Error) -> ProviderError {
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

#[async_trait]
impl FanCurveDefaultsMutationProvider for Hardware1FanDefaultsProvider {
    async fn reset_fan_curves_to_defaults(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<ApplyResult, ProviderError> {
        let proxy = Hardware1FanDefaultsProxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(dbus_error)?;
        let confirmed = proxy
            .reset_fan_curves_to_defaults(profile.wire())
            .await
            .map_err(dbus_error)?;
        if confirmed != profile.wire() {
            return Err(ProviderError::Internal(format!(
                "Hardware1 confirmed another fan profile after factory reset: requested={}, confirmed={confirmed}",
                profile.wire()
            )));
        }
        Ok(ApplyResult::Applied)
    }
}

#[cfg(feature = "mock")]
#[async_trait]
impl FanCurveDefaultsMutationProvider for crate::mock::MockProvider {
    async fn reset_fan_curves_to_defaults(
        &self,
        _profile: AsusdFanProfile,
    ) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "mock backend does not model lossless ASUS factory fan defaults".into(),
        ))
    }
}
