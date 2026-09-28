//! Transient keyboard backlight level changes for the active user session.
//!
//! Writes go through `systemd-logind` `Session.SetBrightness`, the operating
//! system's own mechanism for an active session to drive LED class devices (it
//! is what desktop power managers use). No Hardware1 call and no direct sysfs
//! write: this is not a privileged mutation path. Reads use the sysfs provider.

use std::time::Duration;

use async_trait::async_trait;

use crate::error::ProviderError;
use crate::keyboard_backlight::AsusKeyboardBacklightProvider;
use crate::traits::KeyboardBacklightProvider;

const CALL_DEADLINE: Duration = Duration::from_secs(3);
const LED_SUBSYSTEM: &str = "leds";
const LED_NAME: &str = "asus::kbd_backlight";

/// Read the keyboard backlight level and set it transiently.
#[async_trait]
pub trait KeyboardLight: Send + Sync {
    /// Fresh hardware level.
    async fn level(&self) -> Result<u8, ProviderError>;
    /// Ask for `level`; callers confirm with [`KeyboardLight::level`].
    async fn set_level(&self, level: u8) -> Result<(), ProviderError>;
}

/// Sysfs read plus logind write.
pub struct LogindKeyboardLight {
    system: zbus::Connection,
    reader: AsusKeyboardBacklightProvider,
}

impl LogindKeyboardLight {
    /// Use an existing system bus connection.
    pub fn new(system: zbus::Connection) -> Self {
        Self {
            system,
            reader: AsusKeyboardBacklightProvider::default(),
        }
    }
}

#[async_trait]
impl KeyboardLight for LogindKeyboardLight {
    async fn level(&self) -> Result<u8, ProviderError> {
        Ok(self.reader.keyboard_backlight_state().await?.current.get())
    }

    async fn set_level(&self, level: u8) -> Result<(), ProviderError> {
        let body = (LED_SUBSYSTEM, LED_NAME, u32::from(level));
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
