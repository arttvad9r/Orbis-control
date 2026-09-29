//! Built-in panel backlight: sysfs read, `systemd-logind` write.
//!
//! Writes go through `Session.SetBrightness` (the mechanism desktop power
//! managers use), so this is neither a Hardware1 call nor a direct sysfs write.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::ProviderError;

const CALL_DEADLINE: Duration = Duration::from_secs(3);
const BACKLIGHT_SUBSYSTEM: &str = "backlight";

/// Raw backlight reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelBrightness {
    /// Raw level.
    pub level: u32,
    /// Raw maximum.
    pub max: u32,
}

impl PanelBrightness {
    /// Current level as 1..=100 (0 only when the panel is really at zero).
    pub fn percent(self) -> u8 {
        if self.max == 0 {
            return 0;
        }
        let percent = (u64::from(self.level) * 100 + u64::from(self.max) / 2) / u64::from(self.max);
        percent.min(100) as u8
    }
}

/// Raw level for a requested percent; never below 1 so the screen cannot be
/// switched off from here.
pub fn raw_for_percent(percent: u8, max: u32) -> u32 {
    let percent = u64::from(percent.clamp(1, 100));
    let raw = (percent * u64::from(max) + 50) / 100;
    u32::try_from(raw).unwrap_or(max).clamp(1, max.max(1))
}

/// Read the panel backlight and set it as a percent.
#[async_trait]
pub trait PanelLight: Send + Sync {
    /// Fresh hardware reading.
    async fn read(&self) -> Result<PanelBrightness, ProviderError>;
    /// Ask for a percent; callers confirm with [`PanelLight::read`].
    async fn set_percent(&self, percent: u8) -> Result<(), ProviderError>;
}

/// Pick the backlight the desktop would use: firmware, then platform, then raw.
fn find_backlight(root: &std::path::Path) -> Option<(String, PathBuf)> {
    let mut best: Option<(u8, String, PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rank = match std::fs::read_to_string(path.join("type"))
            .unwrap_or_default()
            .trim()
        {
            "firmware" => 0,
            "platform" => 1,
            _ => 2,
        };
        if best
            .as_ref()
            .is_none_or(|(r, n, _)| (rank, &name) < (*r, n))
        {
            best = Some((rank, name, path));
        }
    }
    best.map(|(_, name, path)| (name, path))
}

fn read_value(dir: &std::path::Path, file: &str) -> Result<u32, ProviderError> {
    std::fs::read_to_string(dir.join(file))
        .map_err(ProviderError::Io)?
        .trim()
        .parse()
        .map_err(|_| ProviderError::Internal(format!("backlight {file}")))
}

/// Sysfs read plus logind write.
pub struct LogindPanelLight {
    system: zbus::Connection,
    root: PathBuf,
}

impl LogindPanelLight {
    /// Use an existing system bus connection.
    pub fn new(system: zbus::Connection) -> Self {
        Self {
            system,
            root: PathBuf::from("/sys/class/backlight"),
        }
    }

    fn device(&self) -> Result<(String, PathBuf), ProviderError> {
        find_backlight(&self.root)
            .ok_or_else(|| ProviderError::Unsupported("панель без управляемой подсветки".into()))
    }
}

#[async_trait]
impl PanelLight for LogindPanelLight {
    async fn read(&self) -> Result<PanelBrightness, ProviderError> {
        let (_, dir) = self.device()?;
        let max = read_value(&dir, "max_brightness")?;
        if max == 0 {
            return Err(ProviderError::Internal("backlight max_brightness".into()));
        }
        Ok(PanelBrightness {
            level: read_value(&dir, "brightness")?,
            max,
        })
    }

    async fn set_percent(&self, percent: u8) -> Result<(), ProviderError> {
        let (name, _) = self.device()?;
        let max = self.read().await?.max;
        let body = (BACKLIGHT_SUBSYSTEM, name, raw_for_percent(percent, max));
        let call = self.system.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &body,
        );
        tokio::time::timeout(CALL_DEADLINE, call)
            .await
            .map_err(|_| ProviderError::Timeout("logind SetBrightness".into()))?
            .map(|_| ())
            .map_err(|error| ProviderError::Dbus(format!("logind SetBrightness: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_and_raw_never_reaches_zero() {
        assert_eq!(
            PanelBrightness {
                level: 120,
                max: 200
            }
            .percent(),
            60
        );
        assert_eq!(PanelBrightness { level: 0, max: 200 }.percent(), 0);
        assert_eq!(PanelBrightness { level: 5, max: 3 }.percent(), 100);
        assert_eq!(raw_for_percent(60, 200), 120);
        assert_eq!(raw_for_percent(0, 200), 2);
        assert_eq!(raw_for_percent(1, 50), 1);
        assert_eq!(raw_for_percent(100, 200), 200);
        assert_eq!(raw_for_percent(250, 200), 200);
    }

    #[test]
    fn firmware_backlight_wins_over_raw() {
        let dir = tempfile::tempdir().unwrap();
        for (name, kind) in [("intel_backlight", "raw"), ("acpi_video0", "firmware")] {
            let path = dir.path().join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("type"), format!("{kind}\n")).unwrap();
        }
        assert_eq!(find_backlight(dir.path()).unwrap().0, "acpi_video0");
        assert!(find_backlight(&dir.path().join("missing")).is_none());
    }
}
