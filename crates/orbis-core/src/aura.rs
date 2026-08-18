//! Aura RGB domain types.
//!
//! Wire contract mirrors upstream asusctl `rog_aura` types 1:1:
//! - `AuraModeNum` → [`AuraMode`] (u32 on D-Bus);
//! - `AuraZone` → [`AuraZone`] (u32 on D-Bus);
//! - `LedBrightness` → [`AuraBrightness`] (u32 on D-Bus);
//! - `Speed` → [`AuraSpeed`] (string on D-Bus);
//! - `Direction` → [`AuraDirection`] (string on D-Bus);
//! - `Colour` → [`AuraRgb`] (struct of three u8 on D-Bus);
//! - `AuraEffect` → [`AuraEffect`] (struct on D-Bus).
//!
//! Unknown wire values are preserved as `Unknown`, never coerced to a known
//! state. Upstream `From<u8> for AuraModeNum` coerces unknown values to
//! `Static`; this domain deliberately does not.

use serde::{Deserialize, Serialize};

/// RGB colour, one byte per channel (lossless).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AuraRgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

/// Aura mode, 1:1 with upstream `AuraModeNum` wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum AuraMode {
    /// Static single colour.
    Static = 0,
    /// Breathing.
    Breathe = 1,
    /// Rainbow cycle.
    RainbowCycle = 2,
    /// Rainbow wave.
    RainbowWave = 3,
    /// Stars.
    Star = 4,
    /// Rain.
    Rain = 5,
    /// Highlight.
    Highlight = 6,
    /// Laser.
    Laser = 7,
    /// Ripple.
    Ripple = 8,
    /// Pulse.
    Pulse = 10,
    /// Comet.
    Comet = 11,
    /// Flash.
    Flash = 12,
    /// Unknown wire value, preserved (never coerced to a known mode).
    Unknown(u32),
}

impl AuraMode {
    /// Decode a wire u32 without coercing unknown values.
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Static,
            1 => Self::Breathe,
            2 => Self::RainbowCycle,
            3 => Self::RainbowWave,
            4 => Self::Star,
            5 => Self::Rain,
            6 => Self::Highlight,
            7 => Self::Laser,
            8 => Self::Ripple,
            10 => Self::Pulse,
            11 => Self::Comet,
            12 => Self::Flash,
            other => Self::Unknown(other),
        }
    }

    /// Encode back to the wire u32.
    pub fn to_u32(self) -> u32 {
        match self {
            Self::Static => 0,
            Self::Breathe => 1,
            Self::RainbowCycle => 2,
            Self::RainbowWave => 3,
            Self::Star => 4,
            Self::Rain => 5,
            Self::Highlight => 6,
            Self::Laser => 7,
            Self::Ripple => 8,
            Self::Pulse => 10,
            Self::Comet => 11,
            Self::Flash => 12,
            Self::Unknown(other) => other,
        }
    }
}

/// Aura zone, 1:1 with upstream `AuraZone` wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum AuraZone {
    /// No zone (zoneless keyboard, or setting all).
    None = 0,
    /// Leftmost zone.
    Key1 = 1,
    /// Zone after leftmost.
    Key2 = 2,
    /// Zone second from right.
    Key3 = 3,
    /// Rightmost zone.
    Key4 = 4,
    /// Logo on the lid.
    Logo = 5,
    /// Left part of a lightbar.
    BarLeft = 6,
    /// Right part of a lightbar.
    BarRight = 7,
    /// Unknown wire value, preserved.
    Unknown(u32),
}

impl AuraZone {
    /// Decode a wire u32 without coercing unknown values.
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Key1,
            2 => Self::Key2,
            3 => Self::Key3,
            4 => Self::Key4,
            5 => Self::Logo,
            6 => Self::BarLeft,
            7 => Self::BarRight,
            other => Self::Unknown(other),
        }
    }

    /// Encode back to the wire u32.
    pub fn to_u32(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Key1 => 1,
            Self::Key2 => 2,
            Self::Key3 => 3,
            Self::Key4 => 4,
            Self::Logo => 5,
            Self::BarLeft => 6,
            Self::BarRight => 7,
            Self::Unknown(other) => other,
        }
    }
}

/// Aura brightness level, 1:1 with upstream `LedBrightness` wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum AuraBrightness {
    /// Off.
    Off = 0,
    /// Low.
    Low = 1,
    /// Medium.
    Med = 2,
    /// High.
    High = 3,
    /// Unknown wire value, preserved.
    Unknown(u32),
}

impl AuraBrightness {
    /// Decode a wire u32 without coercing unknown values.
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::Low,
            2 => Self::Med,
            3 => Self::High,
            other => Self::Unknown(other),
        }
    }

    /// Encode back to the wire u32.
    pub fn to_u32(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Low => 1,
            Self::Med => 2,
            Self::High => 3,
            Self::Unknown(other) => other,
        }
    }
}

/// Aura effect speed, 1:1 with upstream `Speed` wire values (string on D-Bus).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraSpeed {
    /// Low.
    Low,
    /// Medium.
    Med,
    /// High.
    High,
    /// Unknown wire value, preserved.
    Unknown(String),
}

impl AuraSpeed {
    /// Decode a wire string without coercing unknown values.
    pub fn from_str(value: &str) -> Self {
        match value {
            "Low" => Self::Low,
            "Med" => Self::Med,
            "High" => Self::High,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Encode back to the wire string (lossless for `Unknown`).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Low => "Low",
            Self::Med => "Med",
            Self::High => "High",
            Self::Unknown(other) => other.as_str(),
        }
    }
}

/// Aura effect direction, 1:1 with upstream `Direction` wire values (string on
/// D-Bus).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraDirection {
    /// Right.
    Right,
    /// Left.
    Left,
    /// Up.
    Up,
    /// Down.
    Down,
    /// Unknown wire value, preserved.
    Unknown(String),
}

impl AuraDirection {
    /// Decode a wire string without coercing unknown values.
    pub fn from_str(value: &str) -> Self {
        match value {
            "Right" => Self::Right,
            "Left" => Self::Left,
            "Up" => Self::Up,
            "Down" => Self::Down,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Encode back to the wire string (lossless for `Unknown`).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Right => "Right",
            Self::Left => "Left",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Unknown(other) => other.as_str(),
        }
    }
}

/// Current Aura effect, 1:1 with upstream `AuraEffect` (struct on D-Bus).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AuraEffect {
    /// Effect mode.
    pub mode: AuraMode,
    /// Effect zone (`AuraZone::None` for zoneless keyboards).
    pub zone: AuraZone,
    /// Primary colour (used by all modes).
    pub colour1: AuraRgb,
    /// Secondary colour (used by some modes).
    pub colour2: AuraRgb,
    /// Effect speed.
    pub speed: AuraSpeed,
    /// Effect direction.
    pub direction: AuraDirection,
}

/// Read-only Aura state snapshot (fresh authoritative read).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraState {
    /// Current mode (`led_mode`).
    pub current_mode: AuraMode,
    /// Current effect data (`led_mode_data`), includes the primary colour.
    pub current_effect: AuraEffect,
    /// Current brightness level (`brightness`).
    pub brightness: AuraBrightness,
    /// Supported basic modes (`supported_basic_modes`).
    pub supported_modes: Vec<AuraMode>,
    /// Supported basic zones (`supported_basic_zones`); empty is a valid
    /// single-zone TUF state.
    pub supported_zones: Vec<AuraZone>,
    /// Supported brightness levels (`supported_brightness`).
    pub supported_brightness: Vec<AuraBrightness>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_wire_roundtrip() {
        for value in [0u32, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12] {
            let mode = AuraMode::from_u32(value);
            assert_eq!(mode.to_u32(), value);
            assert!(!matches!(mode, AuraMode::Unknown(_)));
        }
    }

    #[test]
    fn unknown_mode_preserved_not_coerced() {
        let mode = AuraMode::from_u32(42);
        assert_eq!(mode, AuraMode::Unknown(42));
        assert_eq!(mode.to_u32(), 42);
    }

    #[test]
    fn zone_wire_roundtrip() {
        for value in [0u32, 1, 2, 3, 4, 5, 6, 7] {
            let zone = AuraZone::from_u32(value);
            assert_eq!(zone.to_u32(), value);
            assert!(!matches!(zone, AuraZone::Unknown(_)));
        }
    }

    #[test]
    fn brightness_wire_roundtrip() {
        for value in [0u32, 1, 2, 3] {
            let brightness = AuraBrightness::from_u32(value);
            assert_eq!(brightness.to_u32(), value);
            assert!(!matches!(brightness, AuraBrightness::Unknown(_)));
        }
    }

    #[test]
    fn speed_and_direction_strings() {
        assert_eq!(AuraSpeed::from_str("Low"), AuraSpeed::Low);
        assert_eq!(AuraSpeed::from_str("Med"), AuraSpeed::Med);
        assert_eq!(AuraSpeed::from_str("High"), AuraSpeed::High);
        assert_eq!(
            AuraSpeed::from_str("Turbo"),
            AuraSpeed::Unknown("Turbo".to_string())
        );
        assert_eq!(AuraDirection::from_str("Right"), AuraDirection::Right);
        assert_eq!(AuraDirection::from_str("Left"), AuraDirection::Left);
        assert_eq!(AuraDirection::from_str("Up"), AuraDirection::Up);
        assert_eq!(AuraDirection::from_str("Down"), AuraDirection::Down);
        assert_eq!(
            AuraDirection::from_str("Diagonal"),
            AuraDirection::Unknown("Diagonal".to_string())
        );
    }

    #[test]
    fn speed_and_direction_wire_roundtrip_is_lossless() {
        for speed in [
            AuraSpeed::Low,
            AuraSpeed::Med,
            AuraSpeed::High,
            AuraSpeed::Unknown("Turbo".to_string()),
        ] {
            assert_eq!(AuraSpeed::from_str(speed.as_str()), speed);
        }
        for direction in [
            AuraDirection::Right,
            AuraDirection::Left,
            AuraDirection::Up,
            AuraDirection::Down,
            AuraDirection::Unknown("Diagonal".to_string()),
        ] {
            assert_eq!(AuraDirection::from_str(direction.as_str()), direction);
        }
    }

    #[test]
    fn serde_roundtrip_preserves_unknown() {
        let state = AuraState {
            current_mode: AuraMode::Unknown(42),
            current_effect: AuraEffect {
                mode: AuraMode::Static,
                zone: AuraZone::None,
                colour1: AuraRgb {
                    r: 0xff,
                    g: 0x11,
                    b: 0xdd,
                },
                colour2: AuraRgb { r: 0, g: 0, b: 0 },
                speed: AuraSpeed::Med,
                direction: AuraDirection::Right,
            },
            brightness: AuraBrightness::High,
            supported_modes: vec![AuraMode::Static],
            supported_zones: vec![],
            supported_brightness: vec![
                AuraBrightness::Off,
                AuraBrightness::Low,
                AuraBrightness::Med,
                AuraBrightness::High,
            ],
        };
        let json = serde_json::to_string(&state).unwrap();
        let decoded: AuraState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }
}
