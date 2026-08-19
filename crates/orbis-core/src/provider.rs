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
    fn names_cover_all_variants() {
        for (status, expected) in [
            (ProviderStatus::Healthy, "healthy"),
            (ProviderStatus::Degraded, "degraded"),
            (ProviderStatus::Unavailable, "unavailable"),
        ] {
            assert_eq!(status.as_str(), expected);
        }
    }

    #[test]
    fn serde_roundtrip_preserves_all_variants() {
        for status in [
            ProviderStatus::Healthy,
            ProviderStatus::Degraded,
            ProviderStatus::Unavailable,
        ] {
            let json = serde_json::to_string(&status).expect("serialize provider status");
            assert_eq!(json, format!("\"{}\"", status.as_str()));
            let decoded: ProviderStatus =
                serde_json::from_str(&json).expect("deserialize provider status");
            assert_eq!(decoded, status);
        }
    }
}
