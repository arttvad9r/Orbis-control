//! Manual "is a newer release published?" check.
//!
//! Notification only: nothing is downloaded or installed (packages come from
//! pacman). The network is touched only when the user asks, through `curl` with
//! fixed arguments and no shell.

use std::cmp::Ordering;
use std::process::{Command, Stdio};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/arttvad9r/Orbis-control/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate { current: String },
    Available { current: String, latest: String },
    Failed(String),
}

impl UpdateStatus {
    pub fn text(&self) -> String {
        match self {
            Self::UpToDate { current } => format!("Установлена последняя версия ({current})."),
            Self::Available { current, latest } => format!(
                "Доступна версия {latest} (установлена {current}). Обновите пакет через pacman."
            ),
            Self::Failed(message) => format!("Не удалось проверить обновления: {message}"),
        }
    }

    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

fn parse_version(text: &str) -> Option<[u64; 3]> {
    let core = text.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut out = [0_u64; 3];
    for slot in &mut out {
        *slot = match parts.next() {
            Some(part) => part.parse().ok()?,
            None => 0,
        };
    }
    parts.next().is_none().then_some(out)
}

pub fn compare_release(current: &str, latest_tag: &str) -> UpdateStatus {
    let (Some(now), Some(latest)) = (parse_version(current), parse_version(latest_tag)) else {
        return UpdateStatus::Failed(format!("непонятный номер версии «{latest_tag}»"));
    };
    match latest.cmp(&now) {
        Ordering::Greater => UpdateStatus::Available {
            current: current.to_owned(),
            latest: latest_tag.trim_start_matches('v').to_owned(),
        },
        Ordering::Equal | Ordering::Less => UpdateStatus::UpToDate {
            current: current.to_owned(),
        },
    }
}

fn tag_from_release_json(body: &[u8]) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "неожиданный ответ сервера".to_owned())?;
    value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "в ответе нет номера релиза".to_owned())
}

fn fetch_latest_tag() -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--proto",
            "=https",
            "--max-time",
            "10",
            "--max-filesize",
            "1048576",
            "--header",
            "Accept: application/vnd.github+json",
            "--user-agent",
            concat!("orbis-control/", env!("CARGO_PKG_VERSION")),
            LATEST_RELEASE_URL,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("curl недоступен ({error})"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.lines().next().unwrap_or("нет связи").trim();
        return Err(detail.to_owned());
    }
    tag_from_release_json(&output.stdout)
}

/// Blocking: run from `spawn_blocking`.
pub fn check_for_update() -> UpdateStatus {
    match fetch_latest_tag() {
        Ok(tag) => compare_release(env!("CARGO_PKG_VERSION"), &tag),
        Err(message) => UpdateStatus::Failed(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_tag_is_available_and_equal_or_older_is_not() {
        assert_eq!(
            compare_release("0.1.0", "v0.2.0"),
            UpdateStatus::Available {
                current: "0.1.0".into(),
                latest: "0.2.0".into()
            }
        );
        assert!(matches!(
            compare_release("0.1.0", "v0.1.0"),
            UpdateStatus::UpToDate { .. }
        ));
        assert!(matches!(
            compare_release("0.2.0", "v0.1.9"),
            UpdateStatus::UpToDate { .. }
        ));
        assert!(matches!(
            compare_release("0.9.0", "v0.10.0"),
            UpdateStatus::Available { .. }
        ));
    }

    #[test]
    fn garbage_tags_fail_instead_of_guessing() {
        assert!(compare_release("0.1.0", "nightly").is_problem());
        assert!(compare_release("0.1.0", "v1.2.3.4").is_problem());
        assert!(compare_release("0.1.0", "").is_problem());
    }

    #[test]
    fn release_json_needs_a_tag_name() {
        assert_eq!(
            tag_from_release_json(br#"{"tag_name":"v0.2.0","name":"x"}"#).unwrap(),
            "v0.2.0"
        );
        assert!(tag_from_release_json(br#"{"name":"x"}"#).is_err());
        assert!(tag_from_release_json(b"<html>").is_err());
    }
}
