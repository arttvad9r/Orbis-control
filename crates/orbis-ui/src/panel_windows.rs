//! "Fans and power" and "Extra" as separate windows (G-Helper style).
//!
//! The main window stays the single state holder that the rest of the UI
//! code writes to. While a secondary window is open, its properties are
//! mirrored from the main window on a short timer and its requests are
//! forwarded to the main window's callbacks, so every handler, gate and
//! read-back path is shared.

use std::cell::RefCell;
use std::time::Duration;

use slint::ComponentHandle;

use crate::{AppWindow, ExtraWindow, FansWindow, ThemeState};

/// Mirror period; property writes with unchanged values are no-ops.
const SYNC_INTERVAL: Duration = Duration::from_millis(100);

thread_local! {
    static WINDOWS: RefCell<Windows> = RefCell::new(Windows::default());
}

#[derive(Default)]
struct Windows {
    fans: Option<FansWindow>,
    extra: Option<ExtraWindow>,
    timer: Option<slint::Timer>,
}

pub(crate) fn wire(app: &AppWindow) {
    let weak = app.as_weak();
    app.on_fans_toggled(move |open| {
        if let Some(app) = weak.upgrade() {
            set_fans_open(&app, open);
        }
    });
    let weak = app.as_weak();
    app.on_extra_toggled(move |open| {
        if let Some(app) = weak.upgrade() {
            set_extra_open(&app, open);
        }
    });
}

fn set_fans_open(app: &AppWindow, open: bool) {
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        if open {
            if windows.fans.is_none() {
                match FansWindow::new() {
                    Ok(window) => {
                        forward_fans(app, &window);
                        let weak = app.as_weak();
                        window.window().on_close_requested(move || {
                            if let Some(app) = weak.upgrade() {
                                app.set_fans_open(false);
                            }
                            slint::CloseRequestResponse::HideWindow
                        });
                        windows.fans = Some(window);
                    }
                    Err(error) => {
                        tracing::warn!(?error, "fans window could not be created");
                        app.set_fans_open(false);
                        return;
                    }
                }
            }
            let window = windows.fans.as_ref().expect("created above");
            sync_fans(app, window);
            if let Err(error) = window.show() {
                tracing::warn!(?error, "fans window could not be shown");
                app.set_fans_open(false);
            }
        } else if let Some(window) = &windows.fans {
            let _ = window.hide();
        }
        update_timer(app, &mut windows);
    });
}

fn set_extra_open(app: &AppWindow, open: bool) {
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        if open {
            if windows.extra.is_none() {
                match ExtraWindow::new() {
                    Ok(window) => {
                        forward_extra(app, &window);
                        let weak = app.as_weak();
                        window.window().on_close_requested(move || {
                            if let Some(app) = weak.upgrade() {
                                app.set_extra_open(false);
                            }
                            slint::CloseRequestResponse::HideWindow
                        });
                        windows.extra = Some(window);
                    }
                    Err(error) => {
                        tracing::warn!(?error, "extra window could not be created");
                        app.set_extra_open(false);
                        return;
                    }
                }
            }
            let window = windows.extra.as_ref().expect("created above");
            sync_extra(app, window);
            if let Err(error) = window.show() {
                tracing::warn!(?error, "extra window could not be shown");
                app.set_extra_open(false);
            }
        } else if let Some(window) = &windows.extra {
            let _ = window.hide();
        }
        update_timer(app, &mut windows);
    });
}

/// Open the "Fans and power" window (tray menu).
pub(crate) fn open_fans(app: &AppWindow) {
    app.set_fans_open(true);
    set_fans_open(app, true);
}

/// Open the "Extra" window (tray menu).
pub(crate) fn open_extra(app: &AppWindow) {
    app.set_extra_open(true);
    set_extra_open(app, true);
}

/// Run the mirror timer only while a secondary window is open.
fn update_timer(app: &AppWindow, windows: &mut Windows) {
    let needed = app.get_fans_open() || app.get_extra_open();
    if !needed {
        windows.timer = None;
        return;
    }
    if windows.timer.is_some() {
        return;
    }
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    timer.start(slint::TimerMode::Repeated, SYNC_INTERVAL, move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        WINDOWS.with(|windows| {
            let windows = windows.borrow();
            if app.get_fans_open()
                && let Some(window) = &windows.fans
            {
                sync_fans(&app, window);
            }
            if app.get_extra_open()
                && let Some(window) = &windows.extra
            {
                sync_extra(&app, window);
            }
        });
    });
    windows.timer = Some(timer);
}

/// Hide both windows (main window hidden to the tray or app quitting).
pub(crate) fn close_all(app: &AppWindow) {
    if app.get_fans_open() {
        app.set_fans_open(false);
        set_fans_open(app, false);
    }
    if app.get_extra_open() {
        app.set_extra_open(false);
        set_extra_open(app, false);
    }
}

pub(crate) fn clear() {
    WINDOWS.with(|windows| *windows.borrow_mut() = Windows::default());
}

// Generated-shape mirroring/forwarding, shared with the snapshot example.
include!("panel_sync.rs");
