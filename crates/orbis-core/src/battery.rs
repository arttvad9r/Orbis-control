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
/// - `configured_percent` — configured/reported backend value;
/// - `effective_percent` — фактически действующее hardware value, если оно
///   прочитано отдельным authoritative source;
/// - `bounds = Some` — hardware/backend constraints действительно известны;
/// - `bounds = None` — constraints неизвестны;
/// - значения не clamp-ятся.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChargeLimit {
    /// Лимит включён.
    pub enabled: bool,
    /// Configured/reported end threshold.
    pub configured_percent: Option<Percent>,
    /// Effective end threshold из authoritative hardware source.
    pub effective_percent: Option<Percent>,
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
        configured_percent: Option<Percent>,
        effective_percent: Option<Percent>,
        bounds: Option<ChargeLimitBounds>,
    ) -> std::result::Result<Self, CoreError> {
        if let Some(b) = &bounds {
            for (name, p) in [
                ("ChargeLimit.configured_percent", configured_percent),
                ("ChargeLimit.effective_percent", effective_percent),
            ] {
                if let Some(p) = p {
                    if p < b.min || p > b.max {
                        return Err(CoreError::invariant(
                            name,
                            format!("percent {p} вне [{}, {}]", b.min, b.max),
                        ));
                    }
                }
            }
        }
        Ok(Self {
            enabled,
            configured_percent,
            effective_percent,
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
        let l = ChargeLimit::new(true, Some(p(80)), Some(p(100)), Some(bounds)).unwrap();
        assert_eq!(l.configured_percent.unwrap().get(), 80);
        assert_eq!(l.effective_percent.unwrap().get(), 100);
        assert_eq!(l.bounds.unwrap().min.get(), 40);
    }

    #[test]
    fn disabled_without_percent() {
        let l = ChargeLimit::new(false, None, None, None).unwrap();
        assert!(!l.enabled);
        assert!(l.configured_percent.is_none());
        assert!(l.effective_percent.is_none());
        assert!(l.bounds.is_none());
    }

    #[test]
    fn percent_with_unknown_bounds_allowed() {
        // bounds=None: current percent допустим без выдуманного диапазона.
        let l = ChargeLimit::new(true, Some(p(80)), None, None).unwrap();
        assert_eq!(l.configured_percent.unwrap().get(), 80);
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
        assert!(ChargeLimit::new(true, Some(p(30)), Some(p(30)), Some(bounds)).is_err());
    }
}
