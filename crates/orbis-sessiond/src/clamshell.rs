//! User-session-owned clamshell inhibitor lifecycle.

use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use orbis_session_protocol::clamshell;

/// Narrow lifecycle boundary used by the Session1 service.
pub trait ClamshellInhibitor: Send + Sync {
    /// Return the authoritative current lifecycle state.
    fn status(&self) -> u8;
    /// Request enablement and return the resulting lifecycle state.
    fn set_enabled(&self, enabled: bool) -> u8;
}

struct State {
    child: Option<Child>,
    status: u8,
}

/// Production inhibitor. The child is deliberately owned by `orbis-sessiond`.
pub struct SystemdClamshellInhibitor {
    available: bool,
    state: Mutex<State>,
}

impl SystemdClamshellInhibitor {
    /// Probe the session-local systemd-inhibit executable without starting an inhibitor.
    pub fn new() -> Self {
        let available = Command::new("systemd-inhibit")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        Self {
            available,
            state: Mutex::new(State {
                child: None,
                status: if available {
                    clamshell::INACTIVE
                } else {
                    clamshell::UNAVAILABLE
                },
            }),
        }
    }
}

impl Default for SystemdClamshellInhibitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClamshellInhibitor for SystemdClamshellInhibitor {
    fn status(&self) -> u8 {
        let Ok(mut state) = self.state.lock() else {
            return clamshell::UNKNOWN;
        };
        if let Some(child) = state.child.as_mut() {
            match child.try_wait() {
                Ok(None) => return clamshell::ACTIVE,
                Ok(Some(_)) => {
                    state.child = None;
                    state.status = clamshell::UNKNOWN;
                }
                Err(_) => state.status = clamshell::UNKNOWN,
            }
        }
        state.status
    }

    fn set_enabled(&self, enabled: bool) -> u8 {
        let Ok(mut state) = self.state.lock() else {
            return clamshell::UNKNOWN;
        };
        if !self.available {
            state.status = clamshell::UNAVAILABLE;
            return state.status;
        }
        if enabled {
            if let Some(child) = state.child.as_mut() {
                if matches!(child.try_wait(), Ok(None)) {
                    state.status = clamshell::ACTIVE;
                    return state.status;
                }
                state.child = None;
            }
            let result = Command::new("systemd-inhibit")
                .args([
                    "--what=handle-lid-switch",
                    "--mode=block",
                    "--who=Orbis Control",
                    "--why=Closed-lid mode",
                    "sleep",
                    "infinity",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            state.status = match result {
                Ok(child) => {
                    state.child = Some(child);
                    clamshell::ACTIVE
                }
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    clamshell::PERMISSION_DENIED
                }
                Err(_) => clamshell::START_FAILED,
            };
            return state.status;
        }

        let Some(mut child) = state.child.take() else {
            state.status = clamshell::INACTIVE;
            return state.status;
        };
        state.status = match child.kill().and_then(|_| child.wait()) {
            Ok(_) => clamshell::INACTIVE,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                clamshell::PERMISSION_DENIED
            }
            Err(_) => clamshell::EXIT_FAILED,
        };
        state.status
    }
}
