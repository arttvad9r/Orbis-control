//! Orbis application update backend.
//!
//! This is deliberately not the device-firmware `FirmwareUpdateProvider`.
//! The Updates window represents the Orbis application/package itself. The
//! current repository has multiple packaging targets but no canonical release
//! metadata endpoint and no single installation mutation owner. Therefore this
//! backend proves only local installation ownership and keeps network check and
//! install actions disabled until a concrete package/release owner exists.
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

pub(crate) fn read_state() -> Result<UpdatesUiState, std::io::Error> {
    let owner = detect_install_owner()?;
    let label = owner.label();
    let ownership = owner.ownership_message();

    Ok(UpdatesUiState {
        // The local ownership backend is functional. Remote release source and
        // package installation are separate capabilities and remain false.
        backend_ready: true,
        source_ready: false,
        check_enabled: false,
        install_enabled: false,
        channel: 0,
        channel_enabled: false,
        update_available: false,
        latest_version: "—".into(),
        release_notes: format!(
            "{ownership}. Orbis does not currently define a canonical signed release feed for this installation owner."
        ),
        status: format!("{label} detected · release source not configured"),
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
            "update channel request ignored: no canonical Orbis release source is configured"
        );
    });

    {
        let weak = window.as_weak();
        window.on_check_requested(move || {
            tracing::warn!("update check ignored: no canonical Orbis release source is configured");
            if let Some(window) = weak.upgrade() {
                refresh(&window);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_install_requested(move || {
            tracing::warn!("update install ignored: no proven installation mutation owner exists");
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
    fn source_and_install_are_fail_closed() {
        let owner = InstallOwner::NixStore;
        assert!(owner.ownership_message().contains("Nix"));
        let source = include_str!("updates_backend.rs");
        assert!(source.contains("source_ready: false"));
        assert!(source.contains("check_enabled: false"));
        assert!(source.contains("install_enabled: false"));
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
            assert!(!source.contains(&token), "unexpected update mutation surface: {token}");
        }
    }
}
