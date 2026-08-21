//! Pure alert evaluation primitives.
//!
//! Alerts consume already-observed state and never perform remediation by
//! themselves. This keeps diagnostics/notifications separate from hardware
//! mutation policy.

use serde::{Deserialize, Serialize};

use crate::{FanId, FeatureId, Rpm, TemperatureC};

/// Alert severity independent from localized presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    /// Informational anomaly.
    Info,
    /// User attention is recommended.
    Warning,
    /// Potentially unsafe or non-functional condition.
    Critical,
}

/// Stable alert event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertEvent {
    /// Stable rule identifier.
    pub rule_id: String,
    /// Severity.
    pub severity: AlertSeverity,
    /// Technical detail suitable for diagnostics.
    pub detail: String,
}

/// Evaluate a temperature upper bound.
pub fn temperature_above(
    rule_id: impl Into<String>,
    observed: TemperatureC,
    threshold: TemperatureC,
    severity: AlertSeverity,
) -> Option<AlertEvent> {
    (observed > threshold).then(|| AlertEvent {
        rule_id: rule_id.into(),
        severity,
        detail: format!(
            "temperature {} exceeds threshold {}",
            observed.get(),
            threshold.get()
        ),
    })
}

/// Detect a fan that reports zero RPM while temperature is above the rule's
/// minimum meaningful threshold.
pub fn fan_stopped_when_hot(
    rule_id: impl Into<String>,
    fan: FanId,
    rpm: Rpm,
    temperature: TemperatureC,
    minimum_hot: TemperatureC,
) -> Option<AlertEvent> {
    (rpm.get() == 0 && temperature >= minimum_hot).then(|| AlertEvent {
        rule_id: rule_id.into(),
        severity: AlertSeverity::Critical,
        detail: format!(
            "fan {fan:?} reports 0 rpm at {} C",
            temperature.get()
        ),
    })
}

/// Detect Desired/Observed divergence that persisted for too many completed
/// reconciliation observations.
pub fn persistent_divergence(
    rule_id: impl Into<String>,
    feature: FeatureId,
    divergent_observations: u32,
    threshold: u32,
) -> Option<AlertEvent> {
    (divergent_observations >= threshold && threshold > 0).then(|| AlertEvent {
        rule_id: rule_id.into(),
        severity: AlertSeverity::Warning,
        detail: format!(
            "feature {} remained divergent for {} observations",
            feature.as_str(),
            divergent_observations
        ),
    })
}

/// Detect stale telemetry from monotonic/elapsed age supplied by the caller.
pub fn telemetry_stale(
    rule_id: impl Into<String>,
    age_ms: u64,
    max_age_ms: u64,
) -> Option<AlertEvent> {
    (age_ms > max_age_ms).then(|| AlertEvent {
        rule_id: rule_id.into(),
        severity: AlertSeverity::Warning,
        detail: format!("telemetry age {age_ms} ms exceeds {max_age_ms} ms"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_rule_only_fires_above_threshold() {
        assert!(temperature_above(
            "cpu-hot",
            TemperatureC::new(90).unwrap(),
            TemperatureC::new(85).unwrap(),
            AlertSeverity::Warning,
        )
        .is_some());
        assert!(temperature_above(
            "cpu-hot",
            TemperatureC::new(85).unwrap(),
            TemperatureC::new(85).unwrap(),
            AlertSeverity::Warning,
        )
        .is_none());
    }

    #[test]
    fn stopped_fan_requires_hot_context() {
        assert!(fan_stopped_when_hot(
            "cpu-fan-stopped",
            FanId::Cpu,
            Rpm::new(0).unwrap(),
            TemperatureC::new(80).unwrap(),
            TemperatureC::new(70).unwrap(),
        )
        .is_some());
        assert!(fan_stopped_when_hot(
            "cpu-fan-stopped",
            FanId::Cpu,
            Rpm::new(0).unwrap(),
            TemperatureC::new(40).unwrap(),
            TemperatureC::new(70).unwrap(),
        )
        .is_none());
    }

    #[test]
    fn divergence_and_staleness_are_thresholded() {
        assert!(persistent_divergence("perf-diverged", FeatureId::Performance, 3, 3).is_some());
        assert!(persistent_divergence("perf-diverged", FeatureId::Performance, 2, 3).is_none());
        assert!(telemetry_stale("telemetry-stale", 5001, 5000).is_some());
        assert!(telemetry_stale("telemetry-stale", 5000, 5000).is_none());
    }
}
