//! Pure software fan-policy evaluation.
//!
//! This module is intentionally hardware-inert. It evaluates an already
//! validated curve and applies smoothing/deadband/rate limiting, but it never
//! owns a fan, polls sensors or writes PWM. Firmware-managed fan control remains
//! the default unless a separate software-control backend is explicitly proven.

use serde::{Deserialize, Serialize};

use crate::{
    EmaFilter, FanCurve, FanPwm, PwmRateLimiter, TemperatureC, ThermalControlError,
};

/// Temperature deadband used to suppress small oscillations around curve
/// boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemperatureDeadband {
    width_c: i16,
    effective: Option<TemperatureC>,
}

impl TemperatureDeadband {
    /// Construct a non-negative deadband.
    pub fn new(width_c: i16) -> Self {
        Self {
            width_c: width_c.max(0),
            effective: None,
        }
    }

    /// Feed a temperature and return the effective temperature.
    pub fn update(&mut self, observed: TemperatureC) -> TemperatureC {
        let Some(previous) = self.effective else {
            self.effective = Some(observed);
            return observed;
        };

        let delta = (i32::from(observed.get()) - i32::from(previous.get())).abs();
        if delta >= i32::from(self.width_c) {
            self.effective = Some(observed);
            observed
        } else {
            previous
        }
    }
}

/// Evaluate a raw-PWM fan curve using integer linear interpolation.
///
/// The curve must be non-empty and temperature-ordered. Callers should run the
/// normal `FanCurve::validate` contract before constructing software control.
pub fn interpolate_curve(curve: &FanCurve, temperature: TemperatureC) -> Option<FanPwm> {
    let first = curve.points.first()?;
    if temperature <= first.temp {
        return Some(first.pwm);
    }

    let last = curve.points.last()?;
    if temperature >= last.temp {
        return Some(last.pwm);
    }

    for window in curve.points.windows(2) {
        let left = window[0];
        let right = window[1];
        if temperature > right.temp {
            continue;
        }

        let delta_t = i32::from(right.temp.get()) - i32::from(left.temp.get());
        if delta_t == 0 {
            return Some(right.pwm);
        }

        let position = i32::from(temperature.get()) - i32::from(left.temp.get());
        let left_pwm = i32::from(left.pwm.get());
        let delta_pwm = i32::from(right.pwm.get()) - left_pwm;
        let interpolated = left_pwm + (delta_pwm * position + delta_t / 2) / delta_t;
        let clamped = interpolated.clamp(0, 255) as u8;
        return FanPwm::new(clamped).ok();
    }

    Some(last.pwm)
}

/// Stateful pure fan-policy evaluator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareFanPolicy {
    curve: FanCurve,
    temperature_filter: EmaFilter,
    deadband: TemperatureDeadband,
    rate_limiter: PwmRateLimiter,
}

impl SoftwareFanPolicy {
    /// Construct a policy from an already validated curve.
    pub fn new(
        curve: FanCurve,
        initial_pwm: FanPwm,
        ema_numerator: u16,
        ema_denominator: u16,
        deadband_c: i16,
        max_rise_per_update: u8,
        max_fall_per_update: u8,
    ) -> Result<Self, ThermalControlError> {
        Ok(Self {
            curve,
            temperature_filter: EmaFilter::new(ema_numerator, ema_denominator)?,
            deadband: TemperatureDeadband::new(deadband_c),
            rate_limiter: PwmRateLimiter::new(
                initial_pwm,
                max_rise_per_update,
                max_fall_per_update,
            ),
        })
    }

    /// Evaluate one observed temperature into a rate-limited raw PWM target.
    pub fn update(&mut self, observed: TemperatureC) -> Option<FanPwm> {
        let filtered_raw = self.temperature_filter.update(i32::from(observed.get()));
        let filtered = TemperatureC::new(filtered_raw as i16).ok()?;
        let effective = self.deadband.update(filtered);
        let target = interpolate_curve(&self.curve, effective)?;
        Some(self.rate_limiter.update(target))
    }

    /// Borrow the configured curve.
    pub fn curve(&self) -> &FanCurve {
        &self.curve
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FanCurvePoint, FanId, PerformanceProfile};

    fn curve() -> FanCurve {
        FanCurve {
            profile: PerformanceProfile::Balanced,
            fan: FanId::Cpu,
            points: vec![
                FanCurvePoint::new(
                    TemperatureC::new(40).unwrap(),
                    FanPwm::new(20).unwrap(),
                ),
                FanCurvePoint::new(
                    TemperatureC::new(60).unwrap(),
                    FanPwm::new(100).unwrap(),
                ),
                FanCurvePoint::new(
                    TemperatureC::new(80).unwrap(),
                    FanPwm::new(200).unwrap(),
                ),
            ],
        }
    }

    #[test]
    fn interpolation_is_integer_and_bounded() {
        let c = curve();
        assert_eq!(
            interpolate_curve(&c, TemperatureC::new(30).unwrap())
                .unwrap()
                .get(),
            20
        );
        assert_eq!(
            interpolate_curve(&c, TemperatureC::new(50).unwrap())
                .unwrap()
                .get(),
            60
        );
        assert_eq!(
            interpolate_curve(&c, TemperatureC::new(90).unwrap())
                .unwrap()
                .get(),
            200
        );
    }

    #[test]
    fn duplicate_temperature_uses_right_point_without_division_by_zero() {
        let c = FanCurve {
            profile: PerformanceProfile::Balanced,
            fan: FanId::Cpu,
            points: vec![
                FanCurvePoint::new(
                    TemperatureC::new(50).unwrap(),
                    FanPwm::new(20).unwrap(),
                ),
                FanCurvePoint::new(
                    TemperatureC::new(50).unwrap(),
                    FanPwm::new(40).unwrap(),
                ),
                FanCurvePoint::new(
                    TemperatureC::new(70).unwrap(),
                    FanPwm::new(100).unwrap(),
                ),
            ],
        };
        assert_eq!(
            interpolate_curve(&c, TemperatureC::new(50).unwrap())
                .unwrap()
                .get(),
            20
        );
        assert_eq!(
            interpolate_curve(&c, TemperatureC::new(51).unwrap())
                .unwrap()
                .get(),
            43
        );
    }

    #[test]
    fn deadband_keeps_previous_effective_temperature_for_small_changes() {
        let mut deadband = TemperatureDeadband::new(3);
        assert_eq!(deadband.update(TemperatureC::new(60).unwrap()).get(), 60);
        assert_eq!(deadband.update(TemperatureC::new(62).unwrap()).get(), 60);
        assert_eq!(deadband.update(TemperatureC::new(63).unwrap()).get(), 63);
    }

    #[test]
    fn combined_policy_smooths_and_rate_limits() {
        let mut policy = SoftwareFanPolicy::new(
            curve(),
            FanPwm::new(20).unwrap(),
            1,
            1,
            0,
            10,
            20,
        )
        .unwrap();

        assert_eq!(
            policy.update(TemperatureC::new(80).unwrap()).unwrap().get(),
            30
        );
        assert_eq!(
            policy.update(TemperatureC::new(80).unwrap()).unwrap().get(),
            40
        );
    }
}
