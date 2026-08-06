//! Power limits (SPL/SPPT/FPPT, GPU boost, температуры).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Единица измерения параметра.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Ватты.
    Watts,
    /// Градусы Цельсия.
    DegreesC,
    /// Проценты.
    Percent,
    /// Безразмерный счётчик.
    Count,
    /// Неизвестная единица.
    Unknown,
}

/// Поле power limit, определяемое динамически от backend.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerLimitField {
    /// SPL (PPT PL1).
    Spl,
    /// SPPT (PPT PL2).
    Sppt,
    /// FPPT (PPT PL3).
    Fppt,
    /// CPU temperature limit.
    CpuTempLimit,
    /// NVIDIA Dynamic Boost.
    GpuDynamicBoost,
    /// GPU temperature target.
    GpuTempTarget,
    /// Прочий параметр.
    Other(String),
}

/// Значение параметра с метаданными от backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerLimitValue {
    /// Текущее значение.
    pub value: i32,
    /// Минимум.
    pub min: i32,
    /// Максимум.
    pub max: i32,
    /// Шаг.
    pub step: i32,
    /// Значение по умолчанию (если известно).
    pub default: Option<i32>,
    /// Единица измерения.
    pub unit: Unit,
}

impl PowerLimitValue {
    /// Конструктор с валидацией диапазона.
    pub fn new(
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        default: Option<i32>,
        unit: Unit,
    ) -> std::result::Result<Self, CoreError> {
        if min > max {
            return Err(CoreError::invariant("PowerLimitValue.min", "min > max"));
        }
        if step <= 0 {
            return Err(CoreError::invariant("PowerLimitValue.step", "step <= 0"));
        }
        if value < min || value > max {
            return Err(CoreError::out_of_range(
                "PowerLimitValue.value",
                value,
                min,
                max,
            ));
        }
        if let Some(d) = default {
            if d < min || d > max {
                return Err(CoreError::invariant(
                    "PowerLimitValue.default",
                    format!("default {d} вне [{min}, {max}]"),
                ));
            }
        }
        Ok(Self {
            value,
            min,
            max,
            step,
            default,
            unit,
        })
    }

    /// Значение выровнено по шагу от минимума?
    pub fn is_aligned(&self) -> bool {
        (self.value - self.min) % self.step == 0
    }
}

/// Набор power limits для одного профиля.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerLimits {
    /// Поля по имени (порядок не важен).
    pub fields: BTreeMap<PowerLimitField, PowerLimitValue>,
}

impl PowerLimits {
    /// Получить поле по имени.
    pub fn get(&self, field: &PowerLimitField) -> Option<&PowerLimitValue> {
        self.fields.get(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_value() {
        let v = PowerLimitValue::new(45, 20, 80, 5, Some(45), Unit::Watts).unwrap();
        assert!(v.is_aligned());
    }

    #[test]
    fn out_of_range_rejected() {
        assert!(PowerLimitValue::new(90, 20, 80, 5, None, Unit::Watts).is_err());
        assert!(PowerLimitValue::new(45, 20, 80, 5, Some(90), Unit::Watts).is_err());
    }

    #[test]
    fn min_max_inverted_rejected() {
        assert!(PowerLimitValue::new(45, 80, 20, 5, None, Unit::Watts).is_err());
    }

    #[test]
    fn zero_step_rejected() {
        assert!(PowerLimitValue::new(45, 20, 80, 0, None, Unit::Watts).is_err());
    }

    #[test]
    fn misaligned_detected() {
        let v = PowerLimitValue::new(47, 20, 80, 5, None, Unit::Watts).unwrap();
        assert!(!v.is_aligned());
    }

    #[test]
    fn limits_map() {
        let mut fields = BTreeMap::new();
        fields.insert(
            PowerLimitField::Spl,
            PowerLimitValue::new(45, 20, 80, 5, None, Unit::Watts).unwrap(),
        );
        fields.insert(
            PowerLimitField::GpuTempTarget,
            PowerLimitValue::new(75, 60, 87, 1, None, Unit::DegreesC).unwrap(),
        );
        let limits = PowerLimits { fields };
        assert_eq!(limits.get(&PowerLimitField::Spl).unwrap().value, 45);
        assert!(limits.get(&PowerLimitField::Fppt).is_none());
    }
}
