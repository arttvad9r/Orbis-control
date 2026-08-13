//! Internal typed mutation adapter for the staged supergfxd contract.
//!
//! This module deliberately has no Hardware1 exposure.  GPU lifecycle remains
//! owned by supergfxd; hardwared only validates, performs one typed request and
//! classifies a fresh read-back snapshot.

use async_trait::async_trait;
use orbis_core::gpu::GpuPowerState;
use orbis_providers::error::ProviderError;
use orbis_providers::supergfxd::{
    SupergfxdMode, SupergfxdSnapshot, SupergfxdStagedState, SupergfxdUserAction,
    classify_supergfxd_state,
};
use zbus::proxy::CacheProperties;

pub const SUPERGFXD_BUS_NAME: &str = "org.supergfxctl.Daemon";
pub const SUPERGFXD_OBJECT_PATH: &str = "/org/supergfxctl/Gfx";
pub const SUPERGFXD_INTERFACE: &str = "org.supergfxctl.Daemon";

/// Узкий typed-контракт mutation backend-а.
#[async_trait]
pub trait SupergfxdMutationClient: Send + Sync {
    async fn supported(&self) -> Result<Vec<SupergfxdMode>, ProviderError>;
    async fn set_mode(&self, mode: SupergfxdMode) -> Result<SupergfxdUserAction, ProviderError>;
    async fn mode(&self) -> Result<SupergfxdMode, ProviderError>;
    async fn pending_mode(&self) -> Result<SupergfxdMode, ProviderError>;
    async fn pending_user_action(&self) -> Result<SupergfxdUserAction, ProviderError>;
    async fn power(&self) -> Result<GpuPowerState, ProviderError>;
}

/// Evidence одной операции; это не product-level state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationObservation {
    pub requested: SupergfxdMode,
    pub returned_action: SupergfxdUserAction,
    pub snapshot: SupergfxdSnapshot,
    pub state: SupergfxdStagedState,
}

/// Internal operation over a typed supergfxd client.
pub struct SupergfxdMutationBackend<C> {
    client: C,
}

impl<C> SupergfxdMutationBackend<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }
}

impl<C> SupergfxdMutationBackend<C>
where
    C: SupergfxdMutationClient,
{
    /// Validate, perform at most one SetMode, then classify fresh read-back.
    pub async fn request_mode(
        &self,
        requested: SupergfxdMode,
    ) -> Result<MutationObservation, ProviderError> {
        if matches!(requested, SupergfxdMode::Unknown(_)) {
            return Err(ProviderError::InvalidRequest(
                "unknown supergfxd mode cannot be requested".into(),
            ));
        }

        let supported = self.client.supported().await?;
        if !supported.contains(&requested) {
            return Err(ProviderError::Unsupported(format!(
                "supergfxd mode {requested:?} is not supported"
            )));
        }

        let returned_action = self.client.set_mode(requested).await?;
        let snapshot = self.fresh_snapshot().await?;
        let mut state = classify_supergfxd_state(requested, &snapshot);
        if returned_action != snapshot.pending_user_action {
            state = SupergfxdStagedState::Inconsistent;
        }

        Ok(MutationObservation {
            requested,
            returned_action,
            snapshot,
            state,
        })
    }

    async fn fresh_snapshot(&self) -> Result<SupergfxdSnapshot, ProviderError> {
        let current_mode = self.client.mode().await?;
        let pending_mode = self.client.pending_mode().await?;
        let pending_user_action = self.client.pending_user_action().await?;
        let power = self.client.power().await?;
        let supported_modes = self.client.supported().await?;
        Ok(SupergfxdSnapshot {
            current_mode,
            pending_mode,
            pending_user_action,
            power,
            supported_modes,
        })
    }
}

#[zbus::proxy(
    interface = "org.supergfxctl.Daemon",
    default_service = "org.supergfxctl.Daemon",
    default_path = "/org/supergfxctl/Gfx"
)]
trait SupergfxdDaemon {
    fn supported(&self) -> zbus::Result<Vec<u32>>;
    fn set_mode(&self, mode: u32) -> zbus::Result<u32>;
    fn mode(&self) -> zbus::Result<u32>;
    fn pending_mode(&self) -> zbus::Result<u32>;
    fn pending_user_action(&self) -> zbus::Result<u32>;
    fn power(&self) -> zbus::Result<u32>;
}

/// Production typed zbus client. Construction performs no I/O.
pub struct ZbusSupergfxdMutationClient {
    connection: zbus::Connection,
}

impl ZbusSupergfxdMutationClient {
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<SupergfxdDaemonProxy<'_>, ProviderError> {
        SupergfxdDaemonProxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error)
    }
}

fn zbus_error(error: zbus::Error) -> ProviderError {
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

fn power_from_wire(raw: u32) -> GpuPowerState {
    match raw {
        0 => GpuPowerState::Active,
        1 => GpuPowerState::Suspended,
        2 => GpuPowerState::Off,
        _ => GpuPowerState::Unknown,
    }
}

#[async_trait]
impl SupergfxdMutationClient for ZbusSupergfxdMutationClient {
    async fn supported(&self) -> Result<Vec<SupergfxdMode>, ProviderError> {
        Ok(self
            .proxy()
            .await?
            .supported()
            .await
            .map_err(zbus_error)?
            .into_iter()
            .map(SupergfxdMode::from_wire)
            .collect())
    }

    async fn set_mode(&self, mode: SupergfxdMode) -> Result<SupergfxdUserAction, ProviderError> {
        let raw = match mode {
            SupergfxdMode::Hybrid => 0,
            SupergfxdMode::Integrated => 1,
            SupergfxdMode::NvidiaNoModeset => 2,
            SupergfxdMode::Vfio => 3,
            SupergfxdMode::AsusEgpu => 4,
            SupergfxdMode::AsusMuxDgpu => 5,
            SupergfxdMode::None | SupergfxdMode::Unknown(_) => {
                return Err(ProviderError::InvalidRequest(
                    "mode is not a valid SetMode request".into(),
                ));
            }
        };
        Ok(SupergfxdUserAction::from_wire(
            self.proxy()
                .await?
                .set_mode(raw)
                .await
                .map_err(zbus_error)?,
        ))
    }

    async fn mode(&self) -> Result<SupergfxdMode, ProviderError> {
        Ok(SupergfxdMode::from_wire(
            self.proxy().await?.mode().await.map_err(zbus_error)?,
        ))
    }

    async fn pending_mode(&self) -> Result<SupergfxdMode, ProviderError> {
        Ok(SupergfxdMode::from_wire(
            self.proxy()
                .await?
                .pending_mode()
                .await
                .map_err(zbus_error)?,
        ))
    }

    async fn pending_user_action(&self) -> Result<SupergfxdUserAction, ProviderError> {
        Ok(SupergfxdUserAction::from_wire(
            self.proxy()
                .await?
                .pending_user_action()
                .await
                .map_err(zbus_error)?,
        ))
    }

    async fn power(&self) -> Result<GpuPowerState, ProviderError> {
        Ok(power_from_wire(
            self.proxy().await?.power().await.map_err(zbus_error)?,
        ))
    }
}
