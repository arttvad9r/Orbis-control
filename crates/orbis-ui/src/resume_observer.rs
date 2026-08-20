//! Read-only logind resume signal observer for Automation shadow lifecycle.
//!
//! This module subscribes only to `org.freedesktop.login1.Manager`'s
//! `PrepareForSleep(bool)` signal. It acquires no inhibitor, calls no D-Bus
//! methods after proxy construction, and performs no hardware mutation. Signals
//! are returned to the Slint event loop before touching thread-local UI backend
//! state.

use std::time::{Duration, SystemTime};

use futures_util::StreamExt;
use slint::ComponentHandle;

use crate::AppWindow;

const LOGIND_SERVICE: &str = "org.freedesktop.login1";
const LOGIND_PATH: &str = "/org/freedesktop/login1";
const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";
const PREPARE_FOR_SLEEP: &str = "PrepareForSleep";
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Spawn one long-lived read-only logind observer on the existing Tokio
/// runtime. The task terminates naturally when the application runtime drops.
pub(crate) fn spawn(runtime: tokio::runtime::Handle, app: slint::Weak<AppWindow>) {
    runtime.spawn(async move {
        loop {
            if let Err(error) = listen_once(&app).await {
                tracing::debug!(error = ?error, "logind resume observer unavailable; retrying");
            } else {
                tracing::debug!("logind resume signal stream ended; reconnecting");
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    });
}

async fn listen_once(app: &slint::Weak<AppWindow>) -> zbus::Result<()> {
    let connection = zbus::Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        LOGIND_SERVICE,
        LOGIND_PATH,
        LOGIND_MANAGER,
    )
    .await?;
    let mut signals = proxy.receive_signal(PREPARE_FOR_SLEEP).await?;

    while let Some(message) = signals.next().await {
        let (start,): (bool,) = match message.body().deserialize() {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(error = ?error, "invalid logind PrepareForSleep signal ignored");
                continue;
            }
        };
        let observed_at = SystemTime::now();
        let weak = app.clone();
        if let Err(error) = weak.upgrade_in_event_loop(move |app| {
            super::secondary_windows_backend::observe_prepare_for_sleep(start, observed_at);
            if !start {
                // Resume itself is not enough evidence. Force one fresh typed
                // sysfs read; the resume gate will accept it only if its
                // provider timestamp is at/after the matching resume signal.
                super::observe_automation_from_sysfs(&app);
            }
        }) {
            tracing::debug!(error = ?error, "failed to deliver logind lifecycle observation");
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_is_read_only_and_targets_only_prepare_for_sleep() {
        let source = include_str!("resume_observer.rs");
        assert!(source.contains(LOGIND_SERVICE));
        assert!(source.contains(PREPARE_FOR_SLEEP));
        assert!(source.contains("receive_signal"));
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "profile("].concat(),
            ["set_", "fan_curve("].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "charge_limit("].concat(),
            ["call_", "method("].concat(),
            ["Command", "::new"].concat(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected mutation token: {needle}");
        }
    }

    #[test]
    fn reconnect_delay_is_bounded_and_nonzero() {
        assert!(RECONNECT_DELAY >= Duration::from_secs(1));
        assert!(RECONNECT_DELAY <= Duration::from_secs(30));
    }
}
