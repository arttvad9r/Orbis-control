//! Pure thermal-control primitives for future advanced fan policy.
//!
//! These helpers do not own a fan, poll sensors, or perform hardware writes.
//! They can be composed later by a separately validated software fan-control
//! mode without changing firmware-managed fan semantics implicitly.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FanPwm, TemperatureC};

/// Configuration error for a thermal-control primitive.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ThermalControlError {
    /// EMA denominator must be non-zero and numerator may not exceed it.
    #[error("invalid EMA ratio {numerator}/{denominator}")]
    InvalidEmaRatio {
        /// Numerator.
        numerator: u16,
        /// Denominator.
        denominator: u16,
    },
}

/// Integer exponential moving average.
///
/// The filter uses a rational alpha (`numerator / denominator`) so no floating
/// point is required in the hardware policy domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmaFilter {
    numerator: u16,
    denominator: u16,
    value: Option<i32>,
}

impl EmaFilter {
    /// Construct an EMA filter.
    pub fn new(numerator: u16, denominator: u16) -> Result<Self, ThermalControlError> {
        if denominator == 0 || numerator > denominator {
            return Err(ThermalControlError::InvalidEmaRatio {
                numerator,
                denominator,
            });
        }
        Ok(Self {
            numerator,
            denominator,
            value: None,
        })
    }

    /// Feed one integer sample and return the filtered value.
    pub fn update(&mut self, sample: i32) -> i32 {
        let Some(previous) = self.value else {
            self.value = Some(sample);
            return sample;
        };

        let n = i64::from(self.numerator);
        let d = i64::from(self.denominator);
        let filtered =
            (n * i64::from(sample) + (d - n) * i64::from(previous) + d / 2) / d;
        let filtered = filtered as i32;
        self.value = Some(filtered);
        filtered
    }

    /// Last filtered value.
    pub fn value(&self) -> Option<i32> {
        self.value
    }
}

/// Hysteresis gate around one temperature threshold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HysteresisGate {
    threshold_c: i16,
    width_c: i16,
    active: bool,
}

impl HysteresisGate {
    /// Construct a gate. Negative hysteresis width is normalized to zero
    /// because it has no physical meaning.
    pub fn new(threshold_c: i16, width_c: i16, initially_active: bool) -> Self {
        Self {
            threshold_c,
            width_c: width_c.max(0),
            active: initially_active,
        }
    }

    /// Update the gate from an observed temperature.
    ///
    /// Inactive -> active at `threshold + width`.
    /// Active -> inactive at `threshold - width`.
    pub fn update(&mut self, temperature: TemperatureC) -> bool {
        let value = temperature.get();
        if self.active {
            if value <= self.threshold_c - self.width_c {
                self.active = false;
            }
        } else if value >= self.threshold_c + self.width_c {
            self.active = true;
        }
        self.active
    }

    /// Current gate state.
    pub fn active(&self) -> bool {
        self.active
    }
}

/// Per-update PWM rate limiter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PwmRateLimiter {
    current: FanPwm,
    max_rise: u8,
    max_fall: u8,
}

impl PwmRateLimiter {
    /// Construct a limiter from a known current PWM.
    pub fn new(current: FanPwm, max_rise: u8, max_fall: u8) -> Self {
        Self {
            current,
            max_rise,
            max_fall,
        }
    }

    /// Move toward the target by at most configured up/down steps.
    pub fn update(&mut self, target: FanPwm) -> FanPwm {
        let current = self.current.get();
        let target = target.get();
        let next = if target > current {
            current.saturating_add(self.max_rise).min(target)
        } else {
            current.saturating_sub(self.max_fall).max(target)
        };
        self.current = FanPwm::new(next).expect("u8 is always valid FanPwm");
        self.current
    }
}

/// Maximum virtual temperature over available sensor inputs.
pub fn max_temperature(values: &[TemperatureC]) -> Option<TemperatureC> {
    values.iter().copied().max()
}

/// Minimum virtual temperature over available sensor inputs.
pub fn min_temperature(values: &[TemperatureC]) -> Option<TemperatureC> {
    values.iter().copied().min()
}

/// Integer average virtual temperature.
pub fn average_temperature(values: &[TemperatureC]) -> Option<TemperatureC> {
    if values.is_empty() {
        return None;
    }
    let sum: i32 = values.iter().map(|value| i32::from(value.get())).sum();
    let average = sum / values.len() as i32;
    TemperatureC::new(average as i16).ok()
}

/// Weighted virtual temperature.
///
/// Weights are arbitrary non-negative integer units. Zero-total weight yields
/// `None` rather than inventing a value.
pub fn weighted_temperature(values: &[(TemperatureC, u16)]) -> Option<TemperatureC> {
    let total_weight: u64 = values.iter().map(|(_, weight)| u64::from(*weight)).sum();
    if total_weight == 0 {
        return None;
    }

    let weighted_sum: i64 = values
        .iter()
        .map(|(temperature, weight)| {
            i64::from(temperature.get()) * i64::from(*weight)
        })
        .sum();

    let average = weighted_sum / total_weight as i64;
    TemperatureC::new(average as i16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_is_integer_and_deterministic() {
        let mut ema = EmaFilter::new(1, 2).unwrap();
        assert_eq!(ema.update(40), 40);
        assert_eq!(ema.update(60), 50);
        assert_eq!(ema.update(70), 60);
    }

    #[test]
    fn invalid_ema_ratio_is_rejected() {
        assert!(EmaFilter::new(2, 1).is_err());
        assert!(EmaFilter::new(0, 0).is_err());
    }

    #[test]
    fn hysteresis_prevents_threshold_chatter() {
        let mut gate = HysteresisGate::new(80, 3, false);
        assert!(!gate.update(TemperatureC::new(82).unwrap()));
        assert!(gate.update(TemperatureC::new(83).unwrap()));
        assert!(gate.update(TemperatureC::new(79).unwrap()));
        assert!(!gate.update(TemperatureC::new(77).unwrap()));
    }

    #[test]
    fn pwm_rate_limiter_bounds_both_directions() {
        let mut limiter = PwmRateLimiter::new(FanPwm::new(100).unwrap(), 10, 20);
        assert_eq!(limiter.update(FanPwm::new(150).unwrap()).get(), 110);
        assert_eq!(limiter.update(FanPwm::new(50).unwrap()).get(), 90);
    }

    #[test]
    fn virtual_temperature_helpers_are_explicit_about_missing_data() {
        let cpu = TemperatureC::new(70).unwrap();
        let gpu = TemperatureC::new(80).unwrap();
        assert_eq!(max_temperature(&[cpu, gpu]), Some(gpu));
        assert_eq!(min_temperature(&[cpu, gpu]), Some(cpu));
        assert_eq!(average_temperature(&[cpu, gpu]).unwrap().get(), 75);
        assert_eq!(
            weighted_temperature(&[(cpu, 1), (gpu, 3)]).unwrap().get(),
            77
        );
        assert_eq!(average_temperature(&[]), None);
        assert_eq!(weighted_temperature(&[(cpu, 0)]), None);
    }
}
