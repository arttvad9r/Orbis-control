//! Режимы производительности.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Трёхкнопочная модель G-Helper (модель UI).
///
/// Внутренние имена: Silent, Balanced, Turbo. Отображение на platform-профили
/// системы выполняется через [`PlatformProfile`] и зависит от backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceProfile {
    /// Silent — тихий режим (quiet/low-power).
    Silent,
    /// Balanced — сбалансированный.
    Balanced,
    /// Turbo — производительный (performance).
    Turbo,
}

impl PerformanceProfile {
    /// Все стандартные профили.
    pub const ALL: [PerformanceProfile; 3] = [
        PerformanceProfile::Silent,
        PerformanceProfile::Balanced,
        PerformanceProfile::Turbo,
    ];

    /// Разбор из строки CLI/конфига.
    pub fn parse(s: &str) -> std::result::Result<Self, CoreError> {
        match s.to_ascii_lowercase().as_str() {
            "silent" | "quiet" | "low-power" => Ok(Self::Silent),
            "balanced" => Ok(Self::Balanced),
            "turbo" | "performance" => Ok(Self::Turbo),
            other => Err(CoreError::parse("PerformanceProfile", other)),
        }
    }

    /// Каноническое имя для CLI/конфига.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Balanced => "balanced",
            Self::Turbo => "turbo",
        }
    }
}

impl std::str::FromStr for PerformanceProfile {
    type Err = CoreError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Профиль на уровне backend/ядра (`platform_profile`).
///
/// Включает профили, которые могут существовать в системе помимо трёх кнопок
/// (например, `low-power` на некоторых моделях), и пользовательские.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformProfile {
    /// low-power (иногда отсутствует)
    LowPower,
    /// quiet
    Quiet,
    /// balanced
    Balanced,
    /// performance
    Performance,
    /// пользовательский профиль (asusd custom)
    Custom(String),
}

impl PlatformProfile {
    /// Строковое имя для `platform_profile` sysfs / asusd.
    pub fn as_str(&self) -> String {
        match self {
            Self::LowPower => "low-power".to_string(),
            Self::Quiet => "quiet".to_string(),
            Self::Balanced => "balanced".to_string(),
            Self::Performance => "performance".to_string(),
            Self::Custom(s) => s.clone(),
        }
    }
}

impl std::fmt::Display for PlatformProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Сопоставление трёхкнопочной модели с platform-профилем.
impl From<PerformanceProfile> for PlatformProfile {
    fn from(p: PerformanceProfile) -> Self {
        match p {
            PerformanceProfile::Silent => PlatformProfile::Quiet,
            PerformanceProfile::Balanced => PlatformProfile::Balanced,
            PerformanceProfile::Turbo => PlatformProfile::Performance,
        }
    }
}

/// Попытка обратного сопоставления (не для Custom).
impl TryFrom<&PlatformProfile> for PerformanceProfile {
    type Error = CoreError;

    fn try_from(p: &PlatformProfile) -> std::result::Result<Self, Self::Error> {
        match p {
            PlatformProfile::Quiet | PlatformProfile::LowPower => Ok(Self::Silent),
            PlatformProfile::Balanced => Ok(Self::Balanced),
            PlatformProfile::Performance => Ok(Self::Turbo),
            PlatformProfile::Custom(_) => Err(CoreError::invariant(
                "PlatformProfile",
                "нельзя отобразить пользовательский профиль на трёхкнопочную модель",
            )),
        }
    }
}

/// Lossless asusd fan profile ID (wire 0..3).
///
/// Используется для fan curve mutation API: `Quiet` (wire 2) и `LowPower`
/// (wire 3) остаются различимыми, в отличие от трёхкнопочной
/// `PerformanceProfile`, где оба отображаются на `Silent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsusdFanProfile {
    /// wire 0 — balanced.
    Balanced,
    /// wire 1 — performance.
    Performance,
    /// wire 2 — quiet.
    Quiet,
    /// wire 3 — low-power.
    LowPower,
}

impl AsusdFanProfile {
    /// Wire value для asusd `FanCurveData`/`SetFanCurve`.
    pub fn wire(self) -> u32 {
        match self {
            Self::Balanced => 0,
            Self::Performance => 1,
            Self::Quiet => 2,
            Self::LowPower => 3,
        }
    }

    /// Строковое имя (для диагностики/сравнения с `fan_curves.ron`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Quiet => "quiet",
            Self::LowPower => "low-power",
        }
    }
}

/// Сопоставление asusd fan profile с трёхкнопочной `PerformanceProfile`.
///
/// Read-only mapping (asusd → трёхкнопочная модель). `LowPower` и `Quiet`
/// оба → `Silent`, но сам `AsusdFanProfile` остаётся lossless.
impl From<AsusdFanProfile> for PerformanceProfile {
    fn from(p: AsusdFanProfile) -> Self {
        match p {
            AsusdFanProfile::Balanced => PerformanceProfile::Balanced,
            AsusdFanProfile::Performance => PerformanceProfile::Turbo,
            AsusdFanProfile::Quiet | AsusdFanProfile::LowPower => PerformanceProfile::Silent,
        }
    }
}

/// Обратное сопоставление трёхкнопочной модели с asusd fan profile.
///
/// **Fallible**: `Silent` неоднозначен (Quiet vs LowPower), поэтому
/// автоматический выбор запрещён. Fan mutation API должен использовать
/// lossless `AsusdFanProfile` напрямую.
impl TryFrom<PerformanceProfile> for AsusdFanProfile {
    type Error = CoreError;

    fn try_from(p: PerformanceProfile) -> std::result::Result<Self, Self::Error> {
        match p {
            PerformanceProfile::Balanced => Ok(AsusdFanProfile::Balanced),
            PerformanceProfile::Turbo => Ok(AsusdFanProfile::Performance),
            PerformanceProfile::Silent => Err(CoreError::invariant(
                "AsusdFanProfile",
                "PerformanceProfile::Silent неоднозначен: выберите Quiet или LowPower явно",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        assert_eq!(
            PerformanceProfile::parse("silent").unwrap(),
            PerformanceProfile::Silent
        );
        assert_eq!(
            PerformanceProfile::parse("QUIET").unwrap(),
            PerformanceProfile::Silent
        );
        assert_eq!(
            PerformanceProfile::parse("balanced").unwrap(),
            PerformanceProfile::Balanced
        );
        assert_eq!(
            PerformanceProfile::parse("turbo").unwrap(),
            PerformanceProfile::Turbo
        );
        assert_eq!(
            PerformanceProfile::parse("performance").unwrap(),
            PerformanceProfile::Turbo
        );
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(PerformanceProfile::parse("gaming").is_err());
    }

    #[test]
    fn roundtrip_str() {
        for p in PerformanceProfile::ALL {
            assert_eq!(PerformanceProfile::parse(p.as_str()).unwrap(), p);
        }
    }

    #[test]
    fn mapping_to_platform() {
        assert_eq!(
            PlatformProfile::from(PerformanceProfile::Silent).as_str(),
            "quiet"
        );
        assert_eq!(
            PlatformProfile::from(PerformanceProfile::Turbo).as_str(),
            "performance"
        );
    }

    #[test]
    fn reverse_mapping() {
        assert_eq!(
            PerformanceProfile::try_from(&PlatformProfile::LowPower).unwrap(),
            PerformanceProfile::Silent
        );
        assert!(PerformanceProfile::try_from(&PlatformProfile::Custom("x".into())).is_err());
    }
}
