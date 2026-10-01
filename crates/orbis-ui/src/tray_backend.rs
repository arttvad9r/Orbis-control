//! Minimal Linux StatusNotifierItem owner for Slint 1.13.x.
//!
//! The locked Slint version predates its native SystemTrayIcon element, so this
//! module implements only the standard session-bus StatusNotifierItem activation
//! surface through the already-present zbus dependency. There is no shell helper,
//! subprocess, arbitrary D-Bus method surface or hardware access.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use slint::ComponentHandle;
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedSender;

use crate::AppWindow;

#[allow(dead_code)]
const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
#[allow(dead_code)]
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const RECHECK_INTERVAL: Duration = Duration::from_secs(15);

static TOOLTIP_DESCRIPTION: Mutex<String> = Mutex::new(String::new());
static TOOLTIP_CHANGED: Notify = Notify::const_new();

/// Latest telemetry as shown in the UI; unknown values are "—" or empty.
#[derive(Debug, Clone, Default)]
pub(crate) struct TrayStats {
    pub cpu_temp: String,
    pub gpu_temp: String,
    pub cpu_fan: String,
    pub gpu_fan: String,
    pub battery_percent: String,
    pub on_ac: Option<bool>,
    pub fresh: bool,
}

fn known(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty() && value != "—").then_some(value)
}

fn stats_line(label: &str, temp: &str, fan: &str) -> Option<String> {
    let parts: Vec<&str> = [known(temp), known(fan)].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| format!("{label}: {}", parts.join(", ")))
}

fn tooltip_description(stats: &TrayStats) -> String {
    let mut lines = Vec::new();
    lines.extend(stats_line("CPU", &stats.cpu_temp, &stats.cpu_fan));
    lines.extend(stats_line("GPU", &stats.gpu_temp, &stats.gpu_fan));
    if let Some(percent) = known(&stats.battery_percent) {
        let source = match stats.on_ac {
            Some(true) => ", от сети",
            Some(false) => ", от батареи",
            None => "",
        };
        lines.push(format!("Батарея: {percent}{source}"));
    }
    if !lines.is_empty() && !stats.fresh {
        lines.push("Данные устарели".into());
    }
    lines.join("<br/>")
}

/// Publish the latest telemetry to the tray tooltip; a changed text re-notifies
/// the host. Works before the tray is registered (the text is kept).
pub(crate) fn publish_stats(stats: &TrayStats) {
    let description = tooltip_description(stats);
    let mut current = TOOLTIP_DESCRIPTION
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if *current != description {
        *current = description;
        TOOLTIP_CHANGED.notify_one();
    }
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;
}

#[derive(Debug, Clone, Copy)]
enum TrayCommand {
    Activate,
}

/// SNI `ToolTip`: icon name, pixmaps, title, description.
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);

struct StatusNotifierItem {
    commands: UnboundedSender<TrayCommand>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        "orbis-control".into()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        "Orbis Control".into()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }

    #[zbus(property)]
    fn window_id(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        "io.github.orbiscontrol.Orbis".into()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        let description = TOOLTIP_DESCRIPTION
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (
            "io.github.orbiscontrol.Orbis".into(),
            Vec::new(),
            "Orbis Control".into(),
            description,
        )
    }

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::zvariant::ObjectPath<'static> {
        zbus::zvariant::ObjectPath::try_from("/NO_DBUSMENU")
            .expect("fixed StatusNotifierItem no-menu object path")
    }

    fn activate(&self, _x: i32, _y: i32) {
        let _ = self.commands.send(TrayCommand::Activate);
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.commands.send(TrayCommand::Activate);
    }

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}
}

#[derive(Clone)]
struct TrayContext {
    runtime: tokio::runtime::Handle,
    started: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
}

thread_local! {
    static CONTEXT: RefCell<Option<TrayContext>> = const { RefCell::new(None) };
}

pub(crate) fn initialize(runtime: tokio::runtime::Handle) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(TrayContext {
            runtime,
            started: Arc::new(AtomicBool::new(false)),
            ready: Arc::new(AtomicBool::new(false)),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow().as_ref() {
            context.ready.store(false, Ordering::Release);
        }
        *slot.borrow_mut() = None;
    });
}

/// True only after the standard watcher reports a currently registered host and
/// accepts this exact item registration. Hiding the main window is unsafe when
/// false because no user-visible activation path is proven.
pub(crate) fn is_ready() -> bool {
    CONTEXT.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|context| context.ready.load(Ordering::Acquire))
    })
}

pub(crate) fn wire_app(app: &AppWindow) {
    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        return;
    };
    if context.started.swap(true, Ordering::AcqRel) {
        return;
    }

    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    let weak = app.as_weak();
    context.runtime.spawn(async move {
        while let Some(command) = command_rx.recv().await {
            match command {
                TrayCommand::Activate => {
                    let weak = weak.clone();
                    if let Err(error) = weak.upgrade_in_event_loop(move |app| {
                        // Like G-Helper: the tray icon toggles the window
                        // (window_placement docks it above the icon).
                        crate::toggle_main_window(&app);
                    }) {
                        tracing::debug!(error = ?error, "tray activation UI dispatch failed");
                    }
                }
            }
        }
    });

    let ready = context.ready.clone();
    context.runtime.spawn(async move {
        if let Err(error) = run_item_service(command_tx, ready.clone()).await {
            ready.store(false, Ordering::Release);
            tracing::warn!(error = ?error, "StatusNotifierItem backend unavailable");
        }
    });
}

async fn run_item_service(
    commands: UnboundedSender<TrayCommand>,
    ready: Arc<AtomicBool>,
) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    connection
        .object_server()
        .at(ITEM_PATH, StatusNotifierItem { commands })
        .await?;

    let service_name = format!(
        "org.freedesktop.StatusNotifierItem-{}-1",
        std::process::id()
    );
    connection.request_name(service_name.as_str()).await?;

    let item = connection
        .object_server()
        .interface::<_, StatusNotifierItem>(ITEM_PATH)
        .await?;
    tokio::spawn(async move {
        loop {
            TOOLTIP_CHANGED.notified().await;
            if let Err(error) = StatusNotifierItem::new_tool_tip(item.signal_emitter()).await {
                tracing::debug!(error = ?error, "StatusNotifierItem tooltip signal failed");
            }
        }
    });

    loop {
        let watcher = StatusNotifierWatcherProxy::new(&connection).await;
        match watcher {
            Ok(watcher) => match watcher.is_status_notifier_host_registered().await {
                Ok(true) => match watcher.register_status_notifier_item(&service_name).await {
                    Ok(()) => ready.store(true, Ordering::Release),
                    Err(error) => {
                        ready.store(false, Ordering::Release);
                        tracing::debug!(error = ?error, "StatusNotifierWatcher registration failed");
                    }
                },
                Ok(false) => ready.store(false, Ordering::Release),
                Err(error) => {
                    ready.store(false, Ordering::Release);
                    tracing::debug!(error = ?error, "StatusNotifierWatcher host-state read failed");
                }
            },
            Err(error) => {
                ready.store(false, Ordering::Release);
                tracing::debug!(error = ?error, "StatusNotifierWatcher unavailable");
            }
        }
        tokio::time::sleep(RECHECK_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> TrayStats {
        TrayStats {
            cpu_temp: "46°C".into(),
            gpu_temp: "—".into(),
            cpu_fan: "2600 rpm".into(),
            gpu_fan: "2100 rpm".into(),
            battery_percent: "87%".into(),
            on_ac: Some(false),
            fresh: true,
        }
    }

    #[test]
    fn tooltip_lists_known_values_only() {
        assert_eq!(
            tooltip_description(&stats()),
            "CPU: 46°C, 2600 rpm<br/>GPU: 2100 rpm<br/>Батарея: 87%, от батареи"
        );
    }

    #[test]
    fn tooltip_marks_stale_data_and_stays_empty_without_values() {
        let mut stale = stats();
        stale.fresh = false;
        assert!(tooltip_description(&stale).ends_with("<br/>Данные устарели"));
        assert_eq!(tooltip_description(&TrayStats::default()), "");
    }

    #[test]
    fn tray_has_no_hardware_process_or_generic_dbus_surface() {
        let source = include_str!("tray_backend.rs");
        for forbidden in [
            ["WorkerCommand::", "Set"].concat(),
            ["Command", "::new"].concat(),
            ["std::fs::", "write"].concat(),
            ["set_", "gpu_mode"].concat(),
            ["set_", "fan_curve"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "unexpected tray surface: {forbidden}"
            );
        }
    }
}
