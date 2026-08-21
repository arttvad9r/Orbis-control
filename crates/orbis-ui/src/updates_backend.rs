//! Orbis application update backend.
//!
//! This is deliberately not the device-firmware `FirmwareUpdateProvider`.
//! The Updates window represents the Orbis application/package itself. The
//! repository currently has no canonical signed release source and no single
//! installation mutation owner, so check/install remain fail-closed. The
//! blocker is typed rather than represented only by a disabled button/string.
//!
//! No subprocess, package-manager command, downloader or self-replacing binary
//! path is exposed here.

use std::path::{Path, PathBuf};

use slint::ComponentHandle;

use crate::UpdatesWindow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallOwner {
    NixStore,
    AppImage(PathBuf),
    SystemPrefix(PathBuf),
    Development(PathBuf),
    Unknown(PathBuf),
}

impl InstallOwner {
    fn label(&self) -> &'static str {
        match self {
            Self::NixStore => "Nix store",
            Self::AppImage(_) => "AppImage",
            Self::SystemPrefix(_) => "system package",
            Self::Development(_) => "development build",
            Self::Unknown(_) => "unknown installation",
        }
    }

    fn ownership_message(&self) -> &'static str {
        match self {
            Self::NixStore => "Installed from the Nix store · updates are owned by Nix/NixOS",
            Self::AppImage(_) => {
                "Running as AppImage · no signed Orbis AppImage release source is configured"
            }
            Self::SystemPrefix(_) => {
                "Installed in a system prefix · package-manager ownership is not proven"
            }
            Self::Development(_) => {
                "Development build · in-app update installation is intentionally disabled"
            }
            Self::Unknown(_) => {
                "Installation owner is unknown · in-app update installation is disabled"
            }
        }
    }
}

/// Why a remote release check cannot be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReleaseSourceBlocker {
    /// No repository-owned URL/schema/signature trust root has been designated
    /// as the canonical Orbis application release feed.
    CanonicalSourceMissing,
}

impl ReleaseSourceBlocker {
    fn message(self) -> &'static str {
        match self {
            Self::CanonicalSourceMissing => {
                "No canonical signed Orbis application release source is configured"
            }
        }
    }
}

/// Why a downloaded application update could not be installed by Orbis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallBlocker {
    NixOwnedExternally,
    AppImageInstallerMissing,
    SystemPackageOwnerUnproven,
    DevelopmentBuild,
    UnknownOwner,
}

impl InstallBlocker {
    fn for_owner(owner: &InstallOwner) -> Self {
        match owner {
            InstallOwner::NixStore => Self::NixOwnedExternally,
            InstallOwner::AppImage(_) => Self::AppImageInstallerMissing,
            InstallOwner::SystemPrefix(_) => Self::SystemPackageOwnerUnproven,
            InstallOwner::Development(_) => Self::DevelopmentBuild,
            InstallOwner::Unknown(_) => Self::UnknownOwner,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::NixOwnedExternally => "Installation is owned by Nix/NixOS",
            Self::AppImageInstallerMissing => {
                "No verified AppImage replacement/rollback owner is implemented"
            }
            Self::SystemPackageOwnerUnproven => {
                "The owning system package manager/package identity is not proven"
            }
            Self::DevelopmentBuild => "Development builds are not self-updated",
            Self::UnknownOwner => "Installation owner is unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateBackendAssessment {
    pub(crate) owner: InstallOwner,
    pub(crate) release_source_blocker: ReleaseSourceBlocker,
    pub(crate) install_blocker: InstallBlocker,
}

impl UpdateBackendAssessment {
    fn from_owner(owner: InstallOwner) -> Self {
        let install_blocker = InstallBlocker::for_owner(&owner);
        Self {
            owner,
            release_source_blocker: ReleaseSourceBlocker::CanonicalSourceMissing,
            install_blocker,
        }
    }

    pub(crate) fn can_check(&self) -> bool {
        false
    }

    pub(crate) fn can_install(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdatesUiState {
    pub(crate) backend_ready: bool,
    pub(crate) source_ready: bool,
    pub(crate) check_enabled: bool,
    pub(crate) install_enabled: bool,
    pub(crate) channel: i32,
    pub(crate) channel_enabled: bool,
    pub(crate) update_available: bool,
    pub(crate) latest_version: String,
    pub(crate) release_notes: String,
    pub(crate) status: String,
}

/// Determine who owns the currently running executable without executing an
/// external command or trusting model/package-name heuristics.
pub(crate) fn detect_install_owner() -> std::io::Result<InstallOwner> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let path = PathBuf::from(appimage);
        if path.is_absolute() {
            return Ok(InstallOwner::AppImage(path));
        }
    }

    let executable = std::env::current_exe()?;
    Ok(classify_executable(&executable))
}

fn classify_executable(path: &Path) -> InstallOwner {
    let text = path.to_string_lossy();
    if text.starts_with("/nix/store/") {
        return InstallOwner::NixStore;
    }
    if text.contains("/target/debug/") || text.contains("/target/release/") {
        return InstallOwner::Development(path.to_path_buf());
    }
    if text.starts_with("/usr/bin/")
        || text.starts_with("/usr/local/bin/")
        || text.starts_with("/opt/")
    {
        return InstallOwner::SystemPrefix(path.to_path_buf());
    }
    InstallOwner::Unknown(path.to_path_buf())
}

pub(crate) fn assess_backend() -> Result<UpdateBackendAssessment, std::io::Error> {
    detect_install_owner().map(UpdateBackendAssessment::from_owner)
}

pub(crate) fn read_state() -> Result<UpdatesUiState, std::io::Error> {
    let assessment = assess_backend()?;
    let label = assessment.owner.label();
    let ownership = assessment.owner.ownership_message();

    Ok(UpdatesUiState {
        // Local ownership detection is functional. Remote source and install
        // readiness are distinct capabilities and remain false while their
        // typed blockers are present.
        backend_ready: true,
        source_ready: assessment.can_check(),
        check_enabled: assessment.can_check(),
        install_enabled: assessment.can_install(),
        channel: 0,
        channel_enabled: false,
        update_available: false,
        latest_version: "—".into(),
        release_notes: format!(
            "{ownership}. {}. {}.",
            assessment.release_source_blocker.message(),
            assessment.install_blocker.message(),
        ),
        status: format!(
            "{label} detected · {}",
            assessment.release_source_blocker.message()
        ),
    })
}

fn apply_state(window: &UpdatesWindow, state: &UpdatesUiState) {
    window.set_backend_ready(state.backend_ready);
    window.set_source_ready(state.source_ready);
    window.set_check_enabled(state.check_enabled);
    window.set_install_enabled(state.install_enabled);
    window.set_channel(state.channel);
    window.set_channel_enabled(state.channel_enabled);
    window.set_update_available(state.update_available);
    window.set_latest_version(state.latest_version.clone().into());
    window.set_release_notes(state.release_notes.clone().into());
    window.set_status(state.status.clone().into());
}

pub(crate) fn refresh(window: &UpdatesWindow) {
    match read_state() {
        Ok(state) => apply_state(window, &state),
        Err(error) => {
            tracing::warn!(error = %error, "Orbis update ownership detection failed");
            window.set_backend_ready(false);
            window.set_source_ready(false);
            window.set_check_enabled(false);
            window.set_install_enabled(false);
            window.set_channel_enabled(false);
            window.set_update_available(false);
            window.set_latest_version("—".into());
            window.set_release_notes("Update ownership could not be determined".into());
            window.set_status("Update backend unavailable".into());
        }
    }
}

pub(crate) fn wire(window: &UpdatesWindow) {
    window.on_channel_requested(|channel| {
        tracing::warn!(
            channel,
            "update channel request ignored: no canonical signed Orbis release source is configured"
        );
    });

    {
        let weak = window.as_weak();
        window.on_check_requested(move || {
            tracing::warn!("update check ignored: canonical release source blocker is active");
            if let Some(window) = weak.upgrade() {
                refresh(&window);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_install_requested(move || {
            tracing::warn!("update install ignored: installation-owner blocker is active");
            if let Some(window) = weak.upgrade() {
                refresh(&window);
            }
        });
    }

    refresh(window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_nix_and_development_paths_without_guessing_package_manager() {
        assert_eq!(
            classify_executable(Path::new("/nix/store/abc-orbis/bin/orbis-control")),
            InstallOwner::NixStore
        );
        assert!(matches!(
            classify_executable(Path::new("/work/Orbis-control/target/debug/orbis-control")),
            InstallOwner::Development(_)
        ));
        assert!(matches!(
            classify_executable(Path::new("/usr/bin/orbis-control")),
            InstallOwner::SystemPrefix(_)
        ));
    }

    #[test]
    fn blockers_are_typed_per_installation_owner() {
        let nix = UpdateBackendAssessment::from_owner(InstallOwner::NixStore);
        assert_eq!(
            nix.release_source_blocker,
            ReleaseSourceBlocker::CanonicalSourceMissing
        );
        assert_eq!(nix.install_blocker, InstallBlocker::NixOwnedExternally);
        assert!(!nix.can_check());
        assert!(!nix.can_install());

        let appimage = UpdateBackendAssessment::from_owner(InstallOwner::AppImage(PathBuf::from(
            "/tmp/Orbis.AppImage",
        )));
        assert_eq!(
            appimage.install_blocker,
            InstallBlocker::AppImageInstallerMissing
        );
    }

    #[test]
    fn backend_has_no_downloader_package_manager_or_self_replace_surface() {
        let source = include_str!("updates_backend.rs");
        let forbidden = [
            ["Command", "::new"].concat(),
            ["req", "west"].concat(),
            ["std::fs::", "write"].concat(),
            ["apt", " install"].concat(),
            ["dnf", " install"].concat(),
            ["pacman", " -"].concat(),
        ];
        for token in forbidden {
            assert!(
                !source.contains(&token),
                "unexpected update mutation surface: {token}"
            );
        }
    }
}
