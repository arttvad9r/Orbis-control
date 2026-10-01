//! Tray icon (StatusNotifierItem) with a right-click menu (DBusMenu), via
//! the `ksni` crate.
//!
//! Left click toggles the main window, like G-Helper. The menu opens the
//! windows, switches the performance profile through the main window's own
//! request path (same gates as its buttons) and quits the application. There
//! is no shell helper, subprocess, generic D-Bus surface or hardware access
//! here.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, RadioGroup, RadioItem, StandardItem};
use slint::ComponentHandle;

use crate::AppWindow;

const ICON_NAME: &str = "io.github.orbiscontrol.Orbis";
const PROFILES: [&str; 3] = ["Тихий", "Баланс", "Турбо"];

/// Set by the tray thread while a StatusNotifier host shows the icon.
static READY: AtomicBool = AtomicBool::new(false);

/// Latest state shown by the tray; unknown values are "—" or empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TrayStats {
    pub cpu_temp: String,
    pub gpu_temp: String,
    pub cpu_fan: String,
    pub gpu_fan: String,
    pub battery_percent: String,
    pub on_ac: Option<bool>,
    pub fresh: bool,
    /// Selected profile index (0 quiet, 1 balanced, 2 turbo) when profiles
    /// are readable; `None` hides the profile choice.
    pub profile: Option<usize>,
    /// Per-profile availability and the write gate for the radio items.
    pub profiles_available: [bool; 3],
    pub profiles_writable: bool,
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

struct OrbisTray {
    app: slint::Weak<AppWindow>,
    stats: TrayStats,
}

impl OrbisTray {
    /// Run `action` on the UI thread with the main window.
    fn on_ui(&self, action: impl FnOnce(AppWindow) + Send + 'static) {
        if let Err(error) = self.app.upgrade_in_event_loop(action) {
            tracing::debug!(?error, "tray action dispatch failed");
        }
    }

    fn item(label: &str, action: fn(&AppWindow)) -> MenuItem<Self> {
        StandardItem {
            label: label.into(),
            activate: Box::new(move |tray: &mut Self| tray.on_ui(move |app| action(&app))),
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for OrbisTray {
    fn id(&self) -> String {
        "orbis-control".into()
    }

    fn title(&self) -> String {
        "Orbis Control".into()
    }

    fn icon_name(&self) -> String {
        ICON_NAME.into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: ICON_NAME.into(),
            title: "Orbis Control".into(),
            description: tooltip_description(&self.stats),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Like G-Helper: the icon toggles the window (window_placement docks
        // it above the icon).
        self.on_ui(|app| crate::toggle_main_window(&app));
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = vec![Self::item("Открыть Orbis Control", crate::show_main_window)];
        if let Some(selected) = self.stats.profile {
            items.push(MenuItem::Separator);
            items.push(
                RadioGroup {
                    selected,
                    select: Box::new(|tray: &mut Self, index| {
                        tray.on_ui(move |app| app.invoke_perf_clicked(index as i32));
                    }),
                    options: PROFILES
                        .iter()
                        .zip(self.stats.profiles_available)
                        .map(|(label, available)| RadioItem {
                            label: (*label).into(),
                            enabled: available && self.stats.profiles_writable,
                            ..Default::default()
                        })
                        .collect(),
                }
                .into(),
            );
        }
        items.extend([
            MenuItem::Separator,
            Self::item("Вентиляторы и мощность", crate::panel_windows::open_fans),
            Self::item("Дополнительно", crate::panel_windows::open_extra),
            MenuItem::Separator,
            StandardItem {
                label: "Выход".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.on_ui(|app| crate::quick_controls_backend::quit(&app));
                }),
                ..Default::default()
            }
            .into(),
        ]);
        items
    }

    fn watcher_online(&self) {
        READY.store(true, Ordering::Release);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        READY.store(false, Ordering::Release);
        // Keep the service: the host may come back (Plasma restart).
        true
    }
}

thread_local! {
    static HANDLE: RefCell<Option<ksni::blocking::Handle<OrbisTray>>> = const { RefCell::new(None) };
}

/// True while a StatusNotifier host shows the icon. Hiding the main window is
/// unsafe when false because there would be no way to bring it back.
pub(crate) fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

pub(crate) fn wire_app(app: &AppWindow) {
    if HANDLE.with(|handle| handle.borrow().is_some()) {
        return;
    }
    let tray = OrbisTray {
        app: app.as_weak(),
        stats: TrayStats::default(),
    };
    // ksni reports a missing or lost watcher through `watcher_offline` (also
    // during spawn) and calls `watcher_online` only after such a loss, so the
    // icon counts as shown from the start until told otherwise. Set before
    // spawning so an early offline report is not overwritten.
    READY.store(true, Ordering::Release);
    // Also waits for a watcher that appears later (login before the panel).
    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => HANDLE.with(|slot| *slot.borrow_mut() = Some(handle)),
        Err(error) => {
            READY.store(false, Ordering::Release);
            tracing::warn!(%error, "tray icon unavailable");
        }
    }
}

/// Publish the latest state to the tray (tooltip and menu); unchanged state
/// is not re-sent.
pub(crate) fn publish_stats(stats: &TrayStats) {
    HANDLE.with(|handle| {
        if let Some(handle) = handle.borrow().as_ref() {
            let stats = stats.clone();
            handle.update(move |tray| {
                if tray.stats != stats {
                    tray.stats = stats;
                }
            });
        }
    });
}

pub(crate) fn clear() {
    HANDLE.with(|handle| {
        if let Some(handle) = handle.borrow_mut().take() {
            handle.shutdown().wait();
        }
    });
    READY.store(false, Ordering::Release);
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
            ..Default::default()
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
