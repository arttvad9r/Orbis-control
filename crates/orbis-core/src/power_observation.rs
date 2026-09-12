//! Read-only power/thermal observations with optional backend metadata.

use serde::{Deserialize, Serialize};

use crate::limits::{PowerLimitField, Unit};

/// One authoritative read-only power/thermal observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerLimitObservation {
    /// Stable domain field identifier.
    pub field: PowerLimitField,
    /// Authoritative current value.
    pub value: i32,
    /// Backend-reported unit.
    pub unit: Unit,
    /// Authoritative minimum, when published by the backend.
    pub min: Option<i32>,
    /// Authoritative maximum, when published by the backend.
    pub max: Option<i32>,
    /// Authoritative step, when published by the backend.
    pub step: Option<i32>,
    /// Authoritative default, when published by the backend.
    pub default: Option<i32>,
}

impl PowerLimitObservation {
    /// Create an observation whose metadata is not available.
    pub const fn without_metadata(field: PowerLimitField, value: i32, unit: Unit) -> Self {
        Self {
            field,
            value,
            unit,
            min: None,
            max: None,
            step: None,
            default: None,
        }
    }

    /// Whether the backend supplied a complete editable range.
    pub const fn has_editable_metadata(&self) -> bool {
        self.min.is_some() && self.max.is_some() && self.step.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_metadata_is_not_editable() {
        let observation =
            PowerLimitObservation::without_metadata(PowerLimitField::Spl, 5, Unit::Watts);
        assert_eq!(observation.value, 5);
        assert!(!observation.has_editable_metadata());
    }
}
