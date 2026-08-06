//! Источник питания.

use serde::{Deserialize, Serialize};

/// Источник питания, определяемый по UPower/типам устройств.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    /// Сеть переменного тока.
    Ac,
    /// Батарея.
    Battery,
    /// USB-C зарядное с пониженной мощностью (low-power PD).
    UsbCPdLowPower,
    /// Неизвестно.
    Unknown,
}

impl PowerSource {
    /// Разбор из строки (для тестов и CLI).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ac" | "line" | "mains" => Some(Self::Ac),
            "battery" => Some(Self::Battery),
            "usbc" | "usb-c" | "usb-c-pd-low" | "low-power" => Some(Self::UsbCPdLowPower),
            _ => None,
        }
    }

    /// Каноническое имя.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ac => "ac",
            Self::Battery => "battery",
            Self::UsbCPdLowPower => "usb-c-pd-low",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for PowerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        assert_eq!(PowerSource::parse("AC").unwrap(), PowerSource::Ac);
        assert_eq!(PowerSource::parse("battery").unwrap(), PowerSource::Battery);
        assert_eq!(
            PowerSource::parse("usb-c").unwrap(),
            PowerSource::UsbCPdLowPower
        );
        assert!(PowerSource::parse("whatever").is_none());
    }
}
