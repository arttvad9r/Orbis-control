//! GPU: три независимые сущности (физический MUX / доступ приложений /
//! фактический power state). См. ADR 0003.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Четырёхкнопочная модель GPU (G-Helper): Eco, Standard, Ultimate, Optimized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMode {
    /// Eco: минимум энергопотребления, dGPU недоступна приложениям.
    Eco,
    /// Standard: гибридный режим (iGPU для desktop, dGPU по запросу).
    Standard,
    /// Ultimate: физический MUX направлен на dGPU (обычно требует reboot).
    Ultimate,
    /// Optimized: политика sessiond (battery -> Eco, AC -> Standard).
    Optimized,
}

impl GpuMode {
    /// Все режимы.
    pub const ALL: [GpuMode; 4] =
        [GpuMode::Eco, GpuMode::Standard, GpuMode::Ultimate, GpuMode::Optimized];

    /// Разбор из строки CLI/конфига.
    pub fn parse(s: &str) -> std::result::Result<Self, CoreError> {
        match s.to_ascii_lowercase().as_str() {
            "eco" | "integrated" => Ok(Self::Eco),
            "standard" | "hybrid" => Ok(Self::Standard),
            "ultimate" | "dedicated" | "discrete" => Ok(Self::Ultimate),
            "optimized" | "auto" => Ok(Self::Optimized),
            other => Err(CoreError::parse("GpuMode", other)),
        }
    }

    /// Каноническое имя для CLI/конфига.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eco => "eco",
            Self::Standard => "standard",
            Self::Ultimate => "ultimate",
            Self::Optimized => "optimized",
        }
    }
}

impl std::str::FromStr for GpuMode {
    type Err = CoreError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Физическое состояние MUX (какой GPU обслуживает внутренний дисплей).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMuxState {
    /// MUX направлен на iGPU.
    Integrated,
    /// MUX направлен на dGPU.
    Discrete,
    /// Состояние неизвестно/не определяется.
    Unknown,
}

/// Политика доступа приложений к dGPU (Cardwire-подобная блокировка).
///
/// Это НЕ физический MUX: блокировка лишь скрывает устройства от приложений.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuAccessPolicy {
    /// dGPU доступна приложениям.
    Unblocked,
    /// dGPU заблокирована для приложений (Cardwire / dgpu_disable).
    Blocked,
    /// Переключение в процессе.
    Pending,
    /// Неизвестно.
    Unknown,
}

/// Фактический power state dGPU (чтение не должно будить GPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuPowerState {
    /// D0 — активна.
    Active,
    /// Низкопотребляющее состояние (D3cold и т.п.).
    Suspended,
    /// Выключена/не обнаруживается.
    Off,
    /// Последнее известное значение устарело (GPU спал при чтении).
    Stale,
    /// Неизвестно.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_mode_parse() {
        assert_eq!(GpuMode::parse("eco").unwrap(), GpuMode::Eco);
        assert_eq!(GpuMode::parse("integrated").unwrap(), GpuMode::Eco);
        assert_eq!(GpuMode::parse("standard").unwrap(), GpuMode::Standard);
        assert_eq!(GpuMode::parse("ultimate").unwrap(), GpuMode::Ultimate);
        assert_eq!(GpuMode::parse("optimized").unwrap(), GpuMode::Optimized);
        assert!(GpuMode::parse("xyz").is_err());
    }

    #[test]
    fn gpu_mode_roundtrip() {
        for m in GpuMode::ALL {
            assert_eq!(GpuMode::parse(m.as_str()).unwrap(), m);
        }
    }

    #[test]
    fn serde_snake_case() {
        let s = serde_json::to_string(&GpuMode::Ultimate).unwrap();
        assert_eq!(s, "\"ultimate\"");
    }
}
