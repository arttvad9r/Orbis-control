//! Read-only system and session metadata source for production diagnostics.

use std::{env, fs};

use orbis_core::diagnostics::{DisplayProtocol, SessionType, SystemDiagnostics};

const KERNEL_RELEASE_PATH: &str = "/proc/sys/kernel/osrelease";

/// Read-only provider for narrow, privacy-safe system/session diagnostics.
///
/// The provider reads only the kernel release file and a three-variable
/// environment allowlist used to normalize session/display protocol. It does
/// not inspect the full environment and does not attempt generic compositor
/// identification.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemMetadataProvider;

impl SystemMetadataProvider {
    /// Create a system metadata provider.
    pub const fn new() -> Self {
        Self
    }

    /// Collect one point-in-time system diagnostics snapshot.
    pub fn snapshot(&self) -> SystemDiagnostics {
        let session = SessionEnvironment::from_lookup(|key| env::var(key).ok());
        system_diagnostics_from(
            read_trimmed_file(KERNEL_RELEASE_PATH),
            Some(env::consts::ARCH.to_owned()),
            &session,
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SessionEnvironment {
    xdg_session_type: Option<String>,
    wayland_display: Option<String>,
    display: Option<String>,
}

impl SessionEnvironment {
    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Self {
        Self {
            xdg_session_type: normalized_value(lookup("XDG_SESSION_TYPE")),
            wayland_display: normalized_value(lookup("WAYLAND_DISPLAY")),
            display: normalized_value(lookup("DISPLAY")),
        }
    }
}

fn system_diagnostics_from(
    kernel_release: Option<String>,
    architecture: Option<String>,
    session: &SessionEnvironment,
) -> SystemDiagnostics {
    SystemDiagnostics {
        kernel_release: normalized_value(kernel_release),
        architecture: normalized_value(architecture),
        session_type: classify_session_type(session),
        display_protocol: classify_display_protocol(session),
        // The current architecture has no reliable generic compositor source.
        // In particular, XDG_CURRENT_DESKTOP is a desktop/session label, not a
        // compositor identity, so it is deliberately never queried here.
        compositor: None,
    }
}

fn classify_session_type(session: &SessionEnvironment) -> SessionType {
    match session.xdg_session_type.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("wayland") => SessionType::Wayland,
        Some(value) if value.eq_ignore_ascii_case("x11") => SessionType::X11,
        Some(value) if value.eq_ignore_ascii_case("tty") => SessionType::Tty,
        Some(_) => SessionType::Unknown,
        None if session.wayland_display.is_some() => SessionType::Wayland,
        None if session.display.is_some() => SessionType::X11,
        None => SessionType::Unknown,
    }
}

fn classify_display_protocol(session: &SessionEnvironment) -> DisplayProtocol {
    if session.wayland_display.is_some() {
        return DisplayProtocol::Wayland;
    }
    if session.display.is_some() {
        return DisplayProtocol::X11;
    }

    match session.xdg_session_type.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("wayland") => DisplayProtocol::Wayland,
        Some(value) if value.eq_ignore_ascii_case("x11") => DisplayProtocol::X11,
        _ => DisplayProtocol::Unknown,
    }
}

fn read_trimmed_file(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| normalized_value(Some(value)))
}

fn normalized_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_owned())
        }
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn session(
        xdg_session_type: Option<&str>,
        wayland_display: Option<&str>,
        display: Option<&str>,
    ) -> SessionEnvironment {
        SessionEnvironment {
            xdg_session_type: normalized_value(xdg_session_type.map(str::to_owned)),
            wayland_display: normalized_value(wayland_display.map(str::to_owned)),
            display: normalized_value(display.map(str::to_owned)),
        }
    }

    #[test]
    fn exact_kernel_and_architecture_values_are_preserved_after_trimming() {
        let metadata = system_diagnostics_from(
            Some(" 6.12.7-orbis\n".into()),
            Some(" x86_64 ".into()),
            &SessionEnvironment::default(),
        );

        assert_eq!(metadata.kernel_release.as_deref(), Some("6.12.7-orbis"));
        assert_eq!(metadata.architecture.as_deref(), Some("x86_64"));
        assert!(metadata.compositor.is_none());
    }

    #[test]
    fn recognized_session_types_are_normalized() {
        let wayland = session(Some(" WAYLAND "), None, None);
        let x11 = session(Some("X11"), None, None);
        let tty = session(Some("tty"), None, None);

        assert_eq!(classify_session_type(&wayland), SessionType::Wayland);
        assert_eq!(
            classify_display_protocol(&wayland),
            DisplayProtocol::Wayland
        );
        assert_eq!(classify_session_type(&x11), SessionType::X11);
        assert_eq!(classify_display_protocol(&x11), DisplayProtocol::X11);
        assert_eq!(classify_session_type(&tty), SessionType::Tty);
        assert_eq!(classify_display_protocol(&tty), DisplayProtocol::Unknown);
    }

    #[test]
    fn missing_session_type_uses_display_evidence() {
        let wayland = session(None, Some("wayland-0"), Some(":0"));
        let x11 = session(None, None, Some(":0"));

        assert_eq!(classify_session_type(&wayland), SessionType::Wayland);
        assert_eq!(
            classify_display_protocol(&wayland),
            DisplayProtocol::Wayland
        );
        assert_eq!(classify_session_type(&x11), SessionType::X11);
        assert_eq!(classify_display_protocol(&x11), DisplayProtocol::X11);
    }

    #[test]
    fn unrecognized_session_type_is_not_relabelled() {
        let unknown = session(Some("custom-session"), None, None);
        assert_eq!(classify_session_type(&unknown), SessionType::Unknown);
        assert_eq!(
            classify_display_protocol(&unknown),
            DisplayProtocol::Unknown
        );
    }

    #[test]
    fn display_protocol_prefers_explicit_wayland_display_evidence() {
        let mixed = session(Some("x11"), Some("wayland-0"), Some(":0"));
        assert_eq!(classify_session_type(&mixed), SessionType::X11);
        assert_eq!(classify_display_protocol(&mixed), DisplayProtocol::Wayland);
    }

    #[test]
    fn blank_values_become_absent_or_unknown() {
        let environment = SessionEnvironment::from_lookup(|key| match key {
            "XDG_SESSION_TYPE" => Some("   ".into()),
            "WAYLAND_DISPLAY" => Some("\t".into()),
            "DISPLAY" => Some("\n".into()),
            _ => None,
        });
        let metadata = system_diagnostics_from(Some("  ".into()), Some("\t".into()), &environment);

        assert!(metadata.kernel_release.is_none());
        assert!(metadata.architecture.is_none());
        assert_eq!(metadata.session_type, SessionType::Unknown);
        assert_eq!(metadata.display_protocol, DisplayProtocol::Unknown);
        assert!(metadata.compositor.is_none());
    }

    #[test]
    fn environment_access_is_strictly_allowlisted() {
        let queried = RefCell::new(Vec::new());
        let environment = SessionEnvironment::from_lookup(|key| {
            queried.borrow_mut().push(key.to_owned());
            None
        });

        assert_eq!(environment, SessionEnvironment::default());
        assert_eq!(
            queried.into_inner(),
            vec!["XDG_SESSION_TYPE", "WAYLAND_DISPLAY", "DISPLAY"]
        );
    }

    #[test]
    fn compositor_remains_unknown_without_reliable_source() {
        let environment = session(Some("wayland"), Some("wayland-0"), None);
        let metadata = system_diagnostics_from(None, Some("aarch64".into()), &environment);
        assert!(metadata.compositor.is_none());
    }
}
