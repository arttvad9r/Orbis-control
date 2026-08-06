//! Лимит зарядки батареи.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::newtypes::Percent;

/// Лимит зарядки с диапазоном, возвращаемым backend-ом.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChargeLimit {
    /// Лимит включён.
    pub enabled: bool,
    /// Целевой процент (при выключенном лимите — None).
    pub percent: Option<Percent>,
    /// Минимальный допустимый процент (обычно 40, но брать у backend).
    pub min: Percent,
    /// Максимальный допустимый процент (обычно 100).
    pub max: Percent,
    /// Шаг (обычно 1, но брать у backend).
    pub step: u8,
}

impl ChargeLimit {
    /// Конструктор с валидацией.
    pub fn new(
        enabled: bool,
        percent: Option<Percent>,
        min: Percent,
        max: Percent,
        step: u8,
    ) -> std::result::Result<Self, CoreError> {
        if min.get() > max.get() {
            return Err(CoreError::invariant(
                "ChargeLimit.min",
                format!("min ({}) > max ({})", min, max),
            ));
        }
        if step == 0 {
            return Err(CoreError::invariant("ChargeLimit.step", "step == 0"));
        }
        if let Some(p) = percent {
            if p < min || p > max {
                return Err(CoreError::invariant(
                    "ChargeLimit.percent",
                    format!("percent {p} вне [{min}, {max}]"),
                ));
            }
        }
        Ok(Self {
            enabled,
            percent,
            min,
            max,
            step,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: u8) -> Percent {
        Percent::new(v).unwrap()
    }

    #[test]
    fn valid_limit() {
        let l = ChargeLimit::new(true, Some(p(80)), p(40), p(100), 1).unwrap();
        assert_eq!(l.percent.unwrap().get(), 80);
    }

    #[test]
    fn disabled_without_percent() {
        let l = ChargeLimit::new(false, None, p(40), p(100), 1).unwrap();
        assert!(!l.enabled);
        assert!(l.percent.is_none());
    }

    #[test]
    fn min_greater_than_max_rejected() {
        assert!(ChargeLimit::new(true, Some(p(80)), p(100), p(40), 1).is_err());
    }

    #[test]
    fn percent_outside_range_rejected() {
        assert!(ChargeLimit::new(true, Some(p(30)), p(40), p(100), 1).is_err());
    }

    #[test]
    fn zero_step_rejected() {
        assert!(ChargeLimit::new(true, Some(p(80)), p(40), p(100), 0).is_err());
    }
}
