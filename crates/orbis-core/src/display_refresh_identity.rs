//! Cross-source identity proof for DisplayRefresh mutation targets.
//!
//! A compositor head name such as `eDP-1` is not sufficient evidence that a
//! mutation target is the built-in panel. Likewise a DRM connector's human
//! readable name is not its authoritative connector type. This module therefore
//! models a stricter correlation boundary:
//!
//! 1. the compositor owner supplies its opaque [`DisplayRefreshTargetId`] plus a
//!    complete sink identity (make/model/serial and physical size);
//! 2. an independent DRM/KMS source supplies typed connector evidence, including
//!    `connector_type`, connection status and the same normalized sink identity;
//! 3. exactly one connected DRM connector must match the complete sink identity;
//! 4. only typed DRM connector kinds that are intrinsically embedded-panel
//!    transports may produce [`DisplayRefreshTargetRole::InternalPanelProven`].
//!
//! Missing serials, incomplete identity, duplicate matches and unknown connector
//! types remain unproven. This is deliberately conservative: inability to prove
//! the internal panel disables unattended/Quick-Control writes instead of
//! guessing from connector names.

use serde::{Deserialize, Serialize};

use crate::display_refresh::{DisplayRefreshTargetId, DisplayRefreshTargetRole};

/// Normalized physical size used as independent sink-correlation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplayPhysicalSizeMm {
    /// Physical width in millimetres.
    pub width: u32,
    /// Physical height in millimetres.
    pub height: u32,
}

impl DisplayPhysicalSizeMm {
    /// Construct a meaningful non-zero physical size.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        Some(Self { width, height })
    }
}

/// Canonical sink identity obtained independently from compositor and DRM/EDID
/// observations.
///
/// Serial is mandatory in the first proof version. Some panels omit it; those
/// machines remain write-disabled until a different independently validated
/// correlation method is designed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplaySinkIdentity {
    manufacturer: String,
    model: String,
    serial: String,
}

impl DisplaySinkIdentity {
    /// Build a complete normalized identity. Empty components are rejected.
    pub fn new(
        manufacturer: impl Into<String>,
        model: impl Into<String>,
        serial: impl Into<String>,
    ) -> Option<Self> {
        let manufacturer = manufacturer.into();
        let model = model.into();
        let serial = serial.into();
        if manufacturer.trim().is_empty() || model.trim().is_empty() || serial.trim().is_empty() {
            return None;
        }
        Some(Self {
            manufacturer,
            model,
            serial,
        })
    }

    /// Normalized manufacturer identifier/string.
    pub fn manufacturer(&self) -> &str {
        &self.manufacturer
    }

    /// Normalized model identifier/string.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Normalized non-empty serial identifier/string.
    pub fn serial(&self) -> &str {
        &self.serial
    }
}

/// Typed DRM/KMS connector classification.
///
/// The future Linux source must obtain this from KMS connector metadata, not by
/// parsing a connector's display name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrmConnectorTypeEvidence {
    /// Embedded DisplayPort connector.
    Edp,
    /// Legacy laptop LVDS connector.
    Lvds,
    /// MIPI DSI panel connector.
    Dsi,
    /// Full-size or mini DisplayPort connector.
    DisplayPort,
    /// HDMI-A connector.
    HdmiA,
    /// HDMI-B connector.
    HdmiB,
    /// USB display/USB-C connector exposed distinctly by the DRM source.
    Usb,
    /// Virtual/writeback connector.
    Virtual,
    /// Connector type exists but is not classified by this product contract.
    Other,
}

impl DrmConnectorTypeEvidence {
    fn role(self) -> Option<DisplayRefreshTargetRole> {
        match self {
            Self::Edp | Self::Lvds | Self::Dsi => {
                Some(DisplayRefreshTargetRole::InternalPanelProven)
            }
            Self::DisplayPort | Self::HdmiA | Self::HdmiB | Self::Usb => {
                Some(DisplayRefreshTargetRole::External)
            }
            Self::Virtual | Self::Other => None,
        }
    }
}

/// Independent DRM/KMS evidence for one connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrmDisplayConnectorEvidence {
    /// DRM object ID from the KMS connector query.
    pub connector_id: u32,
    /// Typed connector kind from KMS metadata, not a parsed connector name.
    pub connector_type: DrmConnectorTypeEvidence,
    /// Whether KMS reports this connector as currently connected.
    pub connected: bool,
    /// Canonical sink identity derived from DRM-side evidence.
    pub sink: Option<DisplaySinkIdentity>,
    /// Physical dimensions reported by DRM/KMS for the connected sink.
    pub physical_size: Option<DisplayPhysicalSizeMm>,
}

/// Compositor-owner identity evidence for one opaque mutation target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositorDisplayHeadEvidence {
    /// Opaque runtime target identity owned by the compositor mutation backend.
    pub target: DisplayRefreshTargetId,
    /// Canonical sink identity from compositor output-management metadata.
    pub sink: Option<DisplaySinkIdentity>,
    /// Physical dimensions from compositor output-management metadata.
    pub physical_size: Option<DisplayPhysicalSizeMm>,
}

/// Why an opaque compositor target cannot be assigned a proven role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTargetIdentityBlock {
    /// Compositor did not provide complete make/model/serial identity.
    CompositorSinkIdentityIncomplete,
    /// Compositor did not provide meaningful physical dimensions.
    CompositorPhysicalSizeMissing,
    /// No connected DRM connector had exactly the same complete sink evidence.
    NoMatchingDrmConnector,
    /// More than one connected DRM connector matched all evidence; choosing one
    /// would be ambiguous.
    AmbiguousDrmConnectorMatch,
    /// A unique physical connector was correlated, but its typed DRM connector
    /// kind is not classified safely by this contract.
    ConnectorTypeUnclassified,
}

/// Successful role proof for one opaque compositor target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayTargetIdentityProof {
    target: DisplayRefreshTargetId,
    drm_connector_id: u32,
    role: DisplayRefreshTargetRole,
}

impl DisplayTargetIdentityProof {
    /// Opaque compositor target covered by the proof.
    pub fn target(&self) -> &DisplayRefreshTargetId {
        &self.target
    }

    /// Independent DRM connector object correlated to the compositor target.
    pub fn drm_connector_id(&self) -> u32 {
        self.drm_connector_id
    }

    /// Proven product role.
    pub fn role(&self) -> DisplayRefreshTargetRole {
        self.role
    }
}

/// Correlate one compositor mutation target to exactly one DRM connector and
/// classify its role without using either side's human-readable connector name.
pub fn prove_display_target_identity(
    head: &CompositorDisplayHeadEvidence,
    drm_connectors: &[DrmDisplayConnectorEvidence],
) -> Result<DisplayTargetIdentityProof, DisplayTargetIdentityBlock> {
    let sink = head
        .sink
        .as_ref()
        .ok_or(DisplayTargetIdentityBlock::CompositorSinkIdentityIncomplete)?;
    let physical_size = head
        .physical_size
        .ok_or(DisplayTargetIdentityBlock::CompositorPhysicalSizeMissing)?;

    let matches = drm_connectors
        .iter()
        .filter(|connector| {
            connector.connected
                && connector.sink.as_ref() == Some(sink)
                && connector.physical_size == Some(physical_size)
        })
        .collect::<Vec<_>>();

    let connector = match matches.as_slice() {
        [] => return Err(DisplayTargetIdentityBlock::NoMatchingDrmConnector),
        [connector] => *connector,
        _ => return Err(DisplayTargetIdentityBlock::AmbiguousDrmConnectorMatch),
    };

    let role = connector
        .connector_type
        .role()
        .ok_or(DisplayTargetIdentityBlock::ConnectorTypeUnclassified)?;

    Ok(DisplayTargetIdentityProof {
        target: head.target.clone(),
        drm_connector_id: connector.connector_id,
        role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink(serial: &str) -> DisplaySinkIdentity {
        DisplaySinkIdentity::new("BOE", "NE160QDM", serial).unwrap()
    }

    fn size() -> DisplayPhysicalSizeMm {
        DisplayPhysicalSizeMm::new(344, 215).unwrap()
    }

    fn head(serial: &str) -> CompositorDisplayHeadEvidence {
        CompositorDisplayHeadEvidence {
            target: DisplayRefreshTargetId::new("wlr-head:opaque-7"),
            sink: Some(sink(serial)),
            physical_size: Some(size()),
        }
    }

    fn connector(
        id: u32,
        connector_type: DrmConnectorTypeEvidence,
        serial: &str,
    ) -> DrmDisplayConnectorEvidence {
        DrmDisplayConnectorEvidence {
            connector_id: id,
            connector_type,
            connected: true,
            sink: Some(sink(serial)),
            physical_size: Some(size()),
        }
    }

    #[test]
    fn unique_edp_match_proves_internal_panel() {
        let proof = prove_display_target_identity(
            &head("1234"),
            &[connector(42, DrmConnectorTypeEvidence::Edp, "1234")],
        )
        .unwrap();
        assert_eq!(proof.target().as_str(), "wlr-head:opaque-7");
        assert_eq!(proof.drm_connector_id(), 42);
        assert_eq!(proof.role(), DisplayRefreshTargetRole::InternalPanelProven);
    }

    #[test]
    fn unique_hdmi_match_is_proven_external_not_internal() {
        let proof = prove_display_target_identity(
            &head("1234"),
            &[connector(9, DrmConnectorTypeEvidence::HdmiA, "1234")],
        )
        .unwrap();
        assert_eq!(proof.role(), DisplayRefreshTargetRole::External);
    }

    #[test]
    fn sink_serial_is_mandatory_for_first_version_proof() {
        assert!(DisplaySinkIdentity::new("BOE", "NE160QDM", "").is_none());
        let incomplete = CompositorDisplayHeadEvidence {
            target: DisplayRefreshTargetId::new("opaque"),
            sink: None,
            physical_size: Some(size()),
        };
        assert_eq!(
            prove_display_target_identity(&incomplete, &[]),
            Err(DisplayTargetIdentityBlock::CompositorSinkIdentityIncomplete)
        );
    }

    #[test]
    fn exact_sink_without_matching_physical_size_is_not_link_evidence() {
        let drm = DrmDisplayConnectorEvidence {
            connector_id: 4,
            connector_type: DrmConnectorTypeEvidence::Edp,
            connected: true,
            sink: Some(sink("1234")),
            physical_size: DisplayPhysicalSizeMm::new(300, 200),
        };
        assert_eq!(
            prove_display_target_identity(&head("1234"), &[drm]),
            Err(DisplayTargetIdentityBlock::NoMatchingDrmConnector)
        );
    }

    #[test]
    fn disconnected_connector_never_proves_target() {
        let mut drm = connector(4, DrmConnectorTypeEvidence::Edp, "1234");
        drm.connected = false;
        assert_eq!(
            prove_display_target_identity(&head("1234"), &[drm]),
            Err(DisplayTargetIdentityBlock::NoMatchingDrmConnector)
        );
    }

    #[test]
    fn duplicate_complete_matches_fail_closed() {
        assert_eq!(
            prove_display_target_identity(
                &head("1234"),
                &[
                    connector(4, DrmConnectorTypeEvidence::Edp, "1234"),
                    connector(5, DrmConnectorTypeEvidence::Edp, "1234"),
                ],
            ),
            Err(DisplayTargetIdentityBlock::AmbiguousDrmConnectorMatch)
        );
    }

    #[test]
    fn virtual_or_unknown_connector_kind_never_proves_role() {
        for kind in [
            DrmConnectorTypeEvidence::Virtual,
            DrmConnectorTypeEvidence::Other,
        ] {
            assert_eq!(
                prove_display_target_identity(&head("1234"), &[connector(8, kind, "1234")]),
                Err(DisplayTargetIdentityBlock::ConnectorTypeUnclassified)
            );
        }
    }

    #[test]
    fn resolver_has_no_name_based_shortcut() {
        let source = include_str!("display_refresh_identity.rs");
        assert!(!source.contains("eDP-1"));
        assert!(!source.contains("starts_with(\"eDP"));
        assert!(!source.contains("contains(\"eDP"));
        assert!(source.contains("connector_type"));
        assert!(source.contains("physical_size"));
        assert!(source.contains("sink"));
    }
}
