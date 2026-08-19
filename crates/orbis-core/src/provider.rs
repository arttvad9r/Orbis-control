//! Canonical provider status used by core diagnostics/state models.
//!
//! This type is intentionally small and transport-agnostic. Detailed backend
//! error text and operational evidence belong in provider/capability layers;
//! callers should not infer feature support from this coarse health value.

use serde::{Deserialize, Serialize};

/// Coarse health of a provider/backend.
///
/// This is not a capability-support result. A healthy provider may still not
/// implement a particular feature, while a degraded provider may continue to
/// serve some independent capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    /// Provider is operating normally.
    Healthy,
    /// Provider is operating with reduced functionality or reliability.
    Degraded,
    /// Provider is currently unavailable.
    Unavailable,
}

impl ProviderStatus {
    /// Return the stable diagnostic/serialization-style label.
    ///
    /// The returned string is intended for diagnostics and machine-adjacent
    /// presentation, not as localized end-user copy.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(ProviderStatus::Healthy.as_str(), "healthy");
        assert_eq!(ProviderStatus::Unavailable.as_str(), "unavailable");
    }
}
