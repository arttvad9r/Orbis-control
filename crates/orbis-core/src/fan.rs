//! Вентиляторы и кривые.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::newtypes::{Percent, TemperatureC};
use crate::profile::PerformanceProfile;

/// Идентификатор вентилятора.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanId {
    /// CPU-вентилятор.
    Cpu,
    /// GPU-вентилятор.
    Gpu,
    /// Mid/System-вентилятор.
    Mid,
    /// Системный вентилятор.
    System,
    /// Другой вентилятор (например, из label).
    Other(String),
}

impl FanId {
    /// Стабильный ключ для маппинга (label из hwmon и т.п.).
    pub fn key(&self) -> String {
        match self {
            Self::Cpu => "cpu".to_string(),
            Self::Gpu => "gpu".to_string(),
            Self::Mid => "mid".to_string(),
            Self::System => "system".to_string(),
            Self::Other(s) => format!("other:{s}"),
        }
    }
}

/// Точка кривой вентилятора: температура -> процент ШИМ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanCurvePoint {
    /// Температура, °C.
    pub temp: TemperatureC,
    /// Значение вентилятора в процентах от максимума.
    pub pwm: Percent,
}

impl FanCurvePoint {
    /// Конструктор с валидацией.
    pub fn new(temp: TemperatureC, pwm: Percent) -> Self {
        Self { temp, pwm }
    }
}

/// Кривая вентилятора для одного профиля производительности.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanCurve {
    /// Профиль, к которому относится кривая.
    pub profile: PerformanceProfile,
    /// Вентилятор.
    pub fan: FanId,
    /// Точки кривой. Количество точек задаёт backend (обычно 8).
    pub points: Vec<FanCurvePoint>,
}

impl FanCurve {
    /// Валидация кривой.
    ///
    /// Правила (см. спецификацию):
    /// - количество точек в допустимом диапазоне [1, max_points];
    /// - температуры не убывают;
    /// - значения не убывают, если backend не допускает обратное
    ///   (в ядре политика задаётся `allow_decreasing`);
    /// - значения в пределах [0, 100];
    /// - последняя точка обеспечивает охлаждение (не 0 % при высокой температуре).
    pub fn validate(
        &self,
        max_points: usize,
        allow_decreasing: bool,
    ) -> std::result::Result<(), CoreError> {
        if self.points.is_empty() {
            return Err(CoreError::invariant("FanCurve.points", "пустая кривая"));
        }
        if self.points.len() > max_points {
            return Err(CoreError::invariant(
                "FanCurve.points",
                format!("точек {} > максимума {max_points}", self.points.len()),
            ));
        }
        for w in self.points.windows(2) {
            if w[1].temp < w[0].temp {
                return Err(CoreError::invariant(
                    "FanCurve.points",
                    format!("температуры убывают: {} -> {}", w[0].temp, w[1].temp),
                ));
            }
            if !allow_decreasing && w[1].pwm < w[0].pwm {
                return Err(CoreError::invariant(
                    "FanCurve.points",
                    format!("значения убывают: {} -> {}", w[0].pwm, w[1].pwm),
                ));
            }
        }
        let last = self.points.last().expect("non-empty");
        // Защита от отключения вентилятора на критической температуре.
        if last.temp >= TemperatureC::new(80).expect("const") && last.pwm.get() == 0 {
            return Err(CoreError::invariant(
                "FanCurve.points",
                "нулевой вентилятор на критической температуре (>= 80 °C)",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: u8) -> Percent {
        Percent::new(v).unwrap()
    }

    fn t(v: i16) -> TemperatureC {
        TemperatureC::new(v).unwrap()
    }

    fn curve(points: Vec<(i16, u8)>) -> FanCurve {
        FanCurve {
            profile: PerformanceProfile::Balanced,
            fan: FanId::Cpu,
            points: points
                .into_iter()
                .map(|(t_, p_)| FanCurvePoint::new(t(t_), p(p_)))
                .collect(),
        }
    }

    #[test]
    fn valid_curve() {
        let c = curve(vec![(50, 0), (60, 20), (70, 50), (85, 100)]);
        assert!(c.validate(8, false).is_ok());
    }

    #[test]
    fn empty_rejected() {
        let c = curve(vec![]);
        assert!(c.validate(8, false).is_err());
    }

    #[test]
    fn too_many_points_rejected() {
        let pts = (0..10).map(|i| (50 + i as i16, 10)).collect::<Vec<_>>();
        let c = curve(pts);
        assert!(c.validate(8, false).is_err());
    }

    #[test]
    fn decreasing_temps_rejected() {
        let c = curve(vec![(60, 10), (50, 10)]);
        assert!(c.validate(8, false).is_err());
    }

    #[test]
    fn decreasing_pwm_rejected_by_default() {
        let c = curve(vec![(50, 50), (60, 10)]);
        assert!(c.validate(8, false).is_err());
        // backend может разрешить убывание
        assert!(c.validate(8, true).is_ok());
    }

    #[test]
    fn zero_fan_at_high_temp_rejected() {
        let c = curve(vec![(50, 0), (85, 0)]);
        assert!(c.validate(8, false).is_err());
    }

    #[test]
    fn zero_fan_below_critical_ok() {
        let c = curve(vec![(50, 0), (79, 0)]);
        assert!(c.validate(8, false).is_ok());
    }
}
