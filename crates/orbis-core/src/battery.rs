//! Лимит зарядки батареи.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::newtypes::Percent;

/// Известные hardware/backend constraints диапазона charge limit.
///
/// `Some(bounds)` означает, что конкретный backend/fake backend действительно
/// сообщил ограничения; `None` в `ChargeLimit.bounds` — constraints неизвестны.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChargeLimitBounds {
    /// Минимальный допустимый процент.
    pub min: Percent,
    /// Максимальный допустимый процент.
    pub max: Percent,
    /// Шаг.
    pub step: u8,
}

impl ChargeLimitBounds {
    /// Конструктор с валидацией известных bounds.
    pub fn new(min: Percent, max: Percent, step: u8) -> std::result::Result<Self, CoreError> {
        if min.get() > max.get() {
            return Err(CoreError::invariant(
                "ChargeLimitBounds.min",
                format!("min ({}) > max ({})", min, max),
            ));
        }
        if step == 0 {
            return Err(CoreError::invariant("ChargeLimitBounds.step", "step == 0"));
        }
        Ok(Self { min, max, step })
    }
}

/// Лимит зарядки.
///
/// - `bounds = Some` — hardware/backend constraints действительно известны;
/// - `bounds = None` — constraints неизвестны (current percent при этом
///   допустим без выдуманного диапазона);
/// - значения не clamp-ятся.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChargeLimit {
    /// Лимит включён.
    pub enabled: bool,
    /// Целевой процент (при выключенном лимите — None).
    pub percent: Option<Percent>,
    /// Известные hardware/backend constraints (None — неизвестны).
    pub bounds: Option<ChargeLimitBounds>,
}

impl ChargeLimit {
    /// Конструктор с валидацией.
    ///
    /// При `bounds = Some` и присутствующем `percent` процент должен лежать
    /// внутри bounds; при `bounds = None` валидный percent допустим без
    /// выдуманного диапазона.
    pub fn new(
        enabled: bool,
        percent: Option<Percent>,
        bounds: Option<ChargeLimitBounds>,
    ) -> std::result::Result<Self, CoreError> {
        if let (Some(p), Some(b)) = (percent, &bounds) {
            if p < b.min || p > b.max {
                return Err(CoreError::invariant(
                    "ChargeLimit.percent",
                    format!("percent {p} вне [{}, {}]", b.min, b.max),
                ));
            }
        }
        Ok(Self {
            enabled,
            percent,
            bounds,
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
        let bounds = ChargeLimitBounds::new(p(40), p(100), 1).unwrap();
        let l = ChargeLimit::new(true, Some(p(80)), Some(bounds)).unwrap();
        assert_eq!(l.percent.unwrap().get(), 80);
        assert_eq!(l.bounds.unwrap().min.get(), 40);
    }

    #[test]
    fn disabled_without_percent() {
        let l = ChargeLimit::new(false, None, None).unwrap();
        assert!(!l.enabled);
        assert!(l.percent.is_none());
        assert!(l.bounds.is_none());
    }

    #[test]
    fn percent_with_unknown_bounds_allowed() {
        // bounds=None: current percent допустим без выдуманного диапазона.
        let l = ChargeLimit::new(true, Some(p(80)), None).unwrap();
        assert_eq!(l.percent.unwrap().get(), 80);
        assert!(l.bounds.is_none());
    }

    #[test]
    fn bounds_min_greater_than_max_rejected() {
        assert!(ChargeLimitBounds::new(p(100), p(40), 1).is_err());
    }

    #[test]
    fn bounds_zero_step_rejected() {
        assert!(ChargeLimitBounds::new(p(40), p(100), 0).is_err());
    }

    #[test]
    fn percent_outside_range_rejected() {
        let bounds = ChargeLimitBounds::new(p(40), p(100), 1).unwrap();
        assert!(ChargeLimit::new(true, Some(p(30)), Some(bounds)).is_err());
    }
}
