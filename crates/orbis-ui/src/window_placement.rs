//! G-Helper-style window placement: the main window sits at the bottom of
//! the work area above the tray icon (or in the bottom-right corner), and
//! the "Fans and power" / "Extra" windows line up to its left.
//!
//! Wayland clients cannot position their own windows, so on KDE the layout
//! is applied by a one-shot KWin script loaded over D-Bus
//! (`org.kde.KWin /Scripting`), the compositor's own scripting interface.
//! Window rules cannot express positions relative to other windows. Outside
//! KWin the D-Bus call fails and the windows stay where the window manager
//! put them.

use std::sync::Mutex;
use std::time::Duration;

/// Must match the `title` of the windows in ui/main-window.slint and
/// ui/panels.slint.
const MAIN_TITLE: &str = "Orbis Control";
const FANS_TITLE: &str = "Вентиляторы и мощность";
const EXTRA_TITLE: &str = "Дополнительно";
const PLUGIN_NAME: &str = "orbis-control-placement";
/// Let the compositor map a freshly shown window before arranging.
const MAP_DELAY: Duration = Duration::from_millis(150);

/// Last tray click position (global coordinates), used as the anchor.
static TRAY_ANCHOR: Mutex<Option<(i32, i32)>> = Mutex::new(None);

pub(crate) fn remember_tray_click(x: i32, y: i32) {
    // Hosts that do not know the position send (0, 0).
    let anchor = (x != 0 || y != 0).then_some((x, y));
    *TRAY_ANCHOR.lock().unwrap_or_else(|e| e.into_inner()) = anchor;
}

/// Arrange the visible Orbis windows shortly after a window was shown.
pub(crate) fn arrange_soon() {
    slint::Timer::single_shot(MAP_DELAY, || {
        let anchor = *TRAY_ANCHOR.lock().unwrap_or_else(|e| e.into_inner());
        let script = script(std::process::id(), anchor.map(|(x, _)| x));
        // D-Bus round trips stay off the UI thread.
        std::thread::spawn(move || {
            if let Err(error) = run_kwin_script(&script) {
                tracing::debug!(%error, "KWin window placement unavailable");
            }
        });
    });
}

fn script(pid: u32, anchor_x: Option<i32>) -> String {
    let anchor = anchor_x.map_or("null".to_string(), |x| x.to_string());
    format!(
        r#"(function () {{
    const pid = {pid};
    const anchorX = {anchor};
    const gap = 8;
    const area = workspace.clientArea(KWin.MaximizeArea, workspace.activeScreen, workspace.currentDesktop);
    let main = null, fans = null, extra = null;
    for (const w of workspace.windowList()) {{
        if (w.pid !== pid || !w.normalWindow || w.minimized) continue;
        if (w.caption === {main_title:?}) main = w;
        else if (w.caption === {fans_title:?}) fans = w;
        else if (w.caption === {extra_title:?}) extra = w;
    }}
    if (!main) return;
    const bottom = (h) => Math.max(area.y + gap, area.y + area.height - h - gap);
    const g = main.frameGeometry;
    let x = anchorX === null ? area.x + area.width - g.width - gap : anchorX - g.width / 2;
    x = Math.round(Math.max(area.x + gap, Math.min(x, area.x + area.width - g.width - gap)));
    main.frameGeometry = {{ x: x, y: bottom(g.height), width: g.width, height: g.height }};
    let left = x;
    for (const w of [fans, extra]) {{
        if (!w) continue;
        const f = w.frameGeometry;
        const nx = Math.max(area.x + gap, left - f.width - gap);
        w.frameGeometry = {{ x: nx, y: bottom(f.height), width: f.width, height: f.height }};
        left = nx;
    }}
}})();
"#,
        main_title = MAIN_TITLE,
        fans_title = FANS_TITLE,
        extra_title = EXTRA_TITLE,
    )
}

fn run_kwin_script(script: &str) -> anyhow::Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("orbis-control");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("placement.js");
    std::fs::write(&path, script)?;
    let path = path.to_string_lossy().into_owned();

    let connection = zbus::blocking::Connection::session()?;
    let scripting = zbus::blocking::Proxy::new(
        &connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;
    // A previous run that failed half-way may have left the plugin loaded.
    let _: bool = scripting.call("unloadScript", &(PLUGIN_NAME,))?;
    let id: i32 = scripting.call("loadScript", &(path.as_str(), PLUGIN_NAME))?;
    let result = (|| -> anyhow::Result<()> {
        let script = zbus::blocking::Proxy::new(
            &connection,
            "org.kde.KWin",
            format!("/Scripting/Script{id}"),
            "org.kde.kwin.Script",
        )?;
        script.call::<_, _, ()>("run", &())?;
        Ok(())
    })();
    let _: bool = scripting.call("unloadScript", &(PLUGIN_NAME,))?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_targets_only_this_process_and_known_windows() {
        let text = script(4242, Some(1700));
        assert!(text.contains("const pid = 4242;"));
        assert!(text.contains("const anchorX = 1700;"));
        for title in [MAIN_TITLE, FANS_TITLE, EXTRA_TITLE] {
            assert!(text.contains(&format!("{title:?}")), "{title}");
        }
        assert!(script(1, None).contains("const anchorX = null;"));
    }
}
