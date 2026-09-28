//! Refresh rate of the internal panel, switched through the desktop compositor.
//!
//! This is a user-session concern, not ASUS firmware: the compositor owns the
//! output configuration. The production backend drives KDE's `kscreen-doctor`
//! (JSON read, `output.<name>.mode.<id>` write) with fixed arguments and never
//! passes caller text to a shell. A write is only reported as done after a
//! fresh read shows the requested mode as current.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::ProviderError;

const COMMAND_DEADLINE: Duration = Duration::from_secs(5);
/// `KScreen::Output::Panel`: the built-in display connector.
const KSCREEN_OUTPUT_TYPE_PANEL: u32 = 7;

/// Observed refresh state of the internal panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRefreshState {
    /// Compositor output name (diagnostics only).
    pub output: String,
    /// Current rate rounded to whole hertz.
    pub current_hz: u32,
    /// Distinct whole-hertz rates offered at the current resolution, ascending.
    pub rates_hz: Vec<u32>,
}

/// Read and switch the internal panel refresh rate.
#[async_trait]
pub trait PanelRefreshBackend: Send + Sync {
    /// Fresh read of the panel state.
    async fn read(&self) -> Result<PanelRefreshState, ProviderError>;
    /// Switch to `hz` at the current resolution; `Ok` only after read-back
    /// confirms the new mode.
    async fn set_rate(&self, hz: u32) -> Result<PanelRefreshState, ProviderError>;
}

#[derive(Debug, Deserialize)]
struct KscreenDocument {
    outputs: Vec<KscreenOutput>,
}

#[derive(Debug, Deserialize)]
struct KscreenOutput {
    name: String,
    #[serde(default)]
    connected: bool,
    #[serde(default)]
    enabled: bool,
    #[serde(rename = "type", default)]
    kind: u32,
    #[serde(rename = "currentModeId", default)]
    current_mode_id: String,
    #[serde(default)]
    modes: Vec<KscreenMode>,
}

#[derive(Debug, Deserialize)]
struct KscreenMode {
    id: String,
    size: KscreenSize,
    #[serde(rename = "refreshRate")]
    refresh_rate: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
struct KscreenSize {
    width: u32,
    height: u32,
}

struct Panel {
    output: String,
    current_id: String,
    current_size: KscreenSize,
    current_hz: u32,
    modes: Vec<(String, KscreenSize, u32)>,
}

impl Panel {
    fn rates_hz(&self) -> Vec<u32> {
        let mut rates: Vec<u32> = self
            .modes
            .iter()
            .filter(|(_, size, _)| *size == self.current_size)
            .map(|(_, _, hz)| *hz)
            .collect();
        rates.sort_unstable();
        rates.dedup();
        rates
    }

    fn state(&self) -> PanelRefreshState {
        PanelRefreshState {
            output: self.output.clone(),
            current_hz: self.current_hz,
            rates_hz: self.rates_hz(),
        }
    }

    fn mode_for(&self, hz: u32) -> Option<&str> {
        self.modes
            .iter()
            .find(|(_, size, rate)| *size == self.current_size && *rate == hz)
            .map(|(id, _, _)| id.as_str())
    }
}

fn round_hz(rate: f64) -> u32 {
    rate.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

fn parse_panel(json: &[u8]) -> Result<Panel, ProviderError> {
    let document: KscreenDocument = serde_json::from_slice(json)
        .map_err(|error| ProviderError::Internal(format!("kscreen-doctor JSON: {error}")))?;
    let output = document
        .outputs
        .into_iter()
        .find(|output| {
            output.connected && output.enabled && output.kind == KSCREEN_OUTPUT_TYPE_PANEL
        })
        .ok_or_else(|| ProviderError::Unsupported("встроенная панель не найдена".into()))?;
    let current = output
        .modes
        .iter()
        .find(|mode| mode.id == output.current_mode_id)
        .ok_or_else(|| ProviderError::Internal("текущий режим панели не найден".into()))?;
    Ok(Panel {
        current_size: current.size,
        current_hz: round_hz(current.refresh_rate),
        current_id: output.current_mode_id.clone(),
        modes: output
            .modes
            .iter()
            .map(|mode| (mode.id.clone(), mode.size, round_hz(mode.refresh_rate)))
            .collect(),
        output: output.name,
    })
}

fn is_safe_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `kscreen-doctor` backed panel refresh control.
#[derive(Debug, Clone)]
pub struct KscreenDoctorPanel {
    program: PathBuf,
}

impl Default for KscreenDoctorPanel {
    fn default() -> Self {
        Self::new("kscreen-doctor")
    }
}

impl KscreenDoctorPanel {
    /// Use `program` (a name resolved through `PATH`, or an absolute path).
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    async fn run(&self, args: &[&str]) -> Result<Vec<u8>, ProviderError> {
        let child = tokio::process::Command::new(&self.program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = tokio::time::timeout(COMMAND_DEADLINE, child)
            .await
            .map_err(|_| ProviderError::Timeout("kscreen-doctor не ответил вовремя".into()))?
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => {
                    ProviderError::Unsupported("kscreen-doctor не установлен".into())
                }
                _ => ProviderError::Io(error),
            })?;
        if !output.status.success() {
            return Err(ProviderError::BackendUnavailable(format!(
                "kscreen-doctor завершился с {}",
                output.status
            )));
        }
        Ok(output.stdout)
    }

    async fn read_panel(&self) -> Result<Panel, ProviderError> {
        parse_panel(&self.run(&["-j"]).await?)
    }
}

#[async_trait]
impl PanelRefreshBackend for KscreenDoctorPanel {
    async fn read(&self) -> Result<PanelRefreshState, ProviderError> {
        Ok(self.read_panel().await?.state())
    }

    async fn set_rate(&self, hz: u32) -> Result<PanelRefreshState, ProviderError> {
        let panel = self.read_panel().await?;
        if panel.current_hz == hz {
            return Ok(panel.state());
        }
        let mode_id = panel.mode_for(hz).ok_or_else(|| {
            ProviderError::InvalidRequest(format!(
                "{hz} Гц недоступна при текущем разрешении панели"
            ))
        })?;
        if !is_safe_token(&panel.output) || !is_safe_token(mode_id) {
            return Err(ProviderError::InvalidRequest(
                "недопустимое имя выхода или режима".into(),
            ));
        }
        let target = mode_id.to_string();
        let setting = format!("output.{}.mode.{target}", panel.output);
        self.run(&[&setting]).await?;
        let after = self.read_panel().await?;
        if after.current_id != target {
            return Err(ProviderError::Conflict(format!(
                "компоновщик не подтвердил {hz} Гц (сейчас {} Гц)",
                after.current_hz
            )));
        }
        Ok(after.state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"outputs":[
      {"name":"HDMI-A-1","connected":true,"enabled":true,"type":6,"currentModeId":"a",
       "modes":[{"id":"a","size":{"width":1920,"height":1080},"refreshRate":75.0}]},
      {"name":"eDP-1","connected":true,"enabled":true,"type":7,"currentModeId":"1",
       "modes":[
        {"id":"1","size":{"width":1920,"height":1080},"refreshRate":144.028},
        {"id":"2","size":{"width":1920,"height":1080},"refreshRate":60.01},
        {"id":"3","size":{"width":1680,"height":1050},"refreshRate":144.028},
        {"id":"4","size":{"width":1280,"height":1024},"refreshRate":59.895}]}]}"#;

    #[test]
    fn picks_the_internal_panel_and_current_resolution_rates() {
        let panel = parse_panel(DOC.as_bytes()).unwrap();
        assert_eq!(
            panel.state(),
            PanelRefreshState {
                output: "eDP-1".into(),
                current_hz: 144,
                rates_hz: vec![60, 144],
            }
        );
        assert_eq!(panel.mode_for(60), Some("2"));
        assert_eq!(panel.mode_for(75), None);
    }

    #[test]
    fn missing_panel_is_unsupported() {
        let doc = r#"{"outputs":[{"name":"HDMI-A-1","connected":true,"enabled":true,"type":6,
            "currentModeId":"a","modes":[{"id":"a","size":{"width":1,"height":1},"refreshRate":60.0}]}]}"#;
        assert!(matches!(
            parse_panel(doc.as_bytes()),
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[test]
    fn tokens_reject_shell_and_separator_text() {
        assert!(is_safe_token("eDP-1"));
        assert!(!is_safe_token("eDP 1"));
        assert!(!is_safe_token("a;b"));
        assert!(!is_safe_token(""));
    }

    #[cfg(unix)]
    fn fake_kscreen(dir: &std::path::Path, honour_writes: bool) -> KscreenDoctorPanel {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("doc.json"), DOC).unwrap();
        std::fs::write(dir.join("current"), "1").unwrap();
        let write = if honour_writes {
            r#"echo "${1##*.}" > "$D/current""#
        } else {
            ":"
        };
        let script = format!(
            r#"#!/bin/sh
D={dir}
if [ "$1" = "-j" ]; then
  sed "s/\"currentModeId\":\"1\"/\"currentModeId\":\"$(cat "$D/current")\"/" "$D/doc.json"
else
  {write}
fi
"#,
            dir = dir.display()
        );
        let path = dir.join("kscreen-doctor");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        KscreenDoctorPanel::new(path)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn set_rate_switches_and_confirms_by_readback() {
        let dir = tempfile::tempdir().unwrap();
        let panel = fake_kscreen(dir.path(), true);
        let state = panel.set_rate(60).await.unwrap();
        assert_eq!(state.current_hz, 60);
        assert_eq!(panel.read().await.unwrap().current_hz, 60);
        assert_eq!(panel.set_rate(60).await.unwrap().current_hz, 60);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unconfirmed_switch_is_a_conflict_not_success() {
        let dir = tempfile::tempdir().unwrap();
        let panel = fake_kscreen(dir.path(), false);
        assert!(matches!(
            panel.set_rate(60).await,
            Err(ProviderError::Conflict(_))
        ));
        assert!(matches!(
            panel.set_rate(75).await,
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn missing_binary_is_unsupported() {
        let panel = KscreenDoctorPanel::new("/nonexistent/kscreen-doctor");
        assert!(matches!(
            panel.read().await,
            Err(ProviderError::Unsupported(_))
        ));
    }
}
