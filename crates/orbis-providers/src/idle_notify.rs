//! User input idleness from the compositor (`ext-idle-notify-v1`).
//!
//! Session concern: the compositor is the only place that knows about input on
//! Wayland. One [`IdleWatch`] owns one notification for a fixed timeout; changing
//! the timeout means dropping the watch and opening a new one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use rustix::event::{PollFd, PollFlags, poll};
use tokio::sync::mpsc;
use wayland_client::globals::{BindError, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

use crate::error::ProviderError;

const POLL_SLICE_MS: i32 = 200;

/// Idleness transition reported by the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEvent {
    /// No input for the requested timeout.
    Idle,
    /// Input resumed after an [`IdleEvent::Idle`].
    Resumed,
}

/// Stream of [`IdleEvent`]s for one timeout. Dropping it stops the watcher.
pub struct IdleWatch {
    events: mpsc::UnboundedReceiver<IdleEvent>,
    stop: Arc<AtomicBool>,
}

impl IdleWatch {
    /// Watch fed by the returned sender (for adapters and tests).
    pub fn channel() -> (Self, mpsc::UnboundedSender<IdleEvent>) {
        let (sender, events) = mpsc::unbounded_channel();
        (
            Self {
                events,
                stop: Arc::new(AtomicBool::new(false)),
            },
            sender,
        )
    }

    /// Next transition; `None` once the compositor connection is gone.
    pub async fn next(&mut self) -> Option<IdleEvent> {
        self.events.recv().await
    }
}

impl Drop for IdleWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// Opens idle watches.
#[async_trait]
pub trait IdleSource: Send + Sync {
    /// Start reporting input idleness after `timeout`.
    async fn watch(&self, timeout: Duration) -> Result<IdleWatch, ProviderError>;
}

/// `ext-idle-notify-v1` backed idle source.
#[derive(Debug, Clone, Copy, Default)]
pub struct WaylandIdleSource;

#[async_trait]
impl IdleSource for WaylandIdleSource {
    async fn watch(&self, timeout: Duration) -> Result<IdleWatch, ProviderError> {
        let millis = u32::try_from(timeout.as_millis())
            .map_err(|_| ProviderError::InvalidRequest("idle timeout is too long".into()))?;
        tokio::task::spawn_blocking(move || start_watch(millis))
            .await
            .map_err(|error| ProviderError::Internal(format!("idle watcher start: {error}")))?
    }
}

struct WatchState {
    events: mpsc::UnboundedSender<IdleEvent>,
    stop: Arc<AtomicBool>,
}

fn unsupported(error: BindError) -> ProviderError {
    match error {
        BindError::NotPresent | BindError::UnsupportedVersion => {
            ProviderError::Unsupported("рабочий стол не поддерживает ext-idle-notify-v1".into())
        }
    }
}

fn start_watch(millis: u32) -> Result<IdleWatch, ProviderError> {
    let connection = Connection::connect_to_env()
        .map_err(|error| ProviderError::BackendUnavailable(format!("Wayland: {error}")))?;
    let (globals, mut queue) = registry_queue_init::<WatchState>(&connection)
        .map_err(|error| ProviderError::BackendUnavailable(format!("Wayland registry: {error}")))?;
    let handle = queue.handle();
    let notifier: ExtIdleNotifierV1 = globals.bind(&handle, 1..=2, ()).map_err(unsupported)?;
    let seat: wl_seat::WlSeat = globals.bind(&handle, 1..=1, ()).map_err(unsupported)?;
    // Version 2 ignores idle inhibitors: this is about the person's input.
    let _notification = if notifier.version() >= 2 {
        notifier.get_input_idle_notification(millis, &seat, &handle, ())
    } else {
        notifier.get_idle_notification(millis, &seat, &handle, ())
    };
    let (watch, events) = IdleWatch::channel();
    let mut state = WatchState {
        events,
        stop: watch.stop.clone(),
    };
    let stop = watch.stop.clone();
    queue.roundtrip(&mut state).map_err(|error| {
        ProviderError::BackendUnavailable(format!("Wayland roundtrip: {error}"))
    })?;
    std::thread::Builder::new()
        .name("orbis-idle-watch".into())
        .spawn(move || {
            let _keep = (_notification, notifier, seat);
            while !stop.load(Ordering::Acquire) {
                if queue.flush().is_err() || queue.dispatch_pending(&mut state).is_err() {
                    break;
                }
                let Some(guard) = queue.prepare_read() else {
                    continue;
                };
                let readable = {
                    let fd = guard.connection_fd();
                    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
                    poll(&mut fds, POLL_SLICE_MS)
                        .map(|ready| ready > 0)
                        .unwrap_or(false)
                };
                if readable && guard.read().is_err() {
                    break;
                }
            }
        })
        .map_err(ProviderError::Io)?;
    Ok(watch)
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WatchState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WatchState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for WatchState {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for WatchState {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let event = match event {
            ext_idle_notification_v1::Event::Idled => IdleEvent::Idle,
            ext_idle_notification_v1::Event::Resumed => IdleEvent::Resumed,
            _ => return,
        };
        if state.events.send(event).is_err() {
            state.stop.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn channel_watch_delivers_events_and_ends_with_the_sender() {
        let (mut watch, sender) = IdleWatch::channel();
        sender.send(IdleEvent::Idle).unwrap();
        sender.send(IdleEvent::Resumed).unwrap();
        drop(sender);
        assert_eq!(watch.next().await, Some(IdleEvent::Idle));
        assert_eq!(watch.next().await, Some(IdleEvent::Resumed));
        assert_eq!(watch.next().await, None);
    }

    #[test]
    fn dropping_the_watch_signals_the_thread_to_stop() {
        let (watch, _sender) = IdleWatch::channel();
        let stop = watch.stop.clone();
        drop(watch);
        assert!(stop.load(Ordering::Acquire));
    }
}
