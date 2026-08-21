//! Versioned preset import/export.
//!
//! Imported bundles are untrusted configuration. Parsing and validation only
//! produce inert preset data; loading a bundle never applies hardware state.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Preset;

/// Current preset bundle schema.
pub const PRESET_BUNDLE_SCHEMA_VERSION: u32 = 1;

/// Portable versioned preset bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetBundle {
    /// Schema version.
    pub schema_version: u32,
    /// Presets carried by the bundle.
    pub presets: Vec<Preset>,
}

/// Preset import/validation error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PresetBundleError {
    /// JSON could not be decoded.
    #[error("invalid preset bundle json: {0}")]
    InvalidJson(String),
    /// Bundle version is not supported.
    #[error("unsupported preset bundle schema version {0}")]
    UnsupportedVersion(u32),
    /// Preset id must not be empty.
    #[error("preset id must not be empty")]
    EmptyId,
    /// Preset name must not be empty.
    #[error("preset name must not be empty for id {0}")]
    EmptyName(String),
    /// Duplicate preset id.
    #[error("duplicate preset id: {0}")]
    DuplicateId(String),
}

impl PresetBundle {
    /// Construct and validate a current-version bundle.
    pub fn new(presets: Vec<Preset>) -> Result<Self, PresetBundleError> {
        let bundle = Self {
            schema_version: PRESET_BUNDLE_SCHEMA_VERSION,
            presets,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    /// Validate schema and identity invariants.
    pub fn validate(&self) -> Result<(), PresetBundleError> {
        if self.schema_version != PRESET_BUNDLE_SCHEMA_VERSION {
            return Err(PresetBundleError::UnsupportedVersion(self.schema_version));
        }

        let mut ids = BTreeSet::new();
        for preset in &self.presets {
            if preset.id.trim().is_empty() {
                return Err(PresetBundleError::EmptyId);
            }
            if preset.name.trim().is_empty() {
                return Err(PresetBundleError::EmptyName(preset.id.clone()));
            }
            if !ids.insert(preset.id.as_str()) {
                return Err(PresetBundleError::DuplicateId(preset.id.clone()));
            }
        }
        Ok(())
    }

    /// Parse and validate untrusted JSON.
    pub fn from_json(json: &str) -> Result<Self, PresetBundleError> {
        let bundle: Self =
            serde_json::from_str(json).map_err(|error| PresetBundleError::InvalidJson(error.to_string()))?;
        bundle.validate()?;
        Ok(bundle)
    }

    /// Export a validated bundle as pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, PresetBundleError> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|error| PresetBundleError::InvalidJson(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PerformanceProfile, PresetIntent};

    fn preset(id: &str) -> Preset {
        Preset {
            id: id.into(),
            name: id.into(),
            intent: PresetIntent {
                performance: Some(PerformanceProfile::Balanced),
                ..Default::default()
            },
        }
    }

    #[test]
    fn roundtrip_preserves_versioned_bundle() {
        let bundle = PresetBundle::new(vec![preset("balanced")]).unwrap();
        let json = bundle.to_json_pretty().unwrap();
        let back = PresetBundle::from_json(&json).unwrap();
        assert_eq!(back, bundle);
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        assert_eq!(
            PresetBundle::new(vec![preset("same"), preset("same")]).unwrap_err(),
            PresetBundleError::DuplicateId("same".into())
        );
    }

    #[test]
    fn future_version_is_rejected_fail_closed() {
        let json = r#"{"schema_version":999,"presets":[]}"#;
        assert_eq!(
            PresetBundle::from_json(json).unwrap_err(),
            PresetBundleError::UnsupportedVersion(999)
        );
    }

    #[test]
    fn parsing_does_not_apply_any_intent() {
        let json = r#"{
          "schema_version": 1,
          "presets": [{
            "id": "performance",
            "name": "Performance",
            "intent": {
              "performance": "turbo",
              "gpu_mode": null,
              "charge_limit": null,
              "display_refresh": null,
              "fan_profile": null
            }
          }]
        }"#;
        let bundle = PresetBundle::from_json(json).unwrap();
        assert_eq!(bundle.presets.len(), 1);
    }
}
