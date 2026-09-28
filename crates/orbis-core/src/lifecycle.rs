//! Pure lifecycle events used as reconciliation inputs.
//!
//! These values do not install suspend hooks, schedule work, call
//! providers, or execute reconciliation.

use serde::{Deserialize, Serialize};

use crate::{BackendIdentity, FeatureId};

/// Typed application/backend lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifecycleEvent {
    /// Application startup boundary.
    Startup,
    /// System/session resume was reported by a higher layer.
    Resume,
    /// A previously unavailable backend became available again.
    BackendRecovered {
        /// Recovered backend identity.
        backend: BackendIdentity,
    },
    /// Capability evidence changed and may require re-evaluation.
    CapabilityChanged {
        /// Capability whose evidence changed.
        feature: FeatureId,
    },
}
