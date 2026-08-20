//! Pure lifecycle events used as reconciliation inputs.
//!
//! These values do not install suspend hooks, schedule work, call
//! providers, or execute reconciliation.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::{BackendIdentity, FeatureId, PowerSource};

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

/// Result of one observation in the fail-closed resume gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeGateOutcome {
    /// A `PrepareForSleep(true)` observation armed one suspend cycle.
    SleepArmed,
    /// A matching `PrepareForSleep(false)` observation was received and the
    /// gate is waiting for fresh post-resume power-source telemetry.
    AwaitingFreshTelemetry,
    /// A resume signal arrived without a preceding sleep signal in this
    /// process lifetime and is ignored rather than synthesized into an event.
    IgnoredUnpairedResume,
    /// No resume is currently pending, so telemetry cannot produce a resume
    /// event.
    IgnoredNoPendingResume,
    /// Post-resume telemetry did not contain an authoritative AC/Battery
    /// observation. The pending resume remains armed until its deadline.
    IgnoredUnknownPowerSource,
    /// Telemetry was stale or timestamped in the future relative to the caller
    /// clock. The pending resume remains armed until its deadline.
    IgnoredStaleTelemetry,
    /// Telemetry predates the matching resume signal and therefore cannot prove
    /// post-resume state. The pending resume remains armed until its deadline.
    IgnoredPreResumeTelemetry,
    /// The pending resume waited too long for trustworthy post-resume telemetry
    /// and was discarded.
    Expired,
    /// A matched resume cycle and fresh post-resume AC/Battery sample proved a
    /// typed lifecycle event. This is still only an observation, not execution.
    Ready {
        /// Proven lifecycle event.
        event: LifecycleEvent,
        /// Fresh power source observed after the resume signal.
        power_source: PowerSource,
    },
}

/// Pure state machine that pairs logind-style sleep/resume observations with a
/// fresh post-resume power-source sample before emitting [`LifecycleEvent::Resume`].
///
/// The gate intentionally does not synthesize resume from startup state, does
/// not call D-Bus, and does not perform reconciliation. A higher layer supplies
/// `PrepareForSleep` observations and telemetry timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeTelemetryGate {
    sleep_armed: bool,
    pending_resume_at: Option<SystemTime>,
    max_sample_age: Duration,
    max_resume_wait: Duration,
}

impl ResumeTelemetryGate {
    /// Construct a gate with explicit telemetry freshness and resume-wait
    /// bounds. Both durations must be non-zero.
    pub fn new(max_sample_age: Duration, max_resume_wait: Duration) -> Option<Self> {
        if max_sample_age.is_zero() || max_resume_wait.is_zero() {
            return None;
        }
        Some(Self {
            sleep_armed: false,
            pending_resume_at: None,
            max_sample_age,
            max_resume_wait,
        })
    }

    /// Consume one logind-style `PrepareForSleep(bool)` observation.
    ///
    /// `start=true` arms a new suspend cycle and invalidates any older pending
    /// resume. `start=false` is accepted only when this process previously saw
    /// the matching `true` signal.
    pub fn observe_prepare_for_sleep(
        &mut self,
        start: bool,
        observed_at: SystemTime,
    ) -> ResumeGateOutcome {
        if start {
            self.sleep_armed = true;
            self.pending_resume_at = None;
            return ResumeGateOutcome::SleepArmed;
        }

        if !self.sleep_armed {
            return ResumeGateOutcome::IgnoredUnpairedResume;
        }

        self.sleep_armed = false;
        self.pending_resume_at = Some(observed_at);
        ResumeGateOutcome::AwaitingFreshTelemetry
    }

    /// Consume one telemetry power-source observation while a resume is
    /// pending.
    ///
    /// A resume event is emitted exactly once, and only when the sample is
    /// fresh, not from the future, timestamped at or after the matching resume
    /// signal, and contains an authoritative AC/Battery value.
    pub fn observe_telemetry(
        &mut self,
        ac_online: Option<bool>,
        sample_at: SystemTime,
        now: SystemTime,
    ) -> ResumeGateOutcome {
        let Some(resume_at) = self.pending_resume_at else {
            return ResumeGateOutcome::IgnoredNoPendingResume;
        };

        let Ok(waited) = now.duration_since(resume_at) else {
            self.pending_resume_at = None;
            return ResumeGateOutcome::Expired;
        };
        if waited > self.max_resume_wait {
            self.pending_resume_at = None;
            return ResumeGateOutcome::Expired;
        }

        let Some(ac_online) = ac_online else {
            return ResumeGateOutcome::IgnoredUnknownPowerSource;
        };

        let Ok(sample_age) = now.duration_since(sample_at) else {
            return ResumeGateOutcome::IgnoredStaleTelemetry;
        };
        if sample_age > self.max_sample_age {
            return ResumeGateOutcome::IgnoredStaleTelemetry;
        }

        if sample_at.duration_since(resume_at).is_err() {
            return ResumeGateOutcome::IgnoredPreResumeTelemetry;
        }

        self.pending_resume_at = None;
        ResumeGateOutcome::Ready {
            event: LifecycleEvent::Resume,
            power_source: if ac_online {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
        }
    }

    /// Whether a matched resume is currently waiting for trustworthy telemetry.
    pub fn resume_pending(&self) -> bool {
        self.pending_resume_at.is_some()
    }
}

impl Default for ResumeTelemetryGate {
    fn default() -> Self {
        // Sysfs telemetry normally polls every second. A 3-second sample age
        // mirrors the AC/Battery edge detector; a 15-second resume window is
        // long enough for scheduler/UI wake-up without accepting old state.
        Self::new(Duration::from_secs(3), Duration::from_secs(15))
            .expect("non-zero resume gate constants")
    }
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

    #[test]
    fn resume_requires_paired_sleep_and_fresh_post_resume_telemetry() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut gate = ResumeTelemetryGate::default();

        assert_eq!(
            gate.observe_prepare_for_sleep(false, base),
            ResumeGateOutcome::IgnoredUnpairedResume
        );
        assert_eq!(
            gate.observe_prepare_for_sleep(true, base + Duration::from_secs(1)),
            ResumeGateOutcome::SleepArmed
        );
        assert_eq!(
            gate.observe_prepare_for_sleep(false, base + Duration::from_secs(2)),
            ResumeGateOutcome::AwaitingFreshTelemetry
        );
        assert!(gate.resume_pending());

        assert_eq!(
            gate.observe_telemetry(
                Some(true),
                base + Duration::from_secs(1),
                base + Duration::from_secs(2),
            ),
            ResumeGateOutcome::IgnoredPreResumeTelemetry
        );
        assert_eq!(
            gate.observe_telemetry(
                Some(false),
                base + Duration::from_secs(3),
                base + Duration::from_secs(3),
            ),
            ResumeGateOutcome::Ready {
                event: LifecycleEvent::Resume,
                power_source: PowerSource::Battery,
            }
        );
        assert!(!gate.resume_pending());
        assert_eq!(
            gate.observe_telemetry(
                Some(false),
                base + Duration::from_secs(4),
                base + Duration::from_secs(4),
            ),
            ResumeGateOutcome::IgnoredNoPendingResume
        );
    }

    #[test]
    fn unknown_and_stale_samples_do_not_consume_pending_resume() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut gate = ResumeTelemetryGate::default();
        gate.observe_prepare_for_sleep(true, base);
        gate.observe_prepare_for_sleep(false, base + Duration::from_secs(1));

        assert_eq!(
            gate.observe_telemetry(None, base + Duration::from_secs(2), base + Duration::from_secs(2)),
            ResumeGateOutcome::IgnoredUnknownPowerSource
        );
        assert!(gate.resume_pending());
        assert_eq!(
            gate.observe_telemetry(
                Some(true),
                base + Duration::from_secs(2),
                base + Duration::from_secs(7),
            ),
            ResumeGateOutcome::IgnoredStaleTelemetry
        );
        assert!(gate.resume_pending());
    }

    #[test]
    fn pending_resume_expires_instead_of_accepting_late_state() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut gate = ResumeTelemetryGate::new(Duration::from_secs(3), Duration::from_secs(5))
            .expect("valid gate");
        gate.observe_prepare_for_sleep(true, base);
        gate.observe_prepare_for_sleep(false, base + Duration::from_secs(1));

        assert_eq!(
            gate.observe_telemetry(
                Some(true),
                base + Duration::from_secs(7),
                base + Duration::from_secs(7),
            ),
            ResumeGateOutcome::Expired
        );
        assert!(!gate.resume_pending());
    }

    #[test]
    fn a_new_sleep_cycle_invalidates_an_older_pending_resume() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut gate = ResumeTelemetryGate::default();
        gate.observe_prepare_for_sleep(true, base);
        gate.observe_prepare_for_sleep(false, base + Duration::from_secs(1));
        assert!(gate.resume_pending());

        assert_eq!(
            gate.observe_prepare_for_sleep(true, base + Duration::from_secs(2)),
            ResumeGateOutcome::SleepArmed
        );
        assert!(!gate.resume_pending());
    }
}
