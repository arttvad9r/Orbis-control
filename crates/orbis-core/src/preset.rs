//! Global preset intent.
//!
//! Presets describe desired policy only. Loading or selecting a preset must not
//! itself perform hardware I/O; reconciliation remains responsible for
//! capability checks, mutation and read-back.

use serde::{Deserialize, Serialize};

use crate::{AsusdFanProfile, GpuMode, Percent, PerformanceProfile, PowerSource, RefreshHz};

/// Desired values grouped into one reusable preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetIntent {
    /// Desired performance profile.
    pub performance: Option<PerformanceProfile>,
    /// Desired product GPU policy. Presence is intent only; it does not prove
    /// that a production GPU product-policy backend exists.
    pub gpu_mode: Option<GpuMode>,
    /// Desired battery charge limit.
    pub charge_limit: Option<Percent>,
    /// Desired display refresh rate.
    pub display_refresh: Option<RefreshHz>,
    /// Desired ASUS fan profile identity.
    pub fan_profile: Option<AsusdFanProfile>,
}

/// Reusable global preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    /// Stable user/config identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Desired policy values carried by the preset.
    pub intent: PresetIntent,
}

/// Power-source mapping for automatic preset selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PowerPresetPolicy {
    /// Preset id selected while on AC.
    pub on_ac: Option<String>,
    /// Preset id selected while on battery.
    pub on_battery: Option<String>,
    /// Preset id selected for low-power USB-C PD.
    pub on_usb_c_low_power: Option<String>,
}

impl PowerPresetPolicy {
    /// Resolve a preset id for one observed power source.
    ///
    /// `Unknown` never falls back optimistically to AC or battery.
    pub fn resolve(&self, source: PowerSource) -> Option<&str> {
        match source {
            PowerSource::Ac => self.on_ac.as_deref(),
            PowerSource::Battery => self.on_battery.as_deref(),
            PowerSource::UsbCPdLowPower => self.on_usb_c_low_power.as_deref(),
            PowerSource::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_roundtrip_keeps_intent_without_applying_it() {
        let preset = Preset {
            id: "gaming".into(),
            name: "Gaming".into(),
            intent: PresetIntent {
                performance: Some(PerformanceProfile::Turbo),
                gpu_mode: Some(GpuMode::Standard),
                charge_limit: Some(Percent::new(80).unwrap()),
                display_refresh: Some(RefreshHz::new(165).unwrap()),
                fan_profile: Some(AsusdFanProfile::Performance),
            },
        };
        let json = serde_json::to_string(&preset).unwrap();
        let back: Preset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, preset);
    }

    #[test]
    fn unknown_power_source_does_not_guess_a_preset() {
        let policy = PowerPresetPolicy {
            on_ac: Some("performance".into()),
            on_battery: Some("silent".into()),
            on_usb_c_low_power: Some("silent".into()),
        };
        assert_eq!(policy.resolve(PowerSource::Unknown), None);
        assert_eq!(policy.resolve(PowerSource::Battery), Some("silent"));
    }
}
