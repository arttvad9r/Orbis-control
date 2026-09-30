//! System-wide shortcuts through the XDG desktop portal
//! (`org.freedesktop.portal.GlobalShortcuts`), the Wayland-safe way: no
//! access to `/dev/input`, the desktop owns the key bindings and lets the
//! user change them (KDE: System Settings → Shortcuts). Where the portal is
//! missing the shortcuts are simply not registered.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";

/// Actions a shortcut can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutAction {
    /// Show the window, or hide it to the tray when it is shown.
    ToggleWindow,
    /// Switch to the next available performance profile.
    CycleProfile,
}

/// Shortcut id, description and preferred trigger (G-Helper's defaults where
/// it has one: Ctrl+Shift+F5 cycles the mode).
pub(crate) const SHORTCUTS_SPEC: [(&str, &str, &str, ShortcutAction); 2] = [
    (
        "toggle-window",
        "Показать или скрыть Orbis Control",
        "CTRL+SHIFT+F12",
        ShortcutAction::ToggleWindow,
    ),
    (
        "cycle-profile",
        "Следующий профиль производительности",
        "CTRL+SHIFT+F5",
        ShortcutAction::CycleProfile,
    ),
];

pub(crate) fn action_for(id: &str) -> Option<ShortcutAction> {
    SHORTCUTS_SPEC
        .iter()
        .find(|(shortcut, ..)| *shortcut == id)
        .map(|(.., action)| *action)
}

/// Portal request handles live at a path derived from our unique name, so
/// the Response signal can be subscribed to before the call (no race).
fn request_path(connection: &zbus::Connection, token: &str) -> Option<String> {
    let sender = connection
        .unique_name()?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    Some(format!("{PORTAL_PATH}/request/{sender}/{token}"))
}

async fn call_and_wait(
    connection: &zbus::Connection,
    token: &str,
    method: &str,
    body: &(impl zbus::export::serde::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<HashMap<String, OwnedValue>> {
    let path = request_path(connection, token)
        .ok_or_else(|| zbus::Error::Failure("no unique bus name".into()))?;
    let request = zbus::Proxy::new(connection, PORTAL, path, REQUEST).await?;
    let mut responses = request.receive_signal("Response").await?;
    connection
        .call_method(Some(PORTAL), PORTAL_PATH, Some(SHORTCUTS), method, body)
        .await?;
    // The desktop may show a confirmation dialog; give the user time.
    let response = tokio::time::timeout(Duration::from_secs(300), responses.next())
        .await
        .map_err(|_| zbus::Error::Failure(format!("{method}: no portal response")))?
        .ok_or_else(|| zbus::Error::Failure(format!("{method}: response stream closed")))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = response.body().deserialize()?;
    if code != 0 {
        return Err(zbus::Error::Failure(format!(
            "{method}: portal answered {code}"
        )));
    }
    Ok(results)
}

/// Register the shortcuts and call `on_action` for every activation until the
/// session bus goes away. Returns early (logged) when there is no portal.
pub(crate) async fn run(connection: zbus::Connection, on_action: impl Fn(ShortcutAction)) {
    if let Err(error) = register_and_listen(&connection, &on_action).await {
        tracing::info!(%error, "global shortcuts unavailable");
    }
}

async fn register_and_listen(
    connection: &zbus::Connection,
    on_action: &impl Fn(ShortcutAction),
) -> zbus::Result<()> {
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from("orbis_session"));
    options.insert("session_handle_token", Value::from("orbis"));
    let results = call_and_wait(connection, "orbis_session", "CreateSession", &(options,)).await?;
    let session: OwnedObjectPath = results
        .get("session_handle")
        .and_then(|value| {
            // Spec says `o`, some portals send `s`.
            OwnedObjectPath::try_from(value.clone()).ok().or_else(|| {
                String::try_from(value.clone())
                    .ok()
                    .and_then(|s| OwnedObjectPath::try_from(s).ok())
            })
        })
        .ok_or_else(|| zbus::Error::Failure("CreateSession: no session handle".into()))?;

    let shortcuts: Vec<(&str, HashMap<&str, Value<'_>>)> = SHORTCUTS_SPEC
        .iter()
        .map(|(id, description, trigger, _)| {
            let mut properties = HashMap::new();
            properties.insert("description", Value::from(*description));
            properties.insert("preferred_trigger", Value::from(*trigger));
            (*id, properties)
        })
        .collect();
    let mut bind_options: HashMap<&str, Value<'_>> = HashMap::new();
    bind_options.insert("handle_token", Value::from("orbis_bind"));

    // Subscribe to activations before binding so none is missed.
    let portal = zbus::Proxy::new(connection, PORTAL, PORTAL_PATH, SHORTCUTS).await?;
    let mut activations = portal.receive_signal("Activated").await?;
    call_and_wait(
        connection,
        "orbis_bind",
        "BindShortcuts",
        &(&session, shortcuts, "", bind_options),
    )
    .await?;
    tracing::info!("global shortcuts registered through the desktop portal");

    while let Some(signal) = activations.next().await {
        let Ok((from, id, _timestamp, _options)) =
            signal
                .body()
                .deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>()
        else {
            continue;
        };
        if from != session {
            continue;
        }
        if let Some(action) = action_for(&id) {
            on_action(action);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    /// Private session bus (dbus-daemon) so the portal protocol runs over a
    /// real bus with unique names; skipped when dbus-daemon is missing.
    struct Bus {
        child: std::process::Child,
        address: String,
    }

    impl Bus {
        fn start() -> Option<Self> {
            use std::io::BufRead;
            let mut child = std::process::Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .ok()?;
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take()?)
                .read_line(&mut line)
                .ok()?;
            Some(Self {
                child,
                address: line.trim().to_string(),
            })
        }
        async fn connect(&self) -> zbus::Connection {
            zbus::connection::Builder::address(self.address.as_str())
                .unwrap()
                .build()
                .await
                .unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
        }
    }

    struct FakePortal {
        bound: Arc<Mutex<Vec<String>>>,
    }

    fn reply_path(sender: &str, options: &HashMap<String, OwnedValue>) -> String {
        let token = String::try_from(options["handle_token"].clone()).unwrap();
        let sender = sender.trim_start_matches(':').replace('.', "_");
        format!("{PORTAL_PATH}/request/{sender}/{token}")
    }

    #[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
    impl FakePortal {
        async fn create_session(
            &self,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> OwnedObjectPath {
            let path = reply_path(header.sender().unwrap().as_str(), &options);
            let mut results: HashMap<&str, Value<'_>> = HashMap::new();
            results.insert(
                "session_handle",
                Value::from("/org/freedesktop/portal/desktop/session/1_2/orbis"),
            );
            connection
                .emit_signal(
                    None::<&str>,
                    path.as_str(),
                    REQUEST,
                    "Response",
                    &(0u32, results),
                )
                .await
                .unwrap();
            OwnedObjectPath::try_from(path).unwrap()
        }

        async fn bind_shortcuts(
            &self,
            _session: OwnedObjectPath,
            shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
            _parent: String,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> OwnedObjectPath {
            self.bound
                .lock()
                .unwrap()
                .extend(shortcuts.into_iter().map(|(id, _)| id));
            let path = reply_path(header.sender().unwrap().as_str(), &options);
            connection
                .emit_signal(
                    None::<&str>,
                    path.as_str(),
                    REQUEST,
                    "Response",
                    &(0u32, HashMap::<&str, Value<'_>>::new()),
                )
                .await
                .unwrap();
            OwnedObjectPath::try_from(path).unwrap()
        }
    }

    #[tokio::test]
    async fn registers_through_the_portal_and_reports_activations() {
        let Some(bus) = Bus::start() else {
            eprintln!("dbus-daemon unavailable; skipping");
            return;
        };
        let bound = Arc::new(Mutex::new(Vec::new()));
        let portal = bus.connect().await;
        portal
            .object_server()
            .at(
                PORTAL_PATH,
                FakePortal {
                    bound: bound.clone(),
                },
            )
            .await
            .unwrap();
        portal.request_name(PORTAL).await.unwrap();

        let client = bus.connect().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(run(client, move |action| {
            let _ = tx.send(action);
        }));

        // Wait until both shortcuts are bound, then fire activations.
        for _ in 0..200 {
            if bound.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            *bound.lock().unwrap(),
            vec!["toggle-window", "cycle-profile"]
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        let session =
            OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/session/1_2/orbis").unwrap();
        let other =
            OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/session/9/other").unwrap();
        for (from, id) in [
            (&other, "cycle-profile"),
            (&session, "cycle-profile"),
            (&session, "toggle-window"),
        ] {
            portal
                .emit_signal(
                    None::<&str>,
                    PORTAL_PATH,
                    SHORTCUTS,
                    "Activated",
                    &(from, id, 0u64, HashMap::<&str, Value<'_>>::new()),
                )
                .await
                .unwrap();
        }
        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap();
        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap();
        assert_eq!(
            first,
            Some(ShortcutAction::CycleProfile),
            "other sessions are ignored"
        );
        assert_eq!(second, Some(ShortcutAction::ToggleWindow));
    }

    #[test]
    fn every_shortcut_id_maps_to_its_action() {
        assert_eq!(
            action_for("toggle-window"),
            Some(ShortcutAction::ToggleWindow)
        );
        assert_eq!(
            action_for("cycle-profile"),
            Some(ShortcutAction::CycleProfile)
        );
        assert_eq!(action_for("unknown"), None);
    }
}
