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

/// Severity of read-only evidence for a native live Eco transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcoEvidenceSeverity {
    /// The observed state makes the transition unsafe or impossible.
    HardBlocker,
    /// A typed release/verification phase is required before the transition.
    ReleaseRequired,
    /// Evidence is useful but does not prevent a transition by itself.
    Informational,
    /// The evidence needed to make a safe decision is unavailable.
    Unknown,
}

/// Why a native live Eco transition is blocked or needs release work.
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
    /// The NVIDIA module stack is still loaded and must be released.
    NvidiaModulesLoaded,
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

/// Typed operation required by a pure native Eco transition plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcoReleaseRequirement {
    /// Release userspace processes with an NVIDIA device workload.
    ReleaseApplicationGpuUsers,
    /// Release a secondary NVIDIA DRM card reference.
    ReleaseSecondaryDrmDevice,
    /// Re-scan users and verify that no NVIDIA users remain.
    VerifyNvidiaUsers,
    /// Verify that the NVIDIA module family can be unloaded.
    VerifyNvidiaModuleUnload,
    /// Unload the loaded NVIDIA module family.
    UnloadNvidiaModules,
    /// Verify that the compositor released all NVIDIA DRM/core users.
    VerifyCompositorRelease,
}

/// Pure transition plan. It contains capabilities, never shell commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EcoTransitionPlan {
    /// No release or unresolved verification is present in the snapshot.
    ReadyImmediately,
    /// The state can be prepared by a future release executor.
    CanBecomeReady {
        /// Typed release/verification requirements.
        release_requirements: Vec<EcoReleaseRequirement>,
        /// Evidence that must be resolved before mutation is allowed.
        unresolved: Vec<String>,
    },
    /// The graphical session must be ended before proceeding.
    RequiresLogout(String),
    /// The transition cannot be completed until reboot.
    RequiresReboot(String),
    /// The required capability is absent.
    Unsupported(Vec<String>),
    /// Hardware/backend sources contradict one another.
    Inconsistent(Vec<String>),
}

/// Read-only diagnostic evidence kept separate from the domain verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoDiagnosticEvidence {
    /// Severity assigned by the pure planner.
    pub severity: EcoEvidenceSeverity,
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
    /// NVIDIA-owned DRM card or render node in `/dev/dri/*`.
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
    /// Whether a prior, authoritative unload-feasibility result exists.
    /// `None` means no unload was attempted or the result is unavailable.
    pub nvidia_module_unload_feasible: Option<bool>,
    /// Holder summaries.
    pub holders: Vec<NvidiaHolderEvidence>,
    /// A non-NVIDIA DRM card/render device exists.
    pub integrated_drm_present: bool,
    /// Whether the current compositor can release all NVIDIA DRM/core users
    /// live. `None` is intentionally unresolved, not success. A proven
    /// card-minor release does not imply that render/core users are gone.
    pub compositor_release_supported: Option<bool>,
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

/// Pure assessment containing severity buckets and the resulting plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoLiveAssessment {
    /// Hard blockers.
    pub hard_blockers: Vec<EcoDiagnosticEvidence>,
    /// Release work required before mutation.
    pub release_required: Vec<EcoDiagnosticEvidence>,
    /// Non-blocking observations.
    pub informational: Vec<EcoDiagnosticEvidence>,
    /// Missing critical evidence.
    pub unknown: Vec<EcoDiagnosticEvidence>,
    /// Pure transition result.
    pub plan: EcoTransitionPlan,
}

fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

/// Pure, conservative classification of a read-only snapshot.
pub fn classify_native_asus_eco(snapshot: &EcoLivePreflightSnapshot) -> EcoLiveReadiness {
    let assessment = plan_native_asus_eco(snapshot);
    if let EcoTransitionPlan::Unsupported(reasons) = &assessment.plan {
        return EcoLiveReadiness::Unsupported(reasons.clone());
    }
    if let EcoTransitionPlan::Inconsistent(reasons) = &assessment.plan {
        return EcoLiveReadiness::Inconsistent(reasons.clone());
    }
    let mut reasons = Vec::new();
    let mut evidence = Vec::new();
    for item in assessment
        .hard_blockers
        .iter()
        .chain(assessment.release_required.iter())
    {
        let category = item.category;
        push_unique(&mut reasons, category);
        evidence.push(item.clone());
    }
    if reasons.is_empty() && assessment.unknown.is_empty() {
        EcoLiveReadiness::Ready
    } else {
        evidence.extend(assessment.unknown);
        EcoLiveReadiness::Blocked { reasons, evidence }
    }
}

/// Purely classify evidence and derive a transition plan.
pub fn plan_native_asus_eco(snapshot: &EcoLivePreflightSnapshot) -> EcoLiveAssessment {
    let mut hard_blockers = Vec::new();
    let mut release_required = Vec::new();
    let mut informational = Vec::new();
    let mut unknown = Vec::new();
    let mut inconsistent = snapshot.unknown.clone();

    let evidence = |severity, category, detail: String| EcoDiagnosticEvidence {
        severity,
        category,
        detail,
    };
    let add_holder = |holder: &NvidiaHolderEvidence,
                      release_required: &mut Vec<EcoDiagnosticEvidence>,
                      informational: &mut Vec<EcoDiagnosticEvidence>| {
        if holder.count == 0 {
            return;
        }
        let category = match holder.kind {
            NvidiaHolderKind::Device => EcoLiveBlocker::NvidiaDeviceUser,
            NvidiaHolderKind::Drm => EcoLiveBlocker::NvidiaDrmUser,
            NvidiaHolderKind::I2c => EcoLiveBlocker::NvidiaI2cUser,
            NvidiaHolderKind::MappedLibrary => EcoLiveBlocker::NvidiaMappedLibrary,
        };
        let detail = format!("{} holder(s): {:?}", holder.count, holder.details);
        match holder.kind {
            NvidiaHolderKind::MappedLibrary => informational.push(evidence(
                EcoEvidenceSeverity::Informational,
                category,
                detail,
            )),
            NvidiaHolderKind::Device | NvidiaHolderKind::Drm | NvidiaHolderKind::I2c => {
                release_required.push(evidence(
                    EcoEvidenceSeverity::ReleaseRequired,
                    category,
                    detail,
                ));
            }
        }
    };

    if !snapshot.armoury_supported {
        return EcoLiveAssessment {
            hard_blockers,
            release_required,
            informational,
            unknown,
            plan: EcoTransitionPlan::Unsupported(vec![
                "ASUS Armoury dgpu_disable/gpu_mux_mode ABI".into(),
            ]),
        };
    }
    if snapshot.dgpu_disable_values.as_deref() != Some(&[0, 1]) {
        return EcoLiveAssessment {
            hard_blockers,
            release_required,
            informational,
            unknown,
            plan: EcoTransitionPlan::Unsupported(vec!["dgpu_disable enumeration 0;1".into()]),
        };
    }
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
            hard_blockers.push(evidence(
                EcoEvidenceSeverity::HardBlocker,
                EcoLiveBlocker::PendingGpuMutation,
                format!(
                    "supergfxd pending={:?} action={:?}",
                    supergfxd.pending_mode, supergfxd.pending_user_action
                ),
            ));
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
    for holder in &snapshot.holders {
        add_holder(holder, &mut release_required, &mut informational);
    }
    if snapshot.nvidia_module_refcount.is_some() || snapshot.nvidia_modules_loaded {
        informational.push(evidence(
            EcoEvidenceSeverity::Informational,
            EcoLiveBlocker::NvidiaModuleBusy,
            format!(
                "loaded={} refcount={:?} busy={:?}",
                snapshot.nvidia_modules_loaded,
                snapshot.nvidia_module_refcount,
                snapshot.nvidia_module_busy
            ),
        ));
    }
    if snapshot.nvidia_modules_loaded {
        release_required.push(evidence(
            EcoEvidenceSeverity::ReleaseRequired,
            EcoLiveBlocker::NvidiaModulesLoaded,
            "NVIDIA module stack is loaded; unload and verify before firmware transition".into(),
        ));
    }
    if snapshot.nvidia_modules_loaded && snapshot.nvidia_module_unload_feasible != Some(true) {
        unknown.push(evidence(
            EcoEvidenceSeverity::Unknown,
            EcoLiveBlocker::Unknown,
            format!(
                "NVIDIA module unload feasibility is unresolved: {:?}",
                snapshot.nvidia_module_unload_feasible
            ),
        ));
    }
    if snapshot.mux == Some(GpuMuxState::Discrete) {
        hard_blockers.push(evidence(
            EcoEvidenceSeverity::HardBlocker,
            EcoLiveBlocker::MuxDiscrete,
            "MUX is routed to the dGPU".into(),
        ));
    }
    if !snapshot.integrated_drm_present {
        hard_blockers.push(evidence(
            EcoEvidenceSeverity::HardBlocker,
            EcoLiveBlocker::MissingIntegratedDrm,
            "no non-NVIDIA DRM device detected".into(),
        ));
    }
    if snapshot.access != Some(GpuAccessPolicy::Unblocked) || !snapshot.nvidia_pci_present {
        hard_blockers.push(evidence(
            EcoEvidenceSeverity::HardBlocker,
            EcoLiveBlocker::BackendStateConflict,
            format!(
                "access={:?} nvidia_pci_present={}",
                snapshot.access, snapshot.nvidia_pci_present
            ),
        ));
    }

    if snapshot.compositor_release_supported != Some(true) && !release_required.is_empty() {
        unknown.push(evidence(
            EcoEvidenceSeverity::Unknown,
            EcoLiveBlocker::Unknown,
            "supported live compositor DRM release is unresolved".into(),
        ));
    }

    if !inconsistent.is_empty() {
        return EcoLiveAssessment {
            hard_blockers,
            release_required,
            informational,
            unknown,
            plan: EcoTransitionPlan::Inconsistent(inconsistent),
        };
    }
    let mut requirements = Vec::new();
    if release_required
        .iter()
        .any(|e| e.category == EcoLiveBlocker::NvidiaDeviceUser)
    {
        requirements.push(EcoReleaseRequirement::ReleaseApplicationGpuUsers);
    }
    if release_required
        .iter()
        .any(|e| e.category == EcoLiveBlocker::NvidiaDrmUser)
    {
        requirements.push(EcoReleaseRequirement::ReleaseSecondaryDrmDevice);
    }
    if release_required
        .iter()
        .any(|e| e.category == EcoLiveBlocker::NvidiaModulesLoaded)
    {
        requirements.push(EcoReleaseRequirement::UnloadNvidiaModules);
    }
    if !release_required.is_empty() {
        requirements.push(EcoReleaseRequirement::VerifyNvidiaUsers);
        requirements.push(EcoReleaseRequirement::VerifyNvidiaModuleUnload);
    }
    if !unknown.is_empty() {
        requirements.push(EcoReleaseRequirement::VerifyCompositorRelease);
    }
    let plan = if hard_blockers
        .iter()
        .any(|e| e.category == EcoLiveBlocker::MuxDiscrete)
    {
        EcoTransitionPlan::RequiresReboot("MUX is routed to the dGPU".into())
    } else if hard_blockers
        .iter()
        .any(|e| e.category == EcoLiveBlocker::MissingIntegratedDrm)
    {
        EcoTransitionPlan::Unsupported(vec!["no non-NVIDIA DRM device detected".into()])
    } else if !hard_blockers.is_empty() {
        EcoTransitionPlan::CanBecomeReady {
            release_requirements: requirements,
            unresolved: hard_blockers.iter().map(|e| e.detail.clone()).collect(),
        }
    } else if release_required.is_empty() && unknown.is_empty() {
        EcoTransitionPlan::ReadyImmediately
    } else {
        EcoTransitionPlan::CanBecomeReady {
            release_requirements: requirements,
            unresolved: unknown.iter().map(|e| e.detail.clone()).collect(),
        }
    };
    EcoLiveAssessment {
        hard_blockers,
        release_required,
        informational,
        unknown,
        plan,
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
            nvidia_module_unload_feasible: None,
            holders,
            integrated_drm_present: integrated_drm,
            compositor_release_supported: None,
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
            nvidia_modules_loaded: false,
            nvidia_module_refcount: None,
            nvidia_module_busy: Some(false),
            nvidia_module_unload_feasible: None,
            holders: vec![],
            integrated_drm_present: true,
            compositor_release_supported: None,
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
        assert!(matches!(
            plan_native_asus_eco(&base()).plan,
            EcoTransitionPlan::ReadyImmediately
        ));
    }
    fn with_holder(kind: NvidiaHolderKind) -> EcoLivePreflightSnapshot {
        let mut s = base();
        s.holders = vec![NvidiaHolderEvidence {
            kind,
            count: 1,
            details: vec!["p".into()],
        }];
        s
    }
    #[test]
    fn library_and_module_evidence_are_not_hard_blockers() {
        let s = with_holder(NvidiaHolderKind::MappedLibrary);
        let assessment = plan_native_asus_eco(&s);
        assert!(assessment.hard_blockers.is_empty());
        assert!(assessment.release_required.is_empty());
        assert!(matches!(
            assessment.plan,
            EcoTransitionPlan::ReadyImmediately
        ));

        let mut s = base();
        s.nvidia_modules_loaded = true;
        s.nvidia_module_busy = Some(true);
        s.nvidia_module_refcount = Some(152);
        let assessment = plan_native_asus_eco(&s);
        assert!(assessment.hard_blockers.is_empty());
        assert!(
            assessment
                .informational
                .iter()
                .any(|e| e.category == EcoLiveBlocker::NvidiaModuleBusy)
        );
        assert!(matches!(
            assessment.plan,
            EcoTransitionPlan::CanBecomeReady { .. }
        ));
        assert!(
            assessment
                .release_required
                .iter()
                .any(|e| e.category == EcoLiveBlocker::NvidiaModulesLoaded)
        );
    }
    #[test]
    fn loaded_modules_require_unload_even_with_zero_refcount() {
        let mut s = base();
        s.nvidia_modules_loaded = true;
        s.nvidia_module_refcount = Some(0);
        s.nvidia_module_busy = Some(false);
        let assessment = plan_native_asus_eco(&s);
        assert!(matches!(
            assessment.plan,
            EcoTransitionPlan::CanBecomeReady { .. }
        ));
        let plan = match assessment.plan {
            EcoTransitionPlan::CanBecomeReady {
                release_requirements,
                ..
            } => release_requirements,
            _ => unreachable!(),
        };
        assert_eq!(plan[0], EcoReleaseRequirement::UnloadNvidiaModules);
        assert_eq!(plan[1], EcoReleaseRequirement::VerifyNvidiaUsers);
        assert_eq!(plan[2], EcoReleaseRequirement::VerifyNvidiaModuleUnload);
    }
    #[test]
    fn unload_feasibility_unknown_is_fail_closed() {
        let mut s = base();
        s.nvidia_modules_loaded = true;
        s.nvidia_module_unload_feasible = None;
        let assessment = plan_native_asus_eco(&s);
        assert!(!assessment.unknown.is_empty());
        assert!(!matches!(
            assessment.plan,
            EcoTransitionPlan::ReadyImmediately
        ));
    }
    #[test]
    fn absent_modules_allow_immediate_ready() {
        let assessment = plan_native_asus_eco(&base());
        assert!(matches!(
            assessment.plan,
            EcoTransitionPlan::ReadyImmediately
        ));
    }
    #[test]
    fn users_require_release() {
        for kind in [
            NvidiaHolderKind::Device,
            NvidiaHolderKind::Drm,
            NvidiaHolderKind::I2c,
        ] {
            let assessment = plan_native_asus_eco(&with_holder(kind));
            assert!(assessment.hard_blockers.is_empty());
            assert!(!assessment.release_required.is_empty());
            assert!(matches!(
                assessment.plan,
                EcoTransitionPlan::CanBecomeReady { .. }
            ));
        }
    }
    #[test]
    fn module_mux_display_pending_and_conflict_block_or_inconsistent() {
        let mut s = base();
        s.nvidia_module_busy = Some(true);
        assert_eq!(classify_native_asus_eco(&s), EcoLiveReadiness::Ready);
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
            EcoLiveReadiness::Unsupported(_)
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
    fn unknown_compositor_release_is_not_immediate_ready() {
        let mut s = with_holder(NvidiaHolderKind::Drm);
        s.compositor_release_supported = None;
        let assessment = plan_native_asus_eco(&s);
        assert!(!assessment.unknown.is_empty());
        assert!(matches!(
            assessment.plan,
            EcoTransitionPlan::CanBecomeReady { .. }
        ));
    }

    #[test]
    fn card_release_does_not_imply_compositor_release() {
        let mut s = with_holder(NvidiaHolderKind::Device);
        s.compositor_release_supported = None;
        let assessment = plan_native_asus_eco(&s);

        assert!(
            assessment
                .release_required
                .iter()
                .any(|e| e.category == EcoLiveBlocker::NvidiaDeviceUser)
        );
        assert!(
            assessment
                .unknown
                .iter()
                .any(|e| e.category == EcoLiveBlocker::Unknown)
        );
        let requirements = match &assessment.plan {
            EcoTransitionPlan::CanBecomeReady {
                release_requirements,
                ..
            } => release_requirements,
            _ => unreachable!(),
        };
        assert!(requirements.contains(&EcoReleaseRequirement::VerifyCompositorRelease));
        assert!(!matches!(
            assessment.plan,
            EcoTransitionPlan::ReadyImmediately | EcoTransitionPlan::RequiresLogout(_)
        ));
    }

    #[test]
    fn remaining_render_and_core_users_stay_fail_closed_without_logout_claim() {
        let mut s = base();
        s.holders = vec![
            NvidiaHolderEvidence {
                kind: NvidiaHolderKind::Drm,
                count: 2,
                details: vec!["renderD129".into()],
            },
            NvidiaHolderEvidence {
                kind: NvidiaHolderKind::Device,
                count: 4,
                details: vec!["nvidiactl/nvidia0".into()],
            },
        ];
        s.compositor_release_supported = Some(true);
        let assessment = plan_native_asus_eco(&s);

        assert!(
            assessment
                .release_required
                .iter()
                .any(|e| e.category == EcoLiveBlocker::NvidiaDrmUser)
        );
        assert!(
            assessment
                .release_required
                .iter()
                .any(|e| e.category == EcoLiveBlocker::NvidiaDeviceUser)
        );
        assert!(!matches!(
            assessment.plan,
            EcoTransitionPlan::ReadyImmediately | EcoTransitionPlan::RequiresLogout(_)
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
