//! Typed ASUS Armoury product-GPU queue backend.

use async_trait::async_trait;
use orbis_providers::asus_gpu_mode::{
    AsusGpuMode, AsusGpuModeSnapshot, ProductGpuOutcome, classify_product_gpu_readback,
    target_values,
};
use orbis_providers::error::ProviderError;

pub const ASUSD_BUS_NAME: &str = "xyz.ljones.Asusd";
pub const ASUS_ARMOURY_BASE_PATH: &str = "/xyz/ljones/asus_armoury";
pub const ASUSD_DGPU_DISABLE_PATH: &str = "/xyz/ljones/asus_armoury/dgpu_disable";
pub const ASUSD_GPU_MUX_MODE_PATH: &str = "/xyz/ljones/asus_armoury/gpu_mux_mode";

/// Typed read/write surface for one ASUS Armoury GPU attribute.
#[async_trait]
pub trait AsusGpuAttributeClient: Send + Sync {
    async fn current_value(&self) -> Result<u32, ProviderError>;
    async fn queued_value(&self) -> Result<Option<u32>, ProviderError>;
    async fn set_value(&self, value: u32) -> Result<(), ProviderError>;
}

/// Typed pair of ASUS GPU attributes.
#[async_trait]
pub trait AsusGpuAttributePairClient: Send + Sync {
    async fn dgpu_disable(&self) -> Result<Box<dyn AsusGpuAttributeClient>, ProviderError>;
    async fn gpu_mux_mode(&self) -> Result<Box<dyn AsusGpuAttributeClient>, ProviderError>;
}

#[async_trait]
pub trait AsusProductGpuMutationOperation: Send + Sync {
    async fn read_mode(&self) -> Result<AsusGpuModeSnapshot, ProviderError>;

    async fn set_mode(
        &self,
        requested: AsusGpuMode,
    ) -> Result<AsusGpuMutationReadback, ProviderError>;
}

/// Result of one paired ASUS GPU queue operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsusGpuMutationReadback {
    pub requested: AsusGpuMode,
    pub snapshot: AsusGpuModeSnapshot,
    pub outcome: ProductGpuOutcome,
}

/// Backend that queues both ASUS GPU attributes and verifies their read-back.
pub struct AsusGpuMutationBackend<C> {
    client: C,
}

impl<C> AsusGpuMutationBackend<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }
}

impl<C> AsusGpuMutationBackend<C>
where
    C: AsusGpuAttributePairClient,
{
    pub async fn set_mode(
        &self,
        requested: AsusGpuMode,
    ) -> Result<AsusGpuMutationReadback, ProviderError> {
        let Some((target_dgpu, target_mux)) = target_values(requested) else {
            return Err(ProviderError::InvalidRequest(
                "ASUS product GPU mode is not writable or known".into(),
            ));
        };
        let dgpu = self.client.dgpu_disable().await?;
        let mux = self.client.gpu_mux_mode().await?;
        let before = snapshot(&*dgpu, &*mux).await?;
        if before.current_mode == requested && !before.reboot_required() {
            return Ok(AsusGpuMutationReadback {
                requested,
                snapshot: before,
                outcome: ProductGpuOutcome::AlreadyActive,
            });
        }
        dgpu.set_value(target_dgpu).await?;
        mux.set_value(target_mux).await.map_err(|error| {
            ProviderError::Conflict(format!(
                "ASUS GPU mode queue is partial after dgpu_disable was accepted: {error}"
            ))
        })?;
        let after = snapshot(&*dgpu, &*mux).await?;
        let outcome = classify_product_gpu_readback(requested, after.clone());
        Ok(AsusGpuMutationReadback {
            requested,
            snapshot: after,
            outcome,
        })
    }
}

#[async_trait]
impl<C> AsusProductGpuMutationOperation for AsusGpuMutationBackend<C>
where
    C: AsusGpuAttributePairClient,
{
    async fn read_mode(&self) -> Result<AsusGpuModeSnapshot, ProviderError> {
        let dgpu = self.client.dgpu_disable().await?;
        let mux = self.client.gpu_mux_mode().await?;
        snapshot(&*dgpu, &*mux).await
    }

    async fn set_mode(
        &self,
        requested: AsusGpuMode,
    ) -> Result<AsusGpuMutationReadback, ProviderError> {
        Self::set_mode(self, requested).await
    }
}

pub type AsusGpuMutationOperation<C> = AsusGpuMutationBackend<C>;

#[zbus::proxy(interface = "xyz.ljones.AsusArmoury")]
trait AsusArmouryAttribute {
    #[zbus(property)]
    fn current_value(&self) -> zbus::Result<i32>;
    #[zbus(property)]
    fn queued_gpu_value(&self) -> zbus::Result<i32>;
    #[zbus(property)]
    fn set_current_value(&self, value: i32) -> zbus::Result<()>;
}

pub struct AsusdGpuMutationClient {
    connection: zbus::Connection,
}

impl AsusdGpuMutationClient {
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl AsusGpuAttributePairClient for AsusdGpuMutationClient {
    async fn dgpu_disable(&self) -> Result<Box<dyn AsusGpuAttributeClient>, ProviderError> {
        Ok(Box::new(AsusdGpuAttributeClientImpl {
            connection: self.connection.clone(),
            path: ASUSD_DGPU_DISABLE_PATH,
        }))
    }

    async fn gpu_mux_mode(&self) -> Result<Box<dyn AsusGpuAttributeClient>, ProviderError> {
        Ok(Box::new(AsusdGpuAttributeClientImpl {
            connection: self.connection.clone(),
            path: ASUSD_GPU_MUX_MODE_PATH,
        }))
    }
}

struct AsusdGpuAttributeClientImpl {
    connection: zbus::Connection,
    path: &'static str,
}

impl AsusdGpuAttributeClientImpl {
    async fn proxy(&self) -> Result<AsusArmouryAttributeProxy<'_>, ProviderError> {
        AsusArmouryAttributeProxy::builder(&self.connection)
            .destination(ASUSD_BUS_NAME)
            .map_err(|error| ProviderError::Dbus(error.to_string()))?
            .path(self.path)
            .map_err(|error| ProviderError::Dbus(error.to_string()))?
            .build()
            .await
            .map_err(|error| ProviderError::Dbus(error.to_string()))
    }
}

#[async_trait]
impl AsusGpuAttributeClient for AsusdGpuAttributeClientImpl {
    async fn current_value(&self) -> Result<u32, ProviderError> {
        u32::try_from(
            self.proxy()
                .await?
                .current_value()
                .await
                .map_err(|error| ProviderError::Dbus(error.to_string()))?,
        )
        .map_err(|_| ProviderError::Internal("negative ASUS GPU current value".into()))
    }

    async fn queued_value(&self) -> Result<Option<u32>, ProviderError> {
        let value = self
            .proxy()
            .await?
            .queued_gpu_value()
            .await
            .map_err(|error| ProviderError::Dbus(error.to_string()))?;
        if value < 0 {
            return Ok(None);
        }
        Ok(Some(u32::try_from(value).map_err(|_| {
            ProviderError::Internal("invalid ASUS GPU queued value".into())
        })?))
    }

    async fn set_value(&self, value: u32) -> Result<(), ProviderError> {
        let value = i32::try_from(value)
            .map_err(|_| ProviderError::InvalidRequest("ASUS GPU value out of range".into()))?;
        self.proxy()
            .await?
            .set_current_value(value)
            .await
            .map_err(|error| ProviderError::Dbus(error.to_string()))
    }
}

async fn snapshot(
    dgpu: &dyn AsusGpuAttributeClient,
    mux: &dyn AsusGpuAttributeClient,
) -> Result<AsusGpuModeSnapshot, ProviderError> {
    let (current_dgpu, current_mux, queued_dgpu, queued_mux) = tokio::try_join!(
        dgpu.current_value(),
        mux.current_value(),
        dgpu.queued_value(),
        mux.queued_value(),
    )?;
    Ok(AsusGpuModeSnapshot::from_values(
        Some(current_dgpu),
        Some(current_mux),
        queued_dgpu,
        queued_mux,
    ))
}
