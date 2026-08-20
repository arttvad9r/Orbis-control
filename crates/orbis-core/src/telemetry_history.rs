//! Bounded in-memory history for telemetry and other observations.
//!
//! The history is a fixed-capacity ring buffer. Retention is explicit and no
//! background polling is started by this type.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error constructing a history buffer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HistoryError {
    /// A ring buffer with zero capacity cannot retain evidence.
    #[error("history capacity must be greater than zero")]
    ZeroCapacity,
}

/// One timestamped sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySample<T> {
    /// Caller-supplied Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Observed value.
    pub value: T,
}

/// Fixed-capacity history preserving newest observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedHistory<T> {
    capacity: usize,
    samples: VecDeque<HistorySample<T>>,
}

impl<T> BoundedHistory<T> {
    /// Construct a bounded history.
    pub fn new(capacity: usize) -> Result<Self, HistoryError> {
        if capacity == 0 {
            return Err(HistoryError::ZeroCapacity);
        }
        Ok(Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
        })
    }

    /// Append one sample, evicting the oldest when capacity is full.
    pub fn push(&mut self, sample: HistorySample<T>) {
        if self.samples.len() == self.capacity {
            let _ = self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Number of retained samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no samples are retained.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Configured retention capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Oldest retained sample.
    pub fn oldest(&self) -> Option<&HistorySample<T>> {
        self.samples.front()
    }

    /// Newest retained sample.
    pub fn newest(&self) -> Option<&HistorySample<T>> {
        self.samples.back()
    }

    /// Iterate oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &HistorySample<T>> {
        self.samples.iter()
    }

    /// Clear retained observations without changing capacity.
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl<T: std::fmt::Display> BoundedHistory<T> {
    /// Export a simple two-column CSV (`timestamp_ms,value`).
    ///
    /// Values are RFC4180-style quoted when they contain a comma, quote, CR or
    /// LF. The caller is responsible for choosing a value type whose `Display`
    /// output is appropriate for diagnostics/export.
    pub fn to_csv(&self) -> String {
        fn escape(value: &str) -> String {
            if value.contains(',')
                || value.contains('"')
                || value.contains('\r')
                || value.contains('\n')
            {
                format!("\"{}\"", value.replace('"', "\"\""))
            } else {
                value.to_string()
            }
        }

        let mut output = String::from("timestamp_ms,value\n");
        for sample in &self.samples {
            output.push_str(&sample.timestamp_ms.to_string());
            output.push(',');
            output.push_str(&escape(&sample.value.to_string()));
            output.push('\n');
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_capacity_is_rejected() {
        assert_eq!(
            BoundedHistory::<u8>::new(0).unwrap_err(),
            HistoryError::ZeroCapacity
        );
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let mut history = BoundedHistory::new(2).unwrap();
        history.push(HistorySample {
            timestamp_ms: 1,
            value: 10,
        });
        history.push(HistorySample {
            timestamp_ms: 2,
            value: 20,
        });
        history.push(HistorySample {
            timestamp_ms: 3,
            value: 30,
        });
        assert_eq!(history.len(), 2);
        assert_eq!(history.oldest().unwrap().value, 20);
        assert_eq!(history.newest().unwrap().value, 30);
    }

    #[test]
    fn csv_export_quotes_values_safely() {
        let mut history = BoundedHistory::new(2).unwrap();
        history.push(HistorySample {
            timestamp_ms: 10,
            value: "a,b".to_string(),
        });
        assert_eq!(history.to_csv(), "timestamp_ms,value\n10,\"a,b\"\n");
    }

    #[test]
    fn serde_preserves_capacity_and_samples() {
        let mut history = BoundedHistory::new(2).unwrap();
        history.push(HistorySample {
            timestamp_ms: 1,
            value: 42u8,
        });
        let json = serde_json::to_string(&history).unwrap();
        let back: BoundedHistory<u8> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, history);
    }
}
