//! Read-only DRM/KMS connector identity evidence for DisplayRefresh.
//!
//! This source intentionally uses DRM `GETCONNECTOR` metadata for connector
//! type/state/physical size. It never infers internal-panel role from a sysfs
//! connector name such as `eDP-1`. EDID bytes are correlated to the same DRM
//! connector by the kernel `connector_id` file under that card's sysfs tree.
//!
//! No DRM master, modeset, property write or generic ioctl surface is exposed.

use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use drm::control::Device as ControlDevice;
use drm::control::connector::{Interface, State};

use orbis_core::display_refresh_identity::{
    DisplayPhysicalSizeMm, DisplaySinkIdentity, DrmConnectorTypeEvidence,
    DrmDisplayConnectorEvidence,
};

use crate::error::ProviderError;

const DRM_DEV_ROOT: &str = "/dev/dri";
const DRM_SYSFS_ROOT: &str = "/sys/class/drm";

#[derive(Debug)]
struct ReadOnlyDrmCard {
    file: File,
}

impl AsFd for ReadOnlyDrmCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl drm::Device for ReadOnlyDrmCard {}
impl drm::control::Device for ReadOnlyDrmCard {}

impl ReadOnlyDrmCard {
    fn open(path: &Path) -> Result<Self, ProviderError> {
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|error| map_open_error(path, error))?;
        Ok(Self { file })
    }
}

/// Read-only source of independent DRM connector evidence.
#[async_trait]
pub trait DrmDisplayIdentitySource: Send + Sync {
    /// Read one fresh snapshot from every readable primary DRM card.
    async fn drm_connector_evidence(
        &self,
    ) -> Result<Vec<DrmDisplayConnectorEvidence>, ProviderError>;
}

/// Production DRM/KMS source over fixed Linux `/dev/dri` and `/sys/class/drm`
/// roots. Roots are injectable only through the test constructor.
#[derive(Debug, Clone)]
pub struct LinuxDrmDisplayIdentitySource {
    dev_root: PathBuf,
    sysfs_root: PathBuf,
}

impl Default for LinuxDrmDisplayIdentitySource {
    fn default() -> Self {
        Self {
            dev_root: PathBuf::from(DRM_DEV_ROOT),
            sysfs_root: PathBuf::from(DRM_SYSFS_ROOT),
        }
    }
}

impl LinuxDrmDisplayIdentitySource {
    /// Construct the production fixed-root source.
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn for_test(dev_root: PathBuf, sysfs_root: PathBuf) -> Self {
        Self { dev_root, sysfs_root }
    }

    /// Perform one synchronous read-only KMS snapshot.
    pub fn read_connectors_blocking(
        &self,
    ) -> Result<Vec<DrmDisplayConnectorEvidence>, ProviderError> {
        let cards = primary_card_paths(&self.dev_root)?;
        if cards.is_empty() {
            return Err(ProviderError::Unsupported(
                "no primary DRM card nodes found".into(),
            ));
        }

        let mut evidence = Vec::new();
        let mut opened_any = false;
        let mut last_error = None;

        for card_path in cards {
            let card_name = card_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ProviderError::Internal("non-UTF8 DRM card node name".into()))?;

            let card = match ReadOnlyDrmCard::open(&card_path) {
                Ok(card) => card,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            opened_any = true;

            let resources = card.resource_handles().map_err(|error| {
                ProviderError::BackendUnavailable(format!(
                    "DRM resource query failed for {}: {error}",
                    card_path.display()
                ))
            })?;

            for &handle in resources.connectors() {
                let info = card.get_connector(handle, false).map_err(|error| {
                    ProviderError::BackendUnavailable(format!(
                        "DRM GETCONNECTOR failed for {} connector {}: {error}",
                        card_path.display(),
                        u32::from(handle)
                    ))
                })?;

                let connector_id = u32::from(handle);
                let connected = info.state() == State::Connected;
                let connector_type = connector_type(info.interface());
                let physical_size = info
                    .size()
                    .and_then(|(width, height)| DisplayPhysicalSizeMm::new(width, height));

                let sink = if connected {
                    read_connector_edid(&self.sysfs_root, card_name, connector_id)
                        .ok()
                        .and_then(|bytes| parse_edid_sink_identity(&bytes).ok())
                } else {
                    None
                };

                evidence.push(DrmDisplayConnectorEvidence {
                    connector_id,
                    connector_type,
                    connected,
                    sink,
                    physical_size,
                });
            }
        }

        if !opened_any {
            return Err(last_error.unwrap_or_else(|| {
                ProviderError::BackendUnavailable("no readable primary DRM card nodes".into())
            }));
        }

        Ok(evidence)
    }
}

#[async_trait]
impl DrmDisplayIdentitySource for LinuxDrmDisplayIdentitySource {
    async fn drm_connector_evidence(
        &self,
    ) -> Result<Vec<DrmDisplayConnectorEvidence>, ProviderError> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || source.read_connectors_blocking())
            .await
            .map_err(|error| {
                ProviderError::Internal(format!("DRM identity probe task failed: {error}"))
            })?
    }
}

fn primary_card_paths(root: &Path) -> Result<Vec<PathBuf>, ProviderError> {
    let entries = std::fs::read_dir(root).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            ProviderError::Unsupported(format!("DRM device root absent: {}", root.display()))
        }
        std::io::ErrorKind::PermissionDenied => {
            ProviderError::PermissionDenied(format!("DRM device root denied: {}", root.display()))
        }
        _ => ProviderError::Io(error),
    })?;

    let mut cards = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("card"))
                .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect::<Vec<_>>();
    cards.sort();
    Ok(cards)
}

fn map_open_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            ProviderError::BackendUnavailable(format!("DRM card disappeared: {}", path.display()))
        }
        std::io::ErrorKind::PermissionDenied => {
            ProviderError::PermissionDenied(format!("DRM card read denied: {}", path.display()))
        }
        _ => ProviderError::Io(error),
    }
}

fn connector_type(interface: Interface) -> DrmConnectorTypeEvidence {
    match interface {
        Interface::EmbeddedDisplayPort => DrmConnectorTypeEvidence::Edp,
        Interface::LVDS => DrmConnectorTypeEvidence::Lvds,
        Interface::DSI => DrmConnectorTypeEvidence::Dsi,
        Interface::DisplayPort => DrmConnectorTypeEvidence::DisplayPort,
        Interface::HDMIA => DrmConnectorTypeEvidence::HdmiA,
        Interface::HDMIB => DrmConnectorTypeEvidence::HdmiB,
        Interface::USB => DrmConnectorTypeEvidence::Usb,
        Interface::Virtual | Interface::Writeback => DrmConnectorTypeEvidence::Virtual,
        _ => DrmConnectorTypeEvidence::Other,
    }
}

/// Locate EDID by numeric connector object ID within one already-selected DRM
/// card. The connector directory name is never parsed for connector type/role.
fn read_connector_edid(
    sysfs_root: &Path,
    card_name: &str,
    connector_id: u32,
) -> Result<Vec<u8>, ProviderError> {
    let prefix = format!("{card_name}-");
    let entries = std::fs::read_dir(sysfs_root).map_err(ProviderError::Io)?;
    let mut matches = Vec::new();

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let id_path = path.join("connector_id");
        let Ok(raw) = std::fs::read_to_string(&id_path) else {
            continue;
        };
        let Ok(id) = raw.trim().parse::<u32>() else {
            continue;
        };
        if id == connector_id {
            matches.push(path);
        }
    }

    let connector_dir = match matches.as_slice() {
        [] => {
            return Err(ProviderError::BackendUnavailable(format!(
                "DRM sysfs connector_id {connector_id} not found for {card_name}"
            )));
        }
        [path] => path,
        _ => {
            return Err(ProviderError::Internal(format!(
                "DRM sysfs connector_id {connector_id} is ambiguous for {card_name}"
            )));
        }
    };

    std::fs::read(connector_dir.join("edid")).map_err(|error| match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ProviderError::PermissionDenied("DRM EDID read denied".into())
        }
        std::io::ErrorKind::NotFound => {
            ProviderError::BackendUnavailable("DRM EDID unavailable".into())
        }
        _ => ProviderError::Io(error),
    })
}

/// Strict EDID base-block identity used only for cross-source correlation.
/// Missing monitor-name or serial descriptors remain unproven instead of using
/// product-code/name heuristics that may not match compositor metadata.
pub fn parse_edid_sink_identity(bytes: &[u8]) -> Result<DisplaySinkIdentity, ProviderError> {
    if bytes.len() < 128 {
        return Err(ProviderError::Internal("EDID base block is truncated".into()));
    }
    let base = &bytes[..128];
    const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if base[..8] != HEADER {
        return Err(ProviderError::Internal("EDID header is invalid".into()));
    }
    if base.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) != 0 {
        return Err(ProviderError::Internal("EDID base checksum is invalid".into()));
    }

    let raw_vendor = u16::from_be_bytes([base[8], base[9]]);
    let manufacturer = decode_manufacturer(raw_vendor)?;
    let model = descriptor_text(base, 0xfc)
        .ok_or_else(|| ProviderError::Unsupported("EDID monitor-name descriptor missing".into()))?;
    let serial = descriptor_text(base, 0xff)
        .ok_or_else(|| ProviderError::Unsupported("EDID serial descriptor missing".into()))?;

    DisplaySinkIdentity::new(manufacturer, model, serial)
        .ok_or_else(|| ProviderError::Internal("EDID identity is incomplete".into()))
}

fn decode_manufacturer(raw: u16) -> Result<String, ProviderError> {
    let values = [
        ((raw >> 10) & 0x1f) as u8,
        ((raw >> 5) & 0x1f) as u8,
        (raw & 0x1f) as u8,
    ];
    if values.iter().any(|value| !(1..=26).contains(value)) {
        return Err(ProviderError::Internal("EDID manufacturer code is invalid".into()));
    }
    Ok(values
        .into_iter()
        .map(|value| char::from(b'@' + value))
        .collect())
}

fn descriptor_text(base: &[u8], tag: u8) -> Option<String> {
    for offset in [54usize, 72, 90, 108] {
        let block = &base[offset..offset + 18];
        if block[0] == 0
            && block[1] == 0
            && block[2] == 0
            && block[3] == tag
            && block[4] == 0
        {
            let value = String::from_utf8_lossy(&block[5..18])
                .trim_matches(|ch: char| ch == '\0' || ch == '\n' || ch == '\r' || ch == ' ')
                .to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_edid() -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        // BOE: B=2, O=15, E=5.
        let vendor = (2u16 << 10) | (15u16 << 5) | 5u16;
        bytes[8..10].copy_from_slice(&vendor.to_be_bytes());

        let model = b"NE160QDM\n    ";
        bytes[54..59].copy_from_slice(&[0, 0, 0, 0xfc, 0]);
        bytes[59..72].copy_from_slice(model);
        let serial = b"1234\n        ";
        bytes[72..77].copy_from_slice(&[0, 0, 0, 0xff, 0]);
        bytes[77..90].copy_from_slice(serial);

        let sum = bytes.iter().take(127).fold(0u8, |acc, byte| acc.wrapping_add(*byte));
        bytes[127] = 0u8.wrapping_sub(sum);
        bytes
    }

    #[test]
    fn strict_edid_parser_builds_complete_identity() {
        let identity = parse_edid_sink_identity(&test_edid()).unwrap();
        assert_eq!(identity.manufacturer(), "BOE");
        assert_eq!(identity.model(), "NE160QDM");
        assert_eq!(identity.serial(), "1234");
    }

    #[test]
    fn missing_serial_does_not_fall_back_to_guess() {
        let mut bytes = test_edid();
        bytes[75] = 0xfe;
        let sum = bytes.iter().take(127).fold(0u8, |acc, byte| acc.wrapping_add(*byte));
        bytes[127] = 0u8.wrapping_sub(sum);
        assert!(matches!(
            parse_edid_sink_identity(&bytes),
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[test]
    fn interface_mapping_uses_typed_drm_metadata() {
        assert_eq!(
            connector_type(Interface::EmbeddedDisplayPort),
            DrmConnectorTypeEvidence::Edp
        );
        assert_eq!(connector_type(Interface::LVDS), DrmConnectorTypeEvidence::Lvds);
        assert_eq!(connector_type(Interface::DSI), DrmConnectorTypeEvidence::Dsi);
        assert_eq!(
            connector_type(Interface::HDMIA),
            DrmConnectorTypeEvidence::HdmiA
        );
    }

    #[test]
    fn source_has_no_drm_mutation_or_connector_name_role_heuristic() {
        let source = include_str!("drm_kms_identity.rs");
        let forbidden = [
            ["set_", "crtc("].concat(),
            ["atomic_", "commit"].concat(),
            ["set_", "property("].concat(),
            ["starts_with(\"eDP", "\")"].concat(),
            ["Command", "::new"].concat(),
        ];
        for token in forbidden {
            assert!(!source.contains(&token), "unexpected DRM mutation/heuristic: {token}");
        }
        assert!(source.contains("info.interface()"));
        assert!(source.contains("connector_id"));
    }

    #[test]
    fn test_constructor_keeps_roots_injectable_without_production_generic_paths() {
        let source = LinuxDrmDisplayIdentitySource::for_test(
            PathBuf::from("/tmp/dev"),
            PathBuf::from("/tmp/sys"),
        );
        assert_eq!(source.dev_root, PathBuf::from("/tmp/dev"));
        assert_eq!(source.sysfs_root, PathBuf::from("/tmp/sys"));
    }
}
