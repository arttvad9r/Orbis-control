//! Worker-side CPU energy-preference / boost contract; the production backend
//! lives with the other Hardware1 clients in the binary.

use async_trait::async_trait;
use orbis_core::cpu_tuning::EnergyPreference;
use orbis_providers::error::ProviderError;

/// Observed CPU tuning state plus independent write evidence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CpuTuningState {
    /// Preference of the first CPU; `None` when unreadable or not selectable
    /// (`default`/`custom`).
    pub epp: Option<EnergyPreference>,
    pub epp_supported: bool,
    pub epp_writable: bool,
    pub boost: Option<bool>,
    pub boost_writable: bool,
}

#[async_trait]
pub trait CpuTuningBackend: Send + Sync {
    async fn read(&self) -> CpuTuningState;
    async fn set_epp(&self, preference: EnergyPreference) -> Result<(), ProviderError>;
    async fn set_boost(&self, enabled: bool) -> Result<(), ProviderError>;
}
