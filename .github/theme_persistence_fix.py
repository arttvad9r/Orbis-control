from pathlib import Path

main = Path("crates/orbis-ui/src/main.rs")
text = main.read_text()
old = '''            let app_weak = app.as_weak();
            let window_weak = window.as_weak();
            window.on_theme_changed(move |light| {
                if let Some(app) = app_weak.upgrade() {
                    apply_theme_to_all(&app, light);
                }

                match persist_theme(light) {
                    Ok(()) => {
                        if let Some(window) = window_weak.upgrade() {
                            let status = if light {
                                "Light theme active · saved"
                            } else {
                                "Dark theme active · saved"
                            };
                            window.set_local_status(status.into());
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "theme preference save failed; runtime theme remains active"
                        );
                        if let Some(window) = window_weak.upgrade() {
                            window.set_local_status(
                                "Theme active for this session · save failed; see logs".into(),
                            );
                        }
                    }
                }
            });'''
new = '''            let app_weak = app.as_weak();
            window.on_theme_changed(move |light| {
                if let Some(app) = app_weak.upgrade() {
                    apply_theme_to_all(&app, light);
                }

                if let Err(error) = persist_theme(light) {
                    tracing::warn!(
                        error = %error,
                        "theme preference save failed; runtime theme remains active"
                    );
                }
            });'''
if text.count(old) != 1:
    raise SystemExit(f"expected one generated callback, found {text.count(old)}")
main.write_text(text.replace(old, new, 1))

preferences_ui = Path("ui/audited/preferences-window.slint")
text = preferences_ui.read_text()
old = 'root.local-status = light ? "Light theme active · saving…" : "Dark theme active · saving…";'
new = 'root.local-status = light ? "Light theme active · save requested" : "Dark theme active · save requested";'
if text.count(old) != 1:
    raise SystemExit(f"expected one generated status line, found {text.count(old)}")
preferences_ui.write_text(text.replace(old, new, 1))
