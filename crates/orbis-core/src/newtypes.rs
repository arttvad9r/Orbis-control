//! Строгие newtype для физических величин.
//!
//! Значения, для которых важна точность, не используют `f32`: проценты,
//! температуры, RPM, мощности и частоты хранятся в целочисленных newtype
//! с валидацией диапазонов в конструкторах.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

macro_rules! range_newtype {
    ($(#[$doc:meta])* $name:ident, $inner:ty, $min:expr, $max:expr, $unit:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name($inner);

        impl $name {
            /// Минимальное допустимое значение.
            pub const MIN: $inner = $min;
            /// Максимальное допустимое значение.
            pub const MAX: $inner = $max;

            /// Конструктор с валидацией диапазона.
            #[allow(unused_comparisons)] // для типов, покрывающих весь диапазон (FanPwm/u8)
            pub fn new(value: $inner) -> std::result::Result<Self, CoreError> {
                if value < $min || value > $max {
                    return Err(CoreError::out_of_range(stringify!($name), value, $min, $max));
                }
                Ok(Self(value))
            }

            /// Доступ к сырому значению.
            pub fn get(self) -> $inner {
                self.0
            }
        }

        impl From<$name> for $inner {
            fn from(v: $name) -> $inner {
                v.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", self.0, $unit)
            }
        }
    };
}

range_newtype!(
    /// Процент, 0..=100.
    Percent, u8, 0, 100, "%");

range_newtype!(
    /// Температура в градусах Цельсия.
    TemperatureC, i16, -50, 150, "°C");

range_newtype!(
    /// Скорость вращения вентилятора, RPM.
    Rpm, u16, 0, 65535, " rpm");

range_newtype!(
    /// Мощность в ваттах (целые ватты; для power limits).
    PowerW, u32, 0, 1000, " W");

range_newtype!(
    /// Мощность в милливаттах (точная телеметрия).
    MilliWatt, u32, 0, 10_000_000, " mW");

range_newtype!(
    /// Энергия в милливатт-часах.
    EnergyMWh, u64, 0, 1_000_000_000, " mWh");

range_newtype!(
    /// Частота обновления экрана, Гц.
    RefreshHz, u32, 1, 1000, " Hz");

range_newtype!(
    /// Значение ШИМ вентилятора в шкале hwmon 0..=255.
    FanPwm, u8, 0, 255, "");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_roundtrip() {
        assert_eq!(Percent::new(50).unwrap().get(), 50);
        assert_eq!(Percent::new(0).unwrap().get(), 0);
        assert_eq!(Percent::new(100).unwrap().get(), 100);
    }

    #[test]
    fn percent_rejects_out_of_range() {
        assert!(Percent::new(101).is_err());
        assert!(Percent::new(255).is_err());
    }

    #[test]
    fn temperature_range() {
        assert!(TemperatureC::new(-50).is_ok());
        assert!(TemperatureC::new(150).is_ok());
        assert!(TemperatureC::new(-51).is_err());
        assert!(TemperatureC::new(151).is_err());
    }

    #[test]
    fn fan_pwm_range() {
        assert_eq!(FanPwm::new(0).unwrap().get(), 0);
        assert_eq!(FanPwm::new(255).unwrap().get(), 255);
        // все значения u8 допустимы (0..=255), но вызов с недопустимым типом невозможен —
        // валидацию диапазона покрывают Percent/TemperatureC тесты выше.
    }

    #[test]
    fn display_hz() {
        assert_eq!(RefreshHz::new(120).unwrap().to_string(), "120 Hz");
        assert!(RefreshHz::new(0).is_err());
    }

    #[test]
    fn display_format() {
        assert_eq!(TemperatureC::new(85).unwrap().to_string(), "85°C");
        assert_eq!(Rpm::new(2700).unwrap().to_string(), "2700 rpm");
    }

    #[test]
    fn ordering_works() {
        assert!(Percent::new(10).unwrap() < Percent::new(90).unwrap());
        assert!(TemperatureC::new(45).unwrap() <= TemperatureC::new(45).unwrap());
    }
}
