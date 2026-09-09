use std::cell::RefCell;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_ui::diagnostics_dto::DiagnosticsUiDto;
use orbis_ui::diagnostics_export::{report_json, summary_text};
use orbis_ui::diagnostics_runtime::DiagnosticsRuntime;
use orbis_ui::diagnostics_window_model::DiagnosticsWindowModel;
use slint::ComponentHandle;

use crate::AppWindow;

const EXPORT_DIR_NAME: &str = "diagnostics";
const EXPORT_FILE_PREFIX: &str = "orbis-diagnostics";

#[derive(Clone)]
struct DiagnosticsContext {
    runtime: tokio::runtime::Handle,
    source: DiagnosticsRuntime,
    latest: Arc<Mutex<Option<DiagnosticsUiDto>>>,
}

thread_local! {
    static CONTEXT: RefCell<Option<DiagnosticsContext>> = const { RefCell::new(None) };
}

pub(crate) fn initialize(
    runtime: tokio::runtime::Handle,
    session_connection: zbus::Connection,
    system_connection: zbus::Connection,
    capabilities: Arc<CapabilityRegistrySnapshot>,
    telemetry_freshness_threshold: Duration,
) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(DiagnosticsContext {
            runtime,
            source: DiagnosticsRuntime::new(
                session_connection,
                system_connection,
                capabilities,
                telemetry_freshness_threshold,
            ),
            latest: Arc::new(Mutex::new(None)),
        });
    });
}

pub(crate) fn replace_capabilities(snapshot: Arc<CapabilityRegistrySnapshot>) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow().as_ref() {
            context.source.replace_capabilities(snapshot);
        }
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

fn apply_model(window: &AppWindow, model: DiagnosticsWindowModel) {
    // The full text panels moved out of the UI with the dedicated window; the
    // privacy-safe summary stays available to Copy/Export (spec §2.4).
    let _ = window;
    let _ = model;
}

fn store_latest(context: &DiagnosticsContext, dto: DiagnosticsUiDto) -> bool {
    match context.latest.lock() {
        Ok(mut latest) => {
            *latest = Some(dto);
            true
        }
        Err(_) => {
            tracing::warn!("diagnostics latest-snapshot lock poisoned; export disabled");
            false
        }
    }
}

fn latest_snapshot(context: &DiagnosticsContext) -> Option<DiagnosticsUiDto> {
    match context.latest.lock() {
        Ok(latest) => latest.clone(),
        Err(_) => {
            tracing::warn!("diagnostics latest-snapshot lock poisoned");
            None
        }
    }
}

fn export_path_available() -> bool {
    orbis_config::paths::state_dir_checked().is_ok()
}

pub(crate) fn refresh(window: &AppWindow) {
    if window.get_refresh_pending() {
        return;
    }

    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        window.set_refresh_enabled(false);
        window.set_refresh_pending(false);
        window.set_export_enabled(false);
        window.set_diagnostics_status("Diagnostics runtime unavailable".into());
        return;
    };

    window.set_refresh_enabled(false);
    window.set_refresh_pending(true);
    window.set_export_enabled(false);
    window.set_diagnostics_status("Collecting read-only snapshot…".into());

    let runtime = context.runtime.clone();
    let source = context.source.clone();
    let publish_context = context.clone();
    let weak = window.as_weak();
    runtime.spawn(async move {
        let snapshot = source.snapshot().await;
        let dto = DiagnosticsUiDto::from_snapshot(&snapshot);
        let model = DiagnosticsWindowModel::from_dto(&dto);
        let summary: slint::SharedString = summary_text(&dto).into();
        let export_ready = store_latest(&publish_context, dto) && export_path_available();

        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            apply_model(&window, model);
            window.set_diagnostics_summary(summary);
            window.set_refresh_pending(false);
            window.set_refresh_enabled(true);
            window.set_diagnostics_status(
                if export_ready {
                    "Snapshot refreshed"
                } else {
                    "Snapshot refreshed · export unavailable"
                }
                .into(),
            );
            // Copy is served in-section through the toolkit clipboard from the
            // privacy-safe summary; Open Logs remains absent until a stable
            // host integration exists (#111).
            window.set_export_enabled(export_ready);
        }) {
            tracing::warn!(error = ?error, "failed to publish diagnostics snapshot to UI");
        }
    });
}

fn export_report(window: &AppWindow) {
    if window.get_refresh_pending() {
        return;
    }

    let context = CONTEXT.with(|slot| slot.borrow().clone());
    let Some(context) = context else {
        window.set_export_enabled(false);
        window.set_diagnostics_status("Diagnostics runtime unavailable".into());
        return;
    };

    let Some(dto) = latest_snapshot(&context) else {
        window.set_export_enabled(false);
        window.set_diagnostics_status("Refresh diagnostics before exporting".into());
        return;
    };

    if !export_path_available() {
        window.set_export_enabled(false);
        window.set_diagnostics_status("Diagnostics export path unavailable".into());
        return;
    }

    // Refresh and export are serialized so an older export completion cannot
    // re-enable actions over a newer pending snapshot.
    window.set_refresh_enabled(false);
    window.set_export_enabled(false);
    window.set_diagnostics_status("Exporting privacy-safe report…".into());

    let weak = window.as_weak();
    context.runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(move || write_report(&dto)).await;
        if let Err(error) = weak.upgrade_in_event_loop(move |window| {
            window.set_refresh_enabled(true);
            match result {
                Ok(Ok(path)) => {
                    window.set_export_enabled(true);
                    window.set_diagnostics_status(format!("Exported · {}", path.display()).into());
                }
                Ok(Err(error)) => {
                    tracing::warn!(error = %error, "diagnostics report export failed");
                    window.set_export_enabled(false);
                    window.set_diagnostics_status("Report export failed · refresh to retry".into());
                }
                Err(error) => {
                    tracing::warn!(error = %error, "diagnostics report export task failed");
                    window.set_export_enabled(false);
                    window.set_diagnostics_status("Report export failed · refresh to retry".into());
                }
            }
        }) {
            tracing::warn!(error = ?error, "failed to publish diagnostics export result to UI");
        }
    });
}

fn write_report(dto: &DiagnosticsUiDto) -> anyhow::Result<PathBuf> {
    let state_dir = orbis_config::paths::state_dir_checked()
        .context("resolve fail-closed XDG state directory for diagnostics export")?;
    write_report_to_dir(dto, &state_dir.join(EXPORT_DIR_NAME))
}

fn write_report_to_dir(dto: &DiagnosticsUiDto, dir: &Path) -> anyhow::Result<PathBuf> {
    let contents = report_json(dto).context("serialize privacy-bounded diagnostics report")?;
    write_report_text_to_dir(&contents, dto.generated_at, dir)
}

fn write_report_text_to_dir(
    contents: &str,
    generated_at: SystemTime,
    dir: &Path,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("create diagnostics export directory {dir:?}"))?;

    let stamp = generated_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for suffix in 0..64_u32 {
        let file_name = if suffix == 0 {
            format!("{EXPORT_FILE_PREFIX}-{stamp}.json")
        } else {
            format!("{EXPORT_FILE_PREFIX}-{stamp}-{suffix}.json")
        };
        let path = dir.join(file_name);

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);

        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("create diagnostics report {path:?}"));
            }
        };

        let write_result = (|| -> anyhow::Result<()> {
            file.write_all(contents.as_bytes())
                .with_context(|| format!("write diagnostics report {path:?}"))?;
            file.flush()
                .with_context(|| format!("flush diagnostics report {path:?}"))?;
            file.sync_all()
                .with_context(|| format!("sync diagnostics report {path:?}"))?;
            Ok(())
        })();

        if let Err(error) = write_result {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        drop(file);
        sync_export_directory(dir)?;
        return Ok(path);
    }

    anyhow::bail!("could not allocate a unique diagnostics report filename")
}

#[cfg(unix)]
fn sync_export_directory(dir: &Path) -> anyhow::Result<()> {
    File::open(dir)
        .with_context(|| format!("open diagnostics export directory for sync {dir:?}"))?
        .sync_all()
        .with_context(|| format!("sync diagnostics export directory {dir:?}"))
}

#[cfg(not(unix))]
fn sync_export_directory(_dir: &Path) -> anyhow::Result<()> {
    Ok(())
}

pub(crate) fn wire_window(window: &AppWindow) {
    window.set_refresh_enabled(CONTEXT.with(|slot| slot.borrow().is_some()));
    window.set_export_enabled(false);

    {
        let weak = window.as_weak();
        window.on_diagnostics_refresh_requested(move || {
            if let Some(window) = weak.upgrade() {
                refresh(&window);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_diagnostics_export_requested(move || {
            if let Some(window) = weak.upgrade() {
                export_report(&window);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn bridge_source_contains_no_hardware_mutation_commands() {
        let source = include_str!("diagnostics_backend.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "fan_curve"].concat(),
            ["set_", "gpu_mode"].concat(),
            ["set_", "charge_limit"].concat(),
            ["set_", "performance"].concat(),
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "unexpected mutation token: {needle}"
            );
        }
    }

    #[test]
    fn report_files_are_private_unique_and_never_overwrite() {
        let td = tempfile::tempdir().expect("tempdir");
        let generated_at = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let first = write_report_text_to_dir("{\"schema_version\":1}\n", generated_at, td.path())
            .expect("first export");
        let second = write_report_text_to_dir("{\"schema_version\":1}\n", generated_at, td.path())
            .expect("second export");

        assert_ne!(first, second);
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "{\"schema_version\":1}\n"
        );
        assert_eq!(
            fs::read_to_string(&second).unwrap(),
            "{\"schema_version\":1}\n"
        );

        #[cfg(unix)]
        {
            let first_mode = fs::metadata(&first).unwrap().permissions().mode() & 0o777;
            let second_mode = fs::metadata(&second).unwrap().permissions().mode() & 0o777;
            assert_eq!(first_mode, 0o600);
            assert_eq!(second_mode, 0o600);
        }
    }

    #[test]
    fn export_host_path_is_fixed_under_checked_xdg_state() {
        let source = include_str!("diagnostics_backend.rs");
        assert!(source.contains("state_dir_checked"));
        assert!(source.contains("EXPORT_DIR_NAME"));
        assert!(!source.contains(&["std::env::", "current_dir"].concat()));
        assert!(!source.contains(&["Command", "::new"].concat()));
    }

    #[test]
    fn refresh_and_export_are_mutually_exclusive_in_host_bridge() {
        let source = include_str!("diagnostics_backend.rs");
        assert!(source.contains("window.set_refresh_enabled(false);"));
        assert!(source.contains("window.set_export_enabled(false);"));
        assert!(source.contains("window.set_refresh_enabled(true);"));
        assert!(source.contains("if window.get_refresh_pending()"));
    }

    #[test]
    fn successful_refresh_fills_summary_for_in_section_copy() {
        let source = include_str!("diagnostics_backend.rs");
        assert!(source.contains("window.set_diagnostics_summary(summary);"));
        assert!(source.contains("use orbis_ui::diagnostics_export::{report_json, summary_text};"));
        assert!(!source.contains(&["wl", "-copy"].concat()));
        assert!(!source.contains(&["x", "clip"].concat()));
    }

    #[test]
    fn export_failure_stays_fail_closed_until_refresh() {
        let source = include_str!("diagnostics_backend.rs");
        assert!(source.contains("Report export failed · refresh to retry"));
        assert!(source.contains("export_path_available"));
    }
}
