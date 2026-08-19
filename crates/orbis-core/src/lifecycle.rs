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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_variants_are_typed_and_inert_values() {
        assert_eq!(LifecycleEvent::Startup, LifecycleEvent::Startup);
        assert_eq!(LifecycleEvent::Resume, LifecycleEvent::Resume);
        assert_eq!(
            LifecycleEvent::BackendRecovered {
                backend: BackendIdentity::simple("asusd")
            },
            LifecycleEvent::BackendRecovered {
                backend: BackendIdentity::simple("asusd")
            }
        );
        assert_eq!(
            LifecycleEvent::CapabilityChanged {
                feature: FeatureId::ChargeLimit
            },
            LifecycleEvent::CapabilityChanged {
                feature: FeatureId::ChargeLimit
            }
        );
    }

    #[test]
    fn lifecycle_event_serde_preserves_payload_identity() {
        let events = [
            LifecycleEvent::Startup,
            LifecycleEvent::Resume,
            LifecycleEvent::BackendRecovered {
                backend: BackendIdentity::simple("supergfxd"),
            },
            LifecycleEvent::CapabilityChanged {
                feature: FeatureId::GpuMux,
            },
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let back: LifecycleEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event);
        }
    }
}
