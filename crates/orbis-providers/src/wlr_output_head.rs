//! Pure state model for `zwlr_output_head_v1` evidence.
//!
//! The future protocol adapter may translate compositor events into these typed
//! values. This module itself has no Wayland proxy types and no configuration
//! API, so it can be tested independently before `wayland-protocols-wlr` becomes
//! a direct dependency.
//!
//! Head identity and mode evidence are intentionally separate from role proof:
//! make/model/serial/physical-size can be correlated with independent DRM/KMS
//! evidence through `orbis_core::prove_display_target_identity`, while exact mode
//! data can later feed `DisplayRefreshEvidence`. A head name is retained only for
//! diagnostics and is never used to decide internal-vs-external role.

use std::collections::BTreeMap;

use orbis_core::display_output::DisplayMode;
use orbis_core::display_refresh::{
    DisplayRefreshEvidence, DisplayRefreshTargetId, DisplayRefreshTargetRole,
};
use orbis_core::display_refresh_identity::{
    CompositorDisplayHeadEvidence, DisplayPhysicalSizeMm, DisplaySinkIdentity,
};
use orbis_core::newtypes::RefreshMilliHz;

/// Opaque compositor-local mode identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WlrOutputModeKey(String);

impl WlrOutputModeKey {
    /// Construct from adapter-owned protocol object identity.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow diagnostic identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed event consumed by the pure head accumulator.
#[derive(Debug, Clone, PartialEq)]
pub enum WlrOutputHeadEvent {
    /// Human-readable compositor head name. Diagnostic only.
    Name(String),
    /// EDID-derived/manufacturer metadata supplied by compositor.
    Make(String),
    /// EDID-derived/model metadata supplied by compositor.
    Model(String),
    /// EDID-derived serial metadata supplied by compositor.
    SerialNumber(String),
    /// Physical dimensions in millimetres.
    PhysicalSize { width: u32, height: u32 },
    /// Whether the compositor currently enables this head.
    Enabled(bool),
    /// Advertised mode object and exact lossless refresh.
    Mode {
        key: WlrOutputModeKey,
        width: u32,
        height: u32,
        refresh_mhz: u32,
        preferred: bool,
    },
    /// Which advertised mode object is currently active.
    CurrentMode(Option<WlrOutputModeKey>),
    /// Mode object was removed/finished by compositor.
    ModeRemoved(WlrOutputModeKey),
}

/// Why a head cannot currently become DisplayRefresh evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WlrHeadEvidenceBlock {
    /// Head is disabled.
    Disabled,
    /// No current mode object is known.
    CurrentModeUnknown,
    /// Current mode refers to an object not present in the current mode set.
    CurrentModeNotAdvertised,
    /// Current mode has zero refresh and cannot represent a refresh target.
    CurrentRefreshUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WlrModeEvidence {
    mode: DisplayMode,
    preferred: bool,
}

/// Pure accumulated state for one output-management head.
#[derive(Debug, Clone, PartialEq)]
pub struct WlrOutputHeadState {
    target: DisplayRefreshTargetId,
    name: Option<String>,
    make: Option<String>,
    model: Option<String>,
    serial: Option<String>,
    physical_size: Option<DisplayPhysicalSizeMm>,
    enabled: bool,
    modes: BTreeMap<WlrOutputModeKey, WlrModeEvidence>,
    current_mode: Option<WlrOutputModeKey>,
}

impl WlrOutputHeadState {
    /// Construct state for one opaque compositor head target.
    pub fn new(target: DisplayRefreshTargetId) -> Self {
        Self {
            target,
            name: None,
            make: None,
            model: None,
            serial: None,
            physical_size: None,
            enabled: false,
            modes: BTreeMap::new(),
            current_mode: None,
        }
    }

    /// Apply one already-decoded compositor event.
    pub fn apply(&mut self, event: WlrOutputHeadEvent) {
        match event {
            WlrOutputHeadEvent::Name(value) => self.name = non_empty(value),
            WlrOutputHeadEvent::Make(value) => self.make = non_empty(value),
            WlrOutputHeadEvent::Model(value) => self.model = non_empty(value),
            WlrOutputHeadEvent::SerialNumber(value) => self.serial = non_empty(value),
            WlrOutputHeadEvent::PhysicalSize { width, height } => {
                self.physical_size = DisplayPhysicalSizeMm::new(width, height);
            }
            WlrOutputHeadEvent::Enabled(enabled) => self.enabled = enabled,
            WlrOutputHeadEvent::Mode {
                key,
                width,
                height,
                refresh_mhz,
                preferred,
            } => {
                self.modes.insert(
                    key,
                    WlrModeEvidence {
                        mode: DisplayMode::new(
                            width,
                            height,
                            RefreshMilliHz::new(refresh_mhz).expect("u32 refresh is representable"),
                        ),
                        preferred,
                    },
                );
            }
            WlrOutputHeadEvent::CurrentMode(key) => self.current_mode = key,
            WlrOutputHeadEvent::ModeRemoved(key) => {
                self.modes.remove(&key);
                if self.current_mode.as_ref() == Some(&key) {
                    self.current_mode = None;
                }
            }
        }
    }

    /// Diagnostic head name if compositor supplied one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Opaque target identity owned by the future protocol adapter.
    pub fn target(&self) -> &DisplayRefreshTargetId {
        &self.target
    }

    /// Whether compositor currently reports this head enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Build cross-source identity evidence. Incomplete make/model/serial yields
    /// `sink=None` and therefore cannot prove internal-panel role.
    pub fn identity_evidence(&self) -> CompositorDisplayHeadEvidence {
        let sink = match (&self.make, &self.model, &self.serial) {
            (Some(make), Some(model), Some(serial)) => {
                DisplaySinkIdentity::new(make.clone(), model.clone(), serial.clone())
            }
            _ => None,
        };
        CompositorDisplayHeadEvidence {
            target: self.target.clone(),
            sink,
            physical_size: self.physical_size,
        }
    }

    /// Build exact current/mode evidence after a separate role proof.
    ///
    /// `auto_supported` is intentionally supplied by the future concrete owner;
    /// merely advertising output-management modes does not prove Auto semantics.
    pub fn refresh_evidence(
        &self,
        role: DisplayRefreshTargetRole,
        auto_supported: bool,
    ) -> Result<DisplayRefreshEvidence, WlrHeadEvidenceBlock> {
        if !self.enabled {
            return Err(WlrHeadEvidenceBlock::Disabled);
        }
        let current_key = self
            .current_mode
            .as_ref()
            .ok_or(WlrHeadEvidenceBlock::CurrentModeUnknown)?;
        let current = self
            .modes
            .get(current_key)
            .ok_or(WlrHeadEvidenceBlock::CurrentModeNotAdvertised)?
            .mode;
        if current.refresh.get() == 0 {
            return Err(WlrHeadEvidenceBlock::CurrentRefreshUnknown);
        }

        let available = self
            .modes
            .iter()
            .filter_map(|(key, evidence)| (key != current_key).then_some(evidence.mode))
            .collect::<Vec<_>>();

        Ok(DisplayRefreshEvidence::from_observed_modes(
            self.target.clone(),
            role,
            current,
            &available,
            auto_supported,
        ))
    }

    /// Preferred modes are diagnostics/evidence only and never override the
    /// compositor's explicit current-mode object.
    pub fn preferred_modes(&self) -> Vec<DisplayMode> {
        self.modes
            .values()
            .filter_map(|evidence| evidence.preferred.then_some(evidence.mode))
            .collect()
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::display_refresh::DisplayRefreshPreset;

    fn target() -> DisplayRefreshTargetId {
        DisplayRefreshTargetId::new("wlr-head:opaque-3")
    }

    fn complete_head() -> WlrOutputHeadState {
        let mut head = WlrOutputHeadState::new(target());
        for event in [
            WlrOutputHeadEvent::Name("eDP-1".into()),
            WlrOutputHeadEvent::Make("BOE".into()),
            WlrOutputHeadEvent::Model("NE160QDM".into()),
            WlrOutputHeadEvent::SerialNumber("1234".into()),
            WlrOutputHeadEvent::PhysicalSize {
                width: 344,
                height: 215,
            },
            WlrOutputHeadEvent::Enabled(true),
            WlrOutputHeadEvent::Mode {
                key: WlrOutputModeKey::new("m60"),
                width: 2560,
                height: 1600,
                refresh_mhz: 59_940,
                preferred: false,
            },
            WlrOutputHeadEvent::Mode {
                key: WlrOutputModeKey::new("m120"),
                width: 2560,
                height: 1600,
                refresh_mhz: 119_880,
                preferred: true,
            },
            WlrOutputHeadEvent::CurrentMode(Some(WlrOutputModeKey::new("m120"))),
        ] {
            head.apply(event);
        }
        head
    }

    #[test]
    fn identity_uses_sink_metadata_not_head_name() {
        let mut first = complete_head();
        let first_identity = first.identity_evidence();
        first.apply(WlrOutputHeadEvent::Name("totally-different-runtime-name".into()));
        assert_eq!(first.identity_evidence(), first_identity);
        assert_eq!(first.name(), Some("totally-different-runtime-name"));
    }

    #[test]
    fn incomplete_serial_never_produces_complete_sink_identity() {
        let mut head = complete_head();
        head.apply(WlrOutputHeadEvent::SerialNumber("".into()));
        assert!(head.identity_evidence().sink.is_none());
    }

    #[test]
    fn exact_fractional_modes_feed_product_evidence_losslessly() {
        let head = complete_head();
        let evidence = head
            .refresh_evidence(DisplayRefreshTargetRole::InternalPanelProven, false)
            .unwrap();
        assert_eq!(evidence.current.refresh.get(), 119_880);
        assert_eq!(
            evidence
                .target_for(DisplayRefreshPreset::Hz60)
                .unwrap()
                .exact_refresh()
                .unwrap()
                .get(),
            59_940
        );
        assert_eq!(
            evidence
                .target_for(DisplayRefreshPreset::Hz120)
                .unwrap()
                .exact_refresh()
                .unwrap()
                .get(),
            119_880
        );
        assert!(evidence.target_for(DisplayRefreshPreset::Auto).is_none());
    }

    #[test]
    fn auto_is_never_inferred_without_owner_evidence() {
        let head = complete_head();
        let without = head
            .refresh_evidence(DisplayRefreshTargetRole::InternalPanelProven, false)
            .unwrap();
        let with = head
            .refresh_evidence(DisplayRefreshTargetRole::InternalPanelProven, true)
            .unwrap();
        assert!(without.target_for(DisplayRefreshPreset::Auto).is_none());
        assert!(with.target_for(DisplayRefreshPreset::Auto).is_some());
    }

    #[test]
    fn disabled_or_unknown_current_mode_blocks_refresh_evidence() {
        let mut head = complete_head();
        head.apply(WlrOutputHeadEvent::Enabled(false));
        assert_eq!(
            head.refresh_evidence(DisplayRefreshTargetRole::InternalPanelProven, false),
            Err(WlrHeadEvidenceBlock::Disabled)
        );

        head.apply(WlrOutputHeadEvent::Enabled(true));
        head.apply(WlrOutputHeadEvent::CurrentMode(None));
        assert_eq!(
            head.refresh_evidence(DisplayRefreshTargetRole::InternalPanelProven, false),
            Err(WlrHeadEvidenceBlock::CurrentModeUnknown)
        );
    }

    #[test]
    fn removed_current_mode_invalidates_evidence() {
        let mut head = complete_head();
        head.apply(WlrOutputHeadEvent::ModeRemoved(WlrOutputModeKey::new("m120")));
        assert_eq!(
            head.refresh_evidence(DisplayRefreshTargetRole::InternalPanelProven, false),
            Err(WlrHeadEvidenceBlock::CurrentModeUnknown)
        );
    }

    #[test]
    fn source_contains_no_configuration_surface() {
        let source = include_str!("wlr_output_head.rs");
        let forbidden = [
            ["create_", "configuration("].concat(),
            ["enable_", "head("].concat(),
            ["disable_", "head("].concat(),
            ["set_", "mode("].concat(),
            ["Command", "::new("].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected mutation/process token: {needle}");
        }
        assert!(!source.contains("wayland_protocols_wlr"));
    }
}
