//! Integer telemetry statistics.
//!
//! Statistics are derived from already-retained observations. No polling or
//! persistence is performed by this module.

use serde::{Deserialize, Serialize};

/// Basic statistics for integer telemetry.
///
/// Mean is represented as `mean_milli` (value * 1000) to avoid floating point
/// in the domain layer while retaining useful precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegerStats {
    /// Minimum sample.
    pub min: i64,
    /// Maximum sample.
    pub max: i64,
    /// Arithmetic mean multiplied by 1000.
    pub mean_milli: i64,
    /// Number of samples.
    pub count: u64,
}

/// Compute min/max/mean for an integer iterator.
///
/// Empty input returns `None`. Sum uses `i128` to avoid overflow for realistic
/// telemetry histories before conversion back to `i64`.
pub fn integer_stats<I>(values: I) -> Option<IntegerStats>
where
    I: IntoIterator<Item = i64>,
{
    let mut iter = values.into_iter();
    let first = iter.next()?;
    let mut min = first;
    let mut max = first;
    let mut sum = i128::from(first);
    let mut count: u64 = 1;

    for value in iter {
        min = min.min(value);
        max = max.max(value);
        sum += i128::from(value);
        count = count.saturating_add(1);
    }

    let scaled = sum.saturating_mul(1000);
    let mean = scaled / i128::from(count);
    let mean_milli = i64::try_from(mean).unwrap_or_else(|_| {
        if mean.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    });

    Some(IntegerStats {
        min,
        max,
        mean_milli,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_no_statistics() {
        assert_eq!(integer_stats(Vec::<i64>::new()), None);
    }

    #[test]
    fn statistics_preserve_fractional_mean_without_float() {
        assert_eq!(
            integer_stats([10, 11]),
            Some(IntegerStats {
                min: 10,
                max: 11,
                mean_milli: 10_500,
                count: 2,
            })
        );
    }

    #[test]
    fn negative_values_are_supported() {
        let stats = integer_stats([-10, 0, 10]).unwrap();
        assert_eq!(stats.min, -10);
        assert_eq!(stats.max, 10);
        assert_eq!(stats.mean_milli, 0);
    }
}
