//! G-Helper-style window placement: the main window sits at the bottom of
//! the work area above the tray icon (or in the bottom-right corner), and
//! the "Fans and power" / "Extra" windows line up to its left.
//!
//! Wayland clients cannot position their own windows, so on KDE the layout
//! is done by a KWin script loaded over D-Bus (`org.kde.KWin /Scripting`),
//! the compositor's own scripting interface; window rules cannot express
//! positions relative to other windows. The script runs for the whole
//! session of this process and places each Orbis window the moment KWin
//! adds it, so a window never shows up elsewhere first. Outside KWin the
//! D-Bus call fails and the windows stay where the window manager put them.

/// Must match the `title` of the windows in ui/main-window.slint and
/// ui/panels.slint.
const MAIN_TITLE: &str = "Orbis Control";
const FANS_TITLE: &str = "Вентиляторы и мощность";
const EXTRA_TITLE: &str = "Дополнительно";
const PLUGIN_NAME: &str = "orbis-control-placement";

/// Load the placement script before the first window is shown (a couple
/// of local D-Bus calls); failures are logged and placement is skipped.
pub(crate) fn install() {
    if let Err(error) = load(&script(std::process::id())) {
        tracing::debug!(%error, "KWin window placement unavailable");
    }
}

/// Unload the script on exit. A crashed process leaves it loaded but
/// inert (it only touches windows of its own pid); the next start replaces it.
pub(crate) fn uninstall() {
    if let Ok(scripting) = scripting_proxy() {
        let _: Result<bool, _> = scripting.1.call("unloadScript", &(PLUGIN_NAME,));
    }
}

fn script(pid: u32) -> String {
    format!(
        r#"const pid = {pid};
const gap = 8;
const titles = {{ main: {main:?}, fans: {fans:?}, extra: {extra:?} }};
// Tray click position: when the main window appears while the pointer is
// below the work area (on the panel), it opens above the pointer.
let anchorX = null;

function area() {{
    return workspace.clientArea(KWin.MaximizeArea, workspace.activeScreen, workspace.currentDesktop);
}}
function ours(w) {{
    return w && w.pid === pid && w.normalWindow && !w.minimized;
}}
function find(title) {{
    for (const w of workspace.windowList()) {{
        if (ours(w) && w.caption === title) return w;
    }}
    return null;
}}
function place(w, x, a) {{
    const g = w.frameGeometry;
    const y = Math.max(a.y + gap, a.y + a.height - g.height - gap);
    x = Math.round(x);
    if (g.x !== x || g.y !== y) w.frameGeometry = {{ x: x, y: y, width: g.width, height: g.height }};
}}
function arrange() {{
    const main = find(titles.main);
    if (!main) return;
    const a = area();
    const g = main.frameGeometry;
    let x = anchorX === null ? a.x + a.width - g.width - gap : anchorX - g.width / 2;
    x = Math.max(a.x + gap, Math.min(x, a.x + a.width - g.width - gap));
    place(main, x, a);
    let left = x;
    for (const title of [titles.fans, titles.extra]) {{
        const w = find(title);
        if (!w) continue;
        left = Math.max(a.x + gap, left - w.frameGeometry.width - gap);
        place(w, left, a);
    }}
}}
function track(w) {{
    if (!w || w.pid !== pid) return;
    if (w.caption === titles.main) {{
        const a = area();
        const c = workspace.cursorPos;
        anchorX = c.y >= a.y + a.height ? c.x : null;
    }}
    // Re-dock when a window's size changes (sections loading), never on a
    // move, so the user can still drag the windows around.
    w.frameGeometryChanged.connect(function (old) {{
        const g = w.frameGeometry;
        if (old && (old.width !== g.width || old.height !== g.height)) arrange();
    }});
    w.captionChanged.connect(arrange);
    arrange();
}}

workspace.windowAdded.connect(track);
workspace.windowRemoved.connect(function (w) {{
    if (w && w.pid === pid) arrange();
}});
for (const w of workspace.windowList()) track(w);
"#,
        main = MAIN_TITLE,
        fans = FANS_TITLE,
        extra = EXTRA_TITLE,
    )
}

fn scripting_proxy() -> anyhow::Result<(zbus::blocking::Connection, zbus::blocking::Proxy<'static>)>
{
    let connection = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;
    Ok((connection, proxy))
}

fn load(script: &str) -> anyhow::Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("orbis-control");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("placement.js");
    std::fs::write(&path, script)?;
    let path = path.to_string_lossy().into_owned();

    let (connection, scripting) = scripting_proxy()?;
    // Replace a script left behind by an earlier (crashed) instance.
    let _: bool = scripting.call("unloadScript", &(PLUGIN_NAME,))?;
    let id: i32 = scripting.call("loadScript", &(path.as_str(), PLUGIN_NAME))?;
    let script = zbus::blocking::Proxy::new(
        &connection,
        "org.kde.KWin",
        format!("/Scripting/Script{id}"),
        "org.kde.kwin.Script",
    )?;
    script.call::<_, _, ()>("run", &())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_targets_only_this_process_and_known_windows() {
        let text = script(4242);
        assert!(text.contains("const pid = 4242;"));
        for title in [MAIN_TITLE, FANS_TITLE, EXTRA_TITLE] {
            assert!(text.contains(&format!("{title:?}")), "{title}");
        }
    }
}
