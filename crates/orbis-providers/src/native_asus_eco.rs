//! Read-only feasibility preflight for a future native ASUS live Eco backend.

use std::collections::HashSet;
use std::path::PathBuf;

use async_trait::async_trait;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState};

use crate::ProviderError;
use crate::supergfxd::{SupergfxdMode, SupergfxdSnapshot, SupergfxdUserAction};

#[zbus::proxy(
    interface = "org.supergfxctl.Daemon",
    default_service = "org.supergfxctl.Daemon",
    default_path = "/org/supergfxctl/Gfx"
)]
trait SupergfxdReadOnly {
    fn mode(&self) -> zbus::Result<u32>;
    fn pending_mode(&self) -> zbus::Result<u32>;
    fn pending_user_action(&self) -> zbus::Result<u32>;
    fn power(&self) -> zbus::Result<u32>;
    fn supported(&self) -> zbus::Result<Vec<u32>>;
}

fn supergfxd_error(error: zbus::Error) -> ProviderError {
    ProviderError::Dbus(error.to_string())
}

fn power_from_wire(raw: u32) -> orbis_core::gpu::GpuPowerState {
    match raw {
        0 => orbis_core::gpu::GpuPowerState::Active,
        1 => orbis_core::gpu::GpuPowerState::Suspended,
        2..=4 => orbis_core::gpu::GpuPowerState::Off,
        _ => orbis_core::gpu::GpuPowerState::Unknown,
    }
}

/// Why a native live Eco transition is blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcoLiveBlocker {
    /// A process owns an NVIDIA character device.
    NvidiaDeviceUser,
    /// A process owns an NVIDIA DRM node.
    NvidiaDrmUser,
    /// A process owns an NVIDIA I²C adapter.
    NvidiaI2cUser,
    /// A process maps NVIDIA device/library state.
    NvidiaMappedLibrary,
    /// NVIDIA kernel module cannot currently be unloaded.
    NvidiaModuleBusy,
    /// No non-NVIDIA DRM device remains for the display stack.
    MissingIntegratedDrm,
    /// MUX is in dGPU display mode.
    MuxDiscrete,
    /// supergfxd has an in-flight mutation.
    PendingGpuMutation,
    /// Backend and primitive state disagree.
    BackendStateConflict,
    /// Evidence is unreadable or not classifiable.
    Unknown,
}

/// Read-only diagnostic evidence kept separate from the domain verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoDiagnosticEvidence {
    /// Stable category.
    pub category: EcoLiveBlocker,
    /// Human-readable, non-UI-contract detail.
    pub detail: String,
}

/// Coarse NVIDIA holder kind; no PID is part of the domain model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvidiaHolderKind {
    /// `/dev/nvidia*`.
    Device,
    /// NVIDIA-owned `/dev/dri/*`.
    Drm,
    /// NVIDIA-owned `/dev/i2c-*`.
    I2c,
    /// NVIDIA device mapping in `/proc/<pid>/maps`.
    MappedLibrary,
}

/// Read-only holder summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvidiaHolderEvidence {
    /// Holder class.
    pub kind: NvidiaHolderKind,
    /// Number of observed holders.
    pub count: usize,
    /// Optional diagnostic details (process names/units, not UI DTOs).
    pub details: Vec<String>,
}

/// Read-only kernel and process snapshot consumed by the pure classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoLivePreflightSnapshot {
    /// Whether both Armoury attributes exist.
    pub armoury_supported: bool,
    /// Whether `dgpu_disable` accepts 0 and 1.
    pub dgpu_disable_values: Option<Vec<u32>>,
    /// Current dGPU disable state.
    pub dgpu_disabled: Option<bool>,
    /// Current physical MUX state.
    pub mux: Option<GpuMuxState>,
    /// Current access policy.
    pub access: Option<GpuAccessPolicy>,
    /// At least one NVIDIA PCI function is present.
    pub nvidia_pci_present: bool,
    /// Runtime status of observed NVIDIA functions.
    pub nvidia_runtime_status: Vec<String>,
    /// NVIDIA module presence and refcount evidence.
    pub nvidia_modules_loaded: bool,
    /// Module refcount evidence; `None` means unreadable.
    pub nvidia_module_refcount: Option<u64>,
    /// Whether module state is known busy.
    pub nvidia_module_busy: Option<bool>,
    /// Holder summaries.
    pub holders: Vec<NvidiaHolderEvidence>,
    /// A non-NVIDIA DRM card/render device exists.
    pub integrated_drm_present: bool,
    /// Fresh supergfxd snapshot.
    pub supergfxd: Option<SupergfxdSnapshot>,
    /// Read errors and unknown evidence.
    pub unknown: Vec<String>,
}

/// Final read-only feasibility verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EcoLiveReadiness {
    /// All currently observable preconditions pass.
    Ready,
    /// One or more typed blockers exist.
    Blocked {
        /// Stable blocker categories.
        reasons: Vec<EcoLiveBlocker>,
        /// Supporting diagnostics.
        evidence: Vec<EcoDiagnosticEvidence>,
    },
    /// Required firmware capability is absent.
    Unsupported(Vec<String>),
    /// State sources contradict one another.
    Inconsistent(Vec<String>),
}

fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

/// Pure, conservative classification of a read-only snapshot.
pub fn classify_native_asus_eco(snapshot: &EcoLivePreflightSnapshot) -> EcoLiveReadiness {
    if !snapshot.armoury_supported {
        return EcoLiveReadiness::Unsupported(vec![
            "ASUS Armoury dgpu_disable/gpu_mux_mode ABI".into(),
        ]);
    }
    if snapshot.dgpu_disable_values.as_deref() != Some(&[0, 1]) {
        return EcoLiveReadiness::Unsupported(vec!["dgpu_disable enumeration 0;1".into()]);
    }

    let mut inconsistent = snapshot.unknown.clone();
    if snapshot.dgpu_disabled.is_none() || snapshot.mux.is_none() || snapshot.access.is_none() {
        inconsistent.push("Armoury primitive read is incomplete".into());
    }
    if let Some(supergfxd) = &snapshot.supergfxd {
        if matches!(supergfxd.current_mode, SupergfxdMode::Unknown(_))
            || matches!(supergfxd.pending_mode, SupergfxdMode::Unknown(_))
            || matches!(
                supergfxd.pending_user_action,
                SupergfxdUserAction::Unknown(_)
            )
        {
            inconsistent.push("unknown supergfxd wire state".into());
        }
        if supergfxd.pending_mode != SupergfxdMode::None
            || supergfxd.pending_user_action != SupergfxdUserAction::Nothing
        {
            let mut reasons = vec![EcoLiveBlocker::PendingGpuMutation];
            return EcoLiveReadiness::Blocked {
                reasons: std::mem::take(&mut reasons),
                evidence: vec![EcoDiagnosticEvidence {
                    category: EcoLiveBlocker::PendingGpuMutation,
                    detail: format!(
                        "supergfxd pending={:?} action={:?}",
                        supergfxd.pending_mode, supergfxd.pending_user_action
                    ),
                }],
            };
        }
        if (supergfxd.current_mode == SupergfxdMode::Integrated
            && snapshot.dgpu_disabled == Some(false))
            || (supergfxd.current_mode == SupergfxdMode::Hybrid
                && snapshot.dgpu_disabled == Some(true))
        {
            inconsistent.push("supergfxd mode contradicts dgpu_disable".into());
        }
    } else {
        inconsistent.push("supergfxd snapshot unavailable".into());
    }
    if !inconsistent.is_empty() {
        return EcoLiveReadiness::Inconsistent(inconsistent);
    }

    let mut reasons = Vec::new();
    let mut evidence = Vec::new();
    for holder in &snapshot.holders {
        if holder.count == 0 {
            continue;
        }
        let category = match holder.kind {
            NvidiaHolderKind::Device => EcoLiveBlocker::NvidiaDeviceUser,
            NvidiaHolderKind::Drm => EcoLiveBlocker::NvidiaDrmUser,
            NvidiaHolderKind::I2c => EcoLiveBlocker::NvidiaI2cUser,
            NvidiaHolderKind::MappedLibrary => EcoLiveBlocker::NvidiaMappedLibrary,
        };
        push_unique(&mut reasons, category);
        evidence.push(EcoDiagnosticEvidence {
            category,
            detail: format!("{} holder(s): {:?}", holder.count, holder.details),
        });
    }
    if snapshot.nvidia_module_busy != Some(false) {
        reasons.push(EcoLiveBlocker::NvidiaModuleBusy);
        evidence.push(EcoDiagnosticEvidence {
            category: EcoLiveBlocker::NvidiaModuleBusy,
            detail: format!(
                "loaded={} refcount={:?} busy={:?}",
                snapshot.nvidia_modules_loaded,
                snapshot.nvidia_module_refcount,
                snapshot.nvidia_module_busy
            ),
        });
    }
    if snapshot.mux == Some(GpuMuxState::Discrete) {
        reasons.push(EcoLiveBlocker::MuxDiscrete);
        evidence.push(EcoDiagnosticEvidence {
            category: EcoLiveBlocker::MuxDiscrete,
            detail: "MUX is routed to the dGPU".into(),
        });
    }
    if !snapshot.integrated_drm_present {
        reasons.push(EcoLiveBlocker::MissingIntegratedDrm);
        evidence.push(EcoDiagnosticEvidence {
            category: EcoLiveBlocker::MissingIntegratedDrm,
            detail: "no non-NVIDIA DRM device detected".into(),
        });
    }
    if snapshot.access != Some(GpuAccessPolicy::Unblocked) || !snapshot.nvidia_pci_present {
        reasons.push(EcoLiveBlocker::BackendStateConflict);
        evidence.push(EcoDiagnosticEvidence {
            category: EcoLiveBlocker::BackendStateConflict,
            detail: format!(
                "access={:?} nvidia_pci_present={}",
                snapshot.access, snapshot.nvidia_pci_present
            ),
        });
    }
    if reasons.is_empty() {
        EcoLiveReadiness::Ready
    } else {
        EcoLiveReadiness::Blocked { reasons, evidence }
    }
}

/// Source used by the read-only provider; mutation methods intentionally do not exist.
#[async_trait]
pub trait NativeAsusEcoPreflightSource: Send + Sync {
    /// Collect a fresh snapshot.
    async fn read_snapshot(&self) -> Result<EcoLivePreflightSnapshot, ProviderError>;
}

/// Read-only provider facade.
pub struct NativeAsusEcoPreflightProvider<S> {
    source: S,
}

impl<S> NativeAsusEcoPreflightProvider<S> {
    /// Construct without I/O.
    pub fn new(source: S) -> Self {
        Self { source }
    }
    /// Read and classify a fresh snapshot.
    pub async fn readiness(&self) -> Result<EcoLiveReadiness, ProviderError>
    where
        S: NativeAsusEcoPreflightSource,
    {
        Ok(classify_native_asus_eco(
            &self.source.read_snapshot().await?,
        ))
    }
    /// Read a fresh snapshot for diagnostics.
    pub async fn snapshot(&self) -> Result<EcoLivePreflightSnapshot, ProviderError>
    where
        S: NativeAsusEcoPreflightSource,
    {
        self.source.read_snapshot().await
    }
}

/// Read-only host source. It intentionally does not invoke NVML: waking/side effects
/// are not required to establish this preflight's conservative evidence.
pub struct SystemNativeAsusEcoPreflightSource {
    root: PathBuf,
}

impl Default for SystemNativeAsusEcoPreflightSource {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }
}

impl SystemNativeAsusEcoPreflightSource {
    /// Construct with a filesystem root, primarily for deterministic tests/tools.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    fn path(&self, p: &str) -> PathBuf {
        self.root.join(p.trim_start_matches('/'))
    }
    fn read(&self, p: &str) -> Result<String, ProviderError> {
        std::fs::read_to_string(self.path(p)).map_err(ProviderError::Io)
    }
    fn optional(&self, p: &str) -> Option<String> {
        self.read(p).ok().map(|v| v.trim().to_owned())
    }

    async fn read_supergfxd(&self) -> Result<SupergfxdSnapshot, ProviderError> {
        let connection = zbus::Connection::system().await.map_err(supergfxd_error)?;
        let proxy = SupergfxdReadOnlyProxy::new(&connection)
            .await
            .map_err(supergfxd_error)?;
        Ok(SupergfxdSnapshot {
            current_mode: SupergfxdMode::from_wire(proxy.mode().await.map_err(supergfxd_error)?),
            pending_mode: SupergfxdMode::from_wire(
                proxy.pending_mode().await.map_err(supergfxd_error)?,
            ),
            pending_user_action: SupergfxdUserAction::from_wire(
                proxy.pending_user_action().await.map_err(supergfxd_error)?,
            ),
            power: power_from_wire(proxy.power().await.map_err(supergfxd_error)?),
            supported_modes: proxy
                .supported()
                .await
                .map_err(supergfxd_error)?
                .into_iter()
                .map(SupergfxdMode::from_wire)
                .collect(),
        })
    }
}

fn parse_u32(s: Option<String>) -> Option<u32> {
    s?.parse().ok()
}

#[async_trait]
impl NativeAsusEcoPreflightSource for SystemNativeAsusEcoPreflightSource {
    async fn read_snapshot(&self) -> Result<EcoLivePreflightSnapshot, ProviderError> {
        let base = "/sys/class/firmware-attributes/asus-armoury/attributes";
        let dgpu_path = format!("{base}/dgpu_disable/current_value");
        let mux_path = format!("{base}/gpu_mux_mode/current_value");
        let dgpu = parse_u32(self.optional(&dgpu_path));
        let mux = parse_u32(self.optional(&mux_path));
        let supported = dgpu.is_some() && mux.is_some();
        let values = self
            .optional(&format!("{base}/dgpu_disable/possible_values"))
            .map(|s| s.split(';').filter_map(|v| v.parse().ok()).collect());
        let mut nvidia_pci = false;
        let mut runtimes = Vec::new();
        let mut integrated_drm = false;
        let drm_root = self.path("sys/class/drm");
        if let Ok(entries) = std::fs::read_dir(drm_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with("card") || name.contains('-') {
                    continue;
                }
                let vendor =
                    std::fs::read_to_string(entry.path().join("device/vendor")).unwrap_or_default();
                if vendor.trim() != "0x10de" {
                    integrated_drm = true;
                }
            }
        }
        let pci_root = self.path("sys/bus/pci/devices");
        if let Ok(entries) = std::fs::read_dir(pci_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if std::fs::read_to_string(path.join("vendor"))
                    .ok()
                    .as_deref()
                    .map(str::trim)
                    != Some("0x10de")
                {
                    continue;
                }
                nvidia_pci = true;
                if let Ok(status) = std::fs::read_to_string(path.join("power/runtime_status")) {
                    runtimes.push(status.trim().into());
                }
            }
        }
        let modules_loaded = self.path("sys/module/nvidia").exists();
        let refcount = parse_u32(self.optional("sys/module/nvidia/refcnt")).map(u64::from);
        let mut holders = Vec::new();
        let mut device = Vec::new();
        let mut drm = Vec::new();
        let mut i2c = Vec::new();
        let mut maps = Vec::new();
        let mut nvidia_drm_nodes = HashSet::new();
        let mut nvidia_i2c_nodes = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(self.path("sys/class/drm")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with("card") && !name.starts_with("renderD") {
                    continue;
                }
                let vendor =
                    std::fs::read_to_string(entry.path().join("device/vendor")).unwrap_or_default();
                if vendor.trim() == "0x10de" {
                    nvidia_drm_nodes.insert(format!("/dev/dri/{name}"));
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(self.path("sys/bus/i2c/devices")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with("i2c-") {
                    continue;
                }
                let parent_vendor = entry
                    .path()
                    .canonicalize()
                    .ok()
                    .and_then(|path| path.parent().map(PathBuf::from))
                    .and_then(|parent| std::fs::read_to_string(parent.join("vendor")).ok());
                if parent_vendor.as_deref().map(str::trim) == Some("0x10de") {
                    nvidia_i2c_nodes.insert(format!("/dev/{name}"));
                }
            }
        }
        let proc_root = self.path("proc");
        if let Ok(entries) = std::fs::read_dir(proc_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.parse::<u32>().is_err() {
                    continue;
                }
                let pid = name.parse::<u32>().unwrap();
                let fd_root = entry.path().join("fd");
                if let Ok(fds) = std::fs::read_dir(fd_root) {
                    for fd in fds.flatten() {
                        if let Ok(target) = std::fs::read_link(fd.path()) {
                            let t = target.to_string_lossy();
                            if t.starts_with("/dev/nvidia") {
                                device.push(pid.to_string());
                            } else if nvidia_drm_nodes.contains(t.as_ref()) {
                                drm.push(pid.to_string());
                            } else if nvidia_i2c_nodes.contains(t.as_ref()) {
                                i2c.push(pid.to_string());
                            }
                        }
                    }
                }
                if let Ok(content) = std::fs::read_to_string(entry.path().join("maps")) {
                    if content.lines().any(|l| {
                        l.contains("/dev/nvidia")
                            || l.contains("libnvidia-")
                            || l.contains("libcuda.so")
                    }) {
                        maps.push(pid.to_string());
                    }
                }
            }
        }
        for (kind, list) in [
            (NvidiaHolderKind::Device, device),
            (NvidiaHolderKind::Drm, drm),
            (NvidiaHolderKind::I2c, i2c),
            (NvidiaHolderKind::MappedLibrary, maps),
        ] {
            if !list.is_empty() {
                holders.push(NvidiaHolderEvidence {
                    kind,
                    count: list.len(),
                    details: list,
                });
            }
        }
        let supergfxd = self.read_supergfxd().await?;
        Ok(EcoLivePreflightSnapshot {
            armoury_supported: supported,
            dgpu_disable_values: values,
            dgpu_disabled: dgpu.map(|v| v == 1),
            mux: mux.map(|v| {
                if v == 0 {
                    GpuMuxState::Discrete
                } else if v == 1 {
                    GpuMuxState::Integrated
                } else {
                    GpuMuxState::Unknown
                }
            }),
            access: dgpu.map(|v| {
                if v == 0 {
                    GpuAccessPolicy::Unblocked
                } else {
                    GpuAccessPolicy::Blocked
                }
            }),
            nvidia_pci_present: nvidia_pci,
            nvidia_runtime_status: runtimes,
            nvidia_modules_loaded: modules_loaded,
            nvidia_module_refcount: refcount,
            nvidia_module_busy: refcount.map(|v| v != 0),
            holders,
            integrated_drm_present: integrated_drm,
            supergfxd: Some(supergfxd),
            unknown: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> EcoLivePreflightSnapshot {
        EcoLivePreflightSnapshot {
            armoury_supported: true,
            dgpu_disable_values: Some(vec![0, 1]),
            dgpu_disabled: Some(false),
            mux: Some(GpuMuxState::Integrated),
            access: Some(GpuAccessPolicy::Unblocked),
            nvidia_pci_present: true,
            nvidia_runtime_status: vec!["suspended".into()],
            nvidia_modules_loaded: true,
            nvidia_module_refcount: Some(0),
            nvidia_module_busy: Some(false),
            holders: vec![],
            integrated_drm_present: true,
            supergfxd: Some(SupergfxdSnapshot {
                current_mode: SupergfxdMode::Hybrid,
                pending_mode: SupergfxdMode::None,
                pending_user_action: SupergfxdUserAction::Nothing,
                power: orbis_core::gpu::GpuPowerState::Suspended,
                supported_modes: vec![SupergfxdMode::Hybrid, SupergfxdMode::Integrated],
            }),
            unknown: vec![],
        }
    }
    #[test]
    fn ready() {
        assert_eq!(classify_native_asus_eco(&base()), EcoLiveReadiness::Ready);
    }
    #[test]
    fn each_holder_blocks() {
        for kind in [
            NvidiaHolderKind::Device,
            NvidiaHolderKind::Drm,
            NvidiaHolderKind::I2c,
            NvidiaHolderKind::MappedLibrary,
        ] {
            let mut s = base();
            s.holders = vec![NvidiaHolderEvidence {
                kind,
                count: 1,
                details: vec!["p".into()],
            }];
            assert!(matches!(
                classify_native_asus_eco(&s),
                EcoLiveReadiness::Blocked { .. }
            ));
        }
    }
    #[test]
    fn module_mux_display_pending_and_conflict_block_or_inconsistent() {
        let mut s = base();
        s.nvidia_module_busy = Some(true);
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Blocked { .. }
        ));
        let mut s = base();
        s.mux = Some(GpuMuxState::Discrete);
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Blocked { .. }
        ));
        let mut s = base();
        s.integrated_drm_present = false;
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Blocked { .. }
        ));
        let mut s = base();
        s.supergfxd.as_mut().unwrap().pending_mode = SupergfxdMode::Integrated;
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Blocked { .. }
        ));
        let mut s = base();
        s.supergfxd.as_mut().unwrap().current_mode = SupergfxdMode::Integrated;
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Inconsistent(_)
        ));
    }
    #[test]
    fn unsupported_and_unknown_are_conservative() {
        let mut s = base();
        s.armoury_supported = false;
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Unsupported(_)
        ));
        let mut s = base();
        s.unknown.push("x".into());
        assert!(matches!(
            classify_native_asus_eco(&s),
            EcoLiveReadiness::Inconsistent(_)
        ));
    }
}
