//! Typed telemetry export rows.
//!
//! Export data is explicit and allowlisted by construction. This module does
//! not crawl arbitrary diagnostics state or serialize provider internals.

use serde::{Deserialize, Serialize};

/// Freshness/completeness classification for one exported metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricFreshness {
    /// Current authoritative observation.
    Fresh,
    /// Last-known-good value retained after its freshness window expired.
    Stale,
    /// Observation is usable but known to be incomplete.
    Partial,
    /// Freshness/completeness evidence is insufficient.
    Unknown,
}

impl MetricFreshness {
    /// Stable serialization label for CSV/diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }
}

/// One allowlisted telemetry export row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricExportRow {
    /// Caller-supplied Unix timestamp milliseconds.
    pub timestamp_ms: u64,
    /// Stable metric id, for example `temperature.cpu.celsius`.
    pub metric: String,
    /// Human-readable scalar representation. Domain-specific exporters should
    /// avoid secrets and free-form provider payloads.
    pub value: String,
    /// Unit label such as `C`, `rpm`, `mW` or `%`.
    pub unit: String,
    /// Freshness/completeness evidence.
    pub freshness: MetricFreshness,
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\r') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Export allowlisted rows as CSV.
///
/// Schema is stable and includes freshness so a successful transport is never
/// silently presented as proof of a full/fresh observation.
pub fn metric_rows_to_csv(rows: &[MetricExportRow]) -> String {
    let mut output = String::from("timestamp_ms,metric,value,unit,freshness\n");
    for row in rows {
        output.push_str(&row.timestamp_ms.to_string());
        output.push(',');
        output.push_str(&csv_escape(&row.metric));
        output.push(',');
        output.push_str(&csv_escape(&row.value));
        output.push(',');
        output.push_str(&csv_escape(&row.unit));
        output.push(',');
        output.push_str(row.freshness.as_str());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_includes_freshness_and_quotes_fields() {
        let csv = metric_rows_to_csv(&[MetricExportRow {
            timestamp_ms: 10,
            metric: "fan,cpu".into(),
            value: "2600".into(),
            unit: "rpm".into(),
            freshness: MetricFreshness::Stale,
        }]);
        assert_eq!(
            csv,
            "timestamp_ms,metric,value,unit,freshness\n10,\"fan,cpu\",2600,rpm,stale\n"
        );
    }

    #[test]
    fn freshness_roundtrip_is_stable() {
        for freshness in [
            MetricFreshness::Fresh,
            MetricFreshness::Stale,
            MetricFreshness::Partial,
            MetricFreshness::Unknown,
        ] {
            let json = serde_json::to_string(&freshness).unwrap();
            let back: MetricFreshness = serde_json::from_str(&json).unwrap();
            assert_eq!(back, freshness);
        }
    }
}
