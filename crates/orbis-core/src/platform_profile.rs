//! Evidence and inert transaction state for ASUS platform profiles.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::ActionRequirement;
use crate::capability::CapabilityStatus;
use crate::desired_observed::{DesiredValue, ObservedValue, PendingValue};
use crate::profile::PlatformProfile;

/// Authoritative backend which produced a platform-profile observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformProfileSource {
    /// Kernel `/sys/firmware/acpi/platform_profile`.
    Sysfs,
    /// ASUS user-space profile backend (asusd).
    Asusd,
}

/// Quality of one platform-profile observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformProfileTelemetryQuality {
    /// Current profile and source identity are known.
    Complete,
    /// The profile is usable but some source metadata is incomplete.
    Partial,
}

/// One timestamped current-profile observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformProfileTelemetry {
    /// Current profile reported by the source.
    pub current: PlatformProfile,
    /// Source of the observation.
    pub source: PlatformProfileSource,
    /// Observation timestamp.
    pub timestamp: SystemTime,
    /// Evidence quality.
    pub quality: PlatformProfileTelemetryQuality,
}

/// Evidence state resulting from comparing independent profile sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformProfileEvidenceState {
    /// Sources agree or only one authoritative source is available.
    Observed,
    /// Independent sources report different current profiles.
    Conflict,
    /// No current profile could be read.
    TemporarilyUnavailable,
}

/// Read/write capability evidence for platform profiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformProfileCapability {
    /// Read operation status.
    pub read: CapabilityStatus,
    /// Write operation status. It remains Unknown until separately validated.
    pub write: CapabilityStatus,
    /// Available choices, without selecting a current value implicitly.
    pub choices: Vec<PlatformProfile>,
    /// Result of comparing independent current-profile sources.
    pub state: PlatformProfileEvidenceState,
}

impl PlatformProfileCapability {
    /// Build read-only evidence from one or two current-profile observations.
    pub fn from_observations(
        sysfs: Option<&PlatformProfileTelemetry>,
        asusd: Option<&PlatformProfileTelemetry>,
        choices: Vec<PlatformProfile>,
    ) -> Self {
        let state = match (sysfs, asusd) {
            (Some(left), Some(right)) if left.current != right.current => {
                PlatformProfileEvidenceState::Conflict
            }
            (Some(_), Some(_)) => PlatformProfileEvidenceState::Observed,
            (Some(_), None) | (None, Some(_)) => PlatformProfileEvidenceState::Observed,
            (None, None) => PlatformProfileEvidenceState::TemporarilyUnavailable,
        };
        let read = match state {
            PlatformProfileEvidenceState::Observed => CapabilityStatus::Supported,
            PlatformProfileEvidenceState::Conflict => CapabilityStatus::Conflicted,
            PlatformProfileEvidenceState::TemporarilyUnavailable => {
                CapabilityStatus::TemporarilyUnavailable
            }
        };
        Self {
            read,
            write: CapabilityStatus::Unknown,
            choices,
            state,
        }
    }
}

/// Inert Desired/Observed/Pending platform-profile transaction state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlatformProfileTransaction {
    /// Requested intent; never applies hardware state.
    pub desired: DesiredValue<PlatformProfile>,
    /// Authoritative current observation.
    pub observed: ObservedValue<PlatformProfile>,
    /// Target awaiting confirmation or a future validated transaction.
    pub pending: Option<PendingValue<PlatformProfile>>,
}

impl PlatformProfileTransaction {
    /// Build state and mark a desired/observed mismatch as pending.
    pub fn from_values(
        desired: DesiredValue<PlatformProfile>,
        observed: ObservedValue<PlatformProfile>,
    ) -> Self {
        let pending = match (&desired, &observed) {
            (DesiredValue::Set(desired), ObservedValue::Known(observed)) if desired != observed => {
                Some(PendingValue::new(desired.clone(), ActionRequirement::None))
            }
            _ => None,
        };
        Self {
            desired,
            observed,
            pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telemetry(
        source: PlatformProfileSource,
        current: PlatformProfile,
    ) -> PlatformProfileTelemetry {
        PlatformProfileTelemetry {
            current,
            source,
            timestamp: SystemTime::UNIX_EPOCH,
            quality: PlatformProfileTelemetryQuality::Complete,
        }
    }

    #[test]
    fn matching_sources_are_observed_and_write_stays_unknown() {
        let sysfs = telemetry(PlatformProfileSource::Sysfs, PlatformProfile::Balanced);
        let asusd = telemetry(PlatformProfileSource::Asusd, PlatformProfile::Balanced);
        let capability = PlatformProfileCapability::from_observations(
            Some(&sysfs),
            Some(&asusd),
            vec![
                PlatformProfile::Quiet,
                PlatformProfile::Balanced,
                PlatformProfile::Performance,
            ],
        );
        assert_eq!(capability.read, CapabilityStatus::Supported);
        assert_eq!(capability.write, CapabilityStatus::Unknown);
    }

    #[test]
    fn divergent_sources_are_conflicted() {
        let sysfs = telemetry(PlatformProfileSource::Sysfs, PlatformProfile::Balanced);
        let asusd = telemetry(PlatformProfileSource::Asusd, PlatformProfile::Performance);
        let capability =
            PlatformProfileCapability::from_observations(Some(&sysfs), Some(&asusd), Vec::new());
        assert_eq!(capability.state, PlatformProfileEvidenceState::Conflict);
        assert_eq!(capability.read, CapabilityStatus::Conflicted);
    }

    #[test]
    fn desired_difference_is_pending() {
        let transaction = PlatformProfileTransaction::from_values(
            DesiredValue::Set(PlatformProfile::Performance),
            ObservedValue::Known(PlatformProfile::Balanced),
        );
        assert!(transaction.pending.is_some());
    }
}
