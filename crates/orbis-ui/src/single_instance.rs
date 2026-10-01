//! One Orbis Control per session. The first instance owns the
//! `io.github.orbiscontrol.Orbis` session-bus name and serves the standard
//! `org.freedesktop.Application.Activate`; a later launch asks it to show
//! its window and exits. Only showing the window is exposed here.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::OwnedValue;

use crate::AppWindow;

const BUS_NAME: &str = "io.github.orbiscontrol.Orbis";
const OBJECT_PATH: &str = "/io/github/orbiscontrol/Orbis";

/// Main window of the primary instance, set once it is built.
pub(crate) type ActivationTarget = Arc<OnceLock<slint::Weak<AppWindow>>>;

struct Application {
    target: ActivationTarget,
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl Application {
    fn activate(&self, _platform_data: HashMap<String, OwnedValue>) {
        let Some(app) = self.target.get() else {
            return;
        };
        if let Err(error) = app.upgrade_in_event_loop(|app| crate::show_main_window(&app)) {
            tracing::debug!(?error, "activation request dropped");
        }
    }
}

/// Become the primary instance. `None` means another instance already owns
/// the name.
pub(crate) async fn claim(connection: &zbus::Connection) -> zbus::Result<Option<ActivationTarget>> {
    let target = ActivationTarget::default();
    connection
        .object_server()
        .at(
            OBJECT_PATH,
            Application {
                target: target.clone(),
            },
        )
        .await?;
    match connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(Some(target)),
        Ok(_) | Err(zbus::Error::NameTaken) => {
            connection
                .object_server()
                .remove::<Application, _>(OBJECT_PATH)
                .await?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Ask the running instance to show its main window.
pub(crate) async fn activate_running(connection: &zbus::Connection) -> zbus::Result<()> {
    let platform_data: HashMap<String, OwnedValue> = HashMap::new();
    connection
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some("org.freedesktop.Application"),
            "Activate",
            &(platform_data,),
        )
        .await?;
    Ok(())
}
