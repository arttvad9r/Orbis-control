//! Worker-side NVIDIA clock-offset / power-limit contract; the production
//! backend lives with the other Hardware1 clients in the binary.

use async_trait::async_trait;
use orbis_core::nvidia_tuning::{NvidiaAvailability, NvidiaField, NvidiaTuningState};
use orbis_providers::error::ProviderError;

#[async_trait]
pub trait NvidiaTuningBackend: Send + Sync {
    /// Full read. Must not wake a suspended GPU.
    async fn read(&self) -> NvidiaTuningState;
    /// Cheap check that never touches the driver; used to notice a GPU that
    /// woke up or appeared so the state can be read once.
    async fn availability(&self) -> NvidiaAvailability;
    /// `interactive`: the user asked for this write, so polkit may prompt.
    /// Automatic replays pass `false` and never open an authentication dialog.
    async fn set(
        &self,
        field: NvidiaField,
        value: i32,
        interactive: bool,
    ) -> Result<(), ProviderError>;
}
