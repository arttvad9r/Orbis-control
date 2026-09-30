//! Desktop notifications (freedesktop `org.freedesktop.Notifications`) for
//! mode changes that happen outside the window: Fn+F5, power rules, global
//! shortcuts. Like G-Helper's toast. A change requested from the window
//! itself is not announced (the user is looking at it), and each new toast
//! replaces the previous one instead of stacking.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

struct Context {
    runtime: tokio::runtime::Handle,
    connection: zbus::Connection,
    last_id: Arc<AtomicU32>,
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
    static LAST_UI_REQUEST: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

static ENABLED: AtomicBool = AtomicBool::new(true);

/// A change the window asked for echoes back within this window of time.
const UI_ECHO: Duration = Duration::from_secs(4);

pub(crate) fn initialize(runtime: tokio::runtime::Handle, connection: zbus::Connection) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(Context {
            runtime,
            connection,
            last_id: Arc::new(AtomicU32::new(0)),
        });
    });
}

pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Record that the window itself requested a mode change.
pub(crate) fn note_ui_request() {
    LAST_UI_REQUEST.with(|slot| *slot.borrow_mut() = Some(Instant::now()));
}

fn recently_requested_by_ui() -> bool {
    LAST_UI_REQUEST.with(|slot| slot.borrow().is_some_and(|at| at.elapsed() < UI_ECHO))
}

/// Show (or replace) the mode toast unless the window caused the change.
pub(crate) fn mode_changed(summary: String, body: String) {
    if !ENABLED.load(Ordering::Relaxed) || recently_requested_by_ui() {
        return;
    }
    CONTEXT.with(|slot| {
        let Some(context) = slot
            .borrow()
            .as_ref()
            .map(|c| (c.runtime.clone(), c.connection.clone(), c.last_id.clone()))
        else {
            return;
        };
        let (runtime, connection, last_id) = context;
        runtime.spawn(async move {
            if let Err(error) = notify(&connection, &last_id, &summary, &body).await {
                tracing::debug!(%error, "mode notification not shown");
            }
        });
    });
}

/// Send one toast, replacing the previous one from this process.
async fn notify(
    connection: &zbus::Connection,
    last_id: &AtomicU32,
    summary: &str,
    body: &str,
) -> zbus::Result<()> {
    let mut hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    hints.insert("transient", zbus::zvariant::Value::from(true));
    hints.insert("urgency", zbus::zvariant::Value::from(0u8));
    let reply = connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Orbis Control",
                last_id.load(Ordering::Relaxed),
                "io.github.orbiscontrol.Orbis",
                summary,
                body,
                Vec::<&str>::new(),
                hints,
                2500_i32,
            ),
        )
        .await?;
    last_id.store(reply.body().deserialize::<u32>()?, Ordering::Relaxed);
    Ok(())
}

/// Toast text for a performance profile index (0 Тихий, 1 Баланс, 2 Турбо).
pub(crate) fn profile_text(index: i32) -> Option<(String, String)> {
    let name = match index {
        0 => "Тихий",
        1 => "Баланс",
        2 => "Турбо",
        _ => return None,
    };
    Some((
        format!("Режим: {name}"),
        "Профиль производительности".to_string(),
    ))
}

/// Toast text for a GPU mode that was queued for the next boot.
pub(crate) fn gpu_queued_text(index: i32) -> Option<(String, String)> {
    let name = match index {
        0 => "Eco",
        1 => "Standard",
        2 => "Ultimate",
        _ => return None,
    };
    Some((
        format!("GPU: {name}"),
        "Режим видеокарты применится после перезагрузки".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_cover_known_modes_only() {
        assert_eq!(profile_text(2).unwrap().0, "Режим: Турбо");
        assert!(profile_text(7).is_none());
        assert_eq!(gpu_queued_text(0).unwrap().0, "GPU: Eco");
        assert!(gpu_queued_text(-1).is_none());
    }

    struct FakeNotifications {
        seen: std::sync::Mutex<Vec<(u32, String)>>,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeNotifications {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            _app: String,
            replaces_id: u32,
            _icon: String,
            summary: String,
            _body: String,
            _actions: Vec<String>,
            _hints: HashMap<String, zbus::zvariant::OwnedValue>,
            _timeout: i32,
        ) -> u32 {
            let mut seen = self.seen.lock().unwrap();
            seen.push((replaces_id, summary));
            if replaces_id == 0 { 41 } else { replaces_id }
        }
    }

    #[tokio::test]
    async fn a_new_toast_replaces_the_previous_one() {
        let Ok(mut daemon) = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
        else {
            eprintln!("dbus-daemon unavailable; skipping");
            return;
        };
        let mut address = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(daemon.stdout.take().unwrap()),
            &mut address,
        )
        .unwrap();
        let connect = || async {
            zbus::connection::Builder::address(address.trim())
                .unwrap()
                .build()
                .await
                .unwrap()
        };
        let server = connect().await;
        server
            .object_server()
            .at(
                "/org/freedesktop/Notifications",
                FakeNotifications {
                    seen: Default::default(),
                },
            )
            .await
            .unwrap();
        server
            .request_name("org.freedesktop.Notifications")
            .await
            .unwrap();
        let client = connect().await;
        let last_id = AtomicU32::new(0);
        notify(&client, &last_id, "Режим: Турбо", "").await.unwrap();
        notify(&client, &last_id, "Режим: Тихий", "").await.unwrap();
        let iface = server
            .object_server()
            .interface::<_, FakeNotifications>("/org/freedesktop/Notifications")
            .await
            .unwrap();
        let seen = iface.get().await.seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![
                (0, "Режим: Турбо".to_string()),
                (41, "Режим: Тихий".to_string())
            ]
        );
        let _ = daemon.kill();
    }

    #[test]
    fn a_window_request_suppresses_its_own_echo() {
        assert!(!recently_requested_by_ui());
        note_ui_request();
        assert!(recently_requested_by_ui());
    }
}
