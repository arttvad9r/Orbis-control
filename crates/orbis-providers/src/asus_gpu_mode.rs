//! Read-only ASUS Armoury product-GPU state decoding.

use std::path::PathBuf;

use crate::ProviderError;

const ASUSD_SERVICE: &str = "xyz.ljones.Asusd";
const ASUS_ARMOURY_PATH: &str = "/xyz/ljones/asus_armoury";

#[zbus::proxy(interface = "xyz.ljones.AsusArmoury")]
trait AsusArmouryGpuAttribute {
    #[zbus(property)]
    fn current_value(&self) -> zbus::Result<i32>;
    #[zbus(property)]
    fn queued_gpu_value(&self) -> zbus::Result<i32>;
}

/// Product-level GPU state reported by the ASUS Armoury attribute pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AsusGpuMode {
    /// Dynamic/Optimus mode: dGPU enabled, MUX integrated.
    Hybrid,
    /// iGPU-only mode: dGPU disabled, MUX integrated.
    Integrated,
    /// dGPU-only/MUX mode: dGPU enabled, MUX discrete.
    Ultimate,
    /// One or more required values were not readable.
    Incomplete,
    /// Values were readable but do not form a known state.
    Unknown {
        /// Raw `dgpu_disable` value.
        dgpu_disable: u32,
        /// Raw `gpu_mux_mode` value.
        gpu_mux_mode: u32,
    },
    /// The pair describes an impossible/contradictory state.
    Conflicted {
        /// Raw `dgpu_disable` value.
        dgpu_disable: u32,
        /// Raw `gpu_mux_mode` value.
        gpu_mux_mode: u32,
    },
}

/// Read-only current and deferred ASUS GPU attribute state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AsusGpuModeSnapshot {
    /// Current firmware `dgpu_disable` value.
    pub current_dgpu_disable: u32,
    /// Current firmware `gpu_mux_mode` value.
    pub current_gpu_mux_mode: u32,
    /// Deferred `dgpu_disable` value, if queued by asusd.
    pub queued_dgpu_disable: Option<u32>,
    /// Deferred `gpu_mux_mode` value, if queued by asusd.
    pub queued_gpu_mux_mode: Option<u32>,
    /// Decoded current product mode.
    pub current_mode: AsusGpuMode,
    /// Decoded deferred product mode, if both values are queued.
    pub queued_mode: Option<AsusGpuMode>,
    /// Whether a complete deferred pair differs from current firmware state.
    pub reboot_required: bool,
}

impl AsusGpuModeSnapshot {
    /// Build a snapshot from raw current and deferred attribute values.
    pub fn from_values(
        current_dgpu_disable: Option<u32>,
        current_gpu_mux_mode: Option<u32>,
        queued_dgpu_disable: Option<u32>,
        queued_gpu_mux_mode: Option<u32>,
    ) -> Self {
        let current_mode = decode_asus_gpu_mode(current_dgpu_disable, current_gpu_mux_mode);
        let queued_mode = match (queued_dgpu_disable, queued_gpu_mux_mode) {
            (Some(dgpu), Some(mux)) => Some(decode_asus_gpu_mode(Some(dgpu), Some(mux))),
            _ => None,
        };
        let reboot_required = queued_mode.is_some()
            && (queued_dgpu_disable != current_dgpu_disable
                || queued_gpu_mux_mode != current_gpu_mux_mode);
        Self {
            current_dgpu_disable: current_dgpu_disable.unwrap_or_default(),
            current_gpu_mux_mode: current_gpu_mux_mode.unwrap_or_default(),
            queued_dgpu_disable,
            queued_gpu_mux_mode,
            current_mode,
            queued_mode,
            reboot_required,
        }
    }

    /// Whether a complete deferred pair exists and differs from current state.
    pub fn reboot_required(&self) -> bool {
        self.reboot_required
    }
}

/// Fixed kernel firmware-attributes paths used by the ASUS Armoury driver.
pub const ASUS_ARMOURY_DGPU_DISABLE_PATH: &str =
    "class/firmware-attributes/asus-armoury/attributes/dgpu_disable/current_value";
/// Fixed kernel firmware-attributes path for the physical MUX mode.
pub const ASUS_ARMOURY_GPU_MUX_MODE_PATH: &str =
    "class/firmware-attributes/asus-armoury/attributes/gpu_mux_mode/current_value";

/// Read-only provider for the ASUS Armoury product-GPU state pair.
pub struct AsusArmouryGpuModeProvider {
    sysfs_root: PathBuf,
}

/// Read-only asusd D-Bus source for current and deferred GPU attributes.
pub struct AsusdGpuModeProvider {
    connection: zbus::Connection,
}

impl AsusdGpuModeProvider {
    /// Construct without performing D-Bus I/O.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    /// Read current and queued values from both ASUS attributes.
    pub async fn read_snapshot(&self) -> Result<AsusGpuModeSnapshot, ProviderError> {
        let read = |attribute: &'static str| async move {
            let path = format!("{ASUS_ARMOURY_PATH}/{attribute}");
            let proxy = AsusArmouryGpuAttributeProxy::builder(&self.connection)
                .destination(ASUSD_SERVICE)
                .map_err(|error| ProviderError::Dbus(error.to_string()))?
                .path(path)
                .map_err(|error| ProviderError::Dbus(error.to_string()))?
                .build()
                .await
                .map_err(|error| ProviderError::Dbus(error.to_string()))?;
            let current = proxy
                .current_value()
                .await
                .map_err(|error| ProviderError::Dbus(error.to_string()))?;
            let queued = proxy
                .queued_gpu_value()
                .await
                .map_err(|error| ProviderError::Dbus(error.to_string()))?;
            let current = u32::try_from(current).map_err(|_| {
                ProviderError::Internal(format!("negative ASUS GPU value: {current}"))
            })?;
            let queued = if queued < 0 {
                None
            } else {
                Some(u32::try_from(queued).map_err(|_| {
                    ProviderError::Internal(format!("invalid queued ASUS GPU value: {queued}"))
                })?)
            };
            Ok::<_, ProviderError>((current, queued))
        };
        let (dgpu, mux) = tokio::try_join!(read("dgpu_disable"), read("gpu_mux_mode"))?;
        Ok(AsusGpuModeSnapshot::from_values(
            Some(dgpu.0),
            Some(mux.0),
            dgpu.1,
            mux.1,
        ))
    }
}

impl AsusArmouryGpuModeProvider {
    /// Construct without performing I/O.
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
        }
    }

    /// Read both attributes and decode their joint product state.
    pub fn read_mode(&self) -> Result<AsusGpuMode, ProviderError> {
        let dgpu_disable = read_attribute(&self.sysfs_root.join(ASUS_ARMOURY_DGPU_DISABLE_PATH))?;
        let gpu_mux_mode = read_attribute(&self.sysfs_root.join(ASUS_ARMOURY_GPU_MUX_MODE_PATH))?;
        Ok(decode_asus_gpu_mode(Some(dgpu_disable), Some(gpu_mux_mode)))
    }
}

impl Default for AsusArmouryGpuModeProvider {
    fn default() -> Self {
        Self::new("/sys")
    }
}

fn read_attribute(path: &std::path::Path) -> Result<u32, ProviderError> {
    let raw = std::fs::read_to_string(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            ProviderError::Unsupported(format!("ASUS GPU attribute absent: {}", path.display()))
        }
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "ASUS GPU attribute read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    })?;
    raw.trim().parse().map_err(|_| {
        ProviderError::Internal(format!(
            "ASUS GPU attribute is malformed: {}",
            path.display()
        ))
    })
}

/// Decode the two ASUS Armoury firmware attributes used by ROG Control Center.
///
/// This is a read-only interpretation. It does not imply that the attributes
/// can be safely written or that a change would apply without reboot.
pub fn decode_asus_gpu_mode(dgpu_disable: Option<u32>, gpu_mux_mode: Option<u32>) -> AsusGpuMode {
    let (Some(dgpu_disable), Some(gpu_mux_mode)) = (dgpu_disable, gpu_mux_mode) else {
        return AsusGpuMode::Incomplete;
    };

    match (dgpu_disable, gpu_mux_mode) {
        (0, 1) => AsusGpuMode::Hybrid,
        (1, 1) => AsusGpuMode::Integrated,
        (0, 0) => AsusGpuMode::Ultimate,
        (1, 0) => AsusGpuMode::Conflicted {
            dgpu_disable,
            gpu_mux_mode,
        },
        _ if dgpu_disable <= 1 && gpu_mux_mode <= 1 => AsusGpuMode::Unknown {
            dgpu_disable,
            gpu_mux_mode,
        },
        _ => AsusGpuMode::Unknown {
            dgpu_disable,
            gpu_mux_mode,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_upstream_asus_three_mode_mapping() {
        assert_eq!(decode_asus_gpu_mode(Some(0), Some(1)), AsusGpuMode::Hybrid);
        assert_eq!(
            decode_asus_gpu_mode(Some(1), Some(1)),
            AsusGpuMode::Integrated
        );
        assert_eq!(
            decode_asus_gpu_mode(Some(0), Some(0)),
            AsusGpuMode::Ultimate
        );
    }

    #[test]
    fn missing_attribute_is_incomplete() {
        assert_eq!(decode_asus_gpu_mode(Some(0), None), AsusGpuMode::Incomplete);
        assert_eq!(decode_asus_gpu_mode(None, Some(1)), AsusGpuMode::Incomplete);
    }

    #[test]
    fn impossible_pair_is_conflicted() {
        assert_eq!(
            decode_asus_gpu_mode(Some(1), Some(0)),
            AsusGpuMode::Conflicted {
                dgpu_disable: 1,
                gpu_mux_mode: 0,
            }
        );
    }

    #[test]
    fn future_values_are_not_coerced() {
        assert_eq!(
            decode_asus_gpu_mode(Some(2), Some(1)),
            AsusGpuMode::Unknown {
                dgpu_disable: 2,
                gpu_mux_mode: 1,
            }
        );
    }

    #[test]
    fn provider_reads_the_two_attributes_without_writing() {
        let root = std::env::temp_dir().join(format!("orbis-asus-gpu-mode-{}", std::process::id()));
        let base = root.join("class/firmware-attributes/asus-armoury/attributes");
        std::fs::create_dir_all(base.join("dgpu_disable")).expect("dirs");
        std::fs::create_dir_all(base.join("gpu_mux_mode")).expect("dirs");
        std::fs::write(base.join("dgpu_disable/current_value"), "0\n").expect("write");
        std::fs::write(base.join("gpu_mux_mode/current_value"), "1\n").expect("write");

        assert_eq!(
            AsusArmouryGpuModeProvider::new(&root)
                .read_mode()
                .expect("read mode"),
            AsusGpuMode::Hybrid
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn queued_pair_requires_reboot_and_decodes_independently() {
        let snapshot = AsusGpuModeSnapshot::from_values(Some(0), Some(1), Some(1), Some(1));
        assert_eq!(snapshot.current_mode, AsusGpuMode::Hybrid);
        assert_eq!(snapshot.queued_mode, Some(AsusGpuMode::Integrated));
        assert!(snapshot.reboot_required());
    }

    #[test]
    fn partial_queue_is_not_presented_as_a_mode() {
        let snapshot = AsusGpuModeSnapshot::from_values(Some(0), Some(1), Some(1), None);
        assert_eq!(snapshot.queued_mode, None);
        assert!(!snapshot.reboot_required());
    }
}
