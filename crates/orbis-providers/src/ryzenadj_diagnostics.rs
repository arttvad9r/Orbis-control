//! Read-only evidence parser for the fixed RyzenAdj `--info` contract.
//!
//! This module intentionally does not execute RyzenAdj and exposes no setter.
//! The reference tool's `--info` table can provide current values, but it does
//! not establish authoritative min/max/step metadata or a read-back contract.
//! Those gaps keep the Orbis mutation capability blocked.

use std::collections::BTreeMap;

/// Explicit executable path used by the audited host.
pub const RYZENADJ_EXECUTABLE: &str = "/usr/bin/ryzenadj";
/// The only non-mutating argument in this evidence probe.
pub const RYZENADJ_INFO_ARGS: &[&str] = &["--info"];

/// A current value parsed from one known RyzenAdj power/thermal table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RyzenAdjCurrentValue {
    /// RyzenAdj CLI parameter name, for example `stapm-limit`.
    pub parameter: String,
    /// Raw value reported by RyzenAdj (`mW`, `mA`, degrees C, seconds, etc.).
    pub value: u32,
}

/// Structured, read-only result of parsing one `ryzenadj --info` attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RyzenAdjInfoEvidence {
    /// Reported RyzenAdj version, when present.
    pub version: Option<String>,
    /// Detected SMU kernel module, when present.
    pub kernel_module: Option<String>,
    /// Current values from the table; empty when initialization fails.
    pub current_values: BTreeMap<String, RyzenAdjCurrentValue>,
    /// Initialization or command failure reported by the tool.
    pub failure: Option<String>,
    /// Exit status of the fixed `--info` invocation.
    pub exit_code: Option<i32>,
}

impl RyzenAdjInfoEvidence {
    /// `--info` does not expose Orbis-authoritative min/max/step metadata.
    pub const fn has_authoritative_metadata(&self) -> bool {
        false
    }

    /// Whether the evidence is sufficient to expose an Orbis write backend.
    pub const fn supports_typed_mutation(&self) -> bool {
        false
    }
}

/// Parse the bounded output of the fixed `ryzenadj --info` invocation.
///
/// Only known table rows with a lowercase CLI parameter token and an integer
/// value are accepted. Free-form output is retained only as a short failure
/// detail; no caller-controlled command, path, or setter is represented.
pub fn parse_info_output(
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> RyzenAdjInfoEvidence {
    let mut evidence = RyzenAdjInfoEvidence {
        version: None,
        kernel_module: None,
        current_values: BTreeMap::new(),
        failure: None,
        exit_code,
    };

    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Version:") {
            evidence.version = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("detected compatible ") {
            evidence.kernel_module = value.strip_suffix(" kernel module").map(str::to_string);
        } else if let Some(value) = line.strip_prefix("Unable to ") {
            evidence.failure = Some(format!("Unable to {value}"));
        }

        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        let cells: Vec<_> = rest.split('|').map(str::trim).collect();
        if cells.len() < 3 {
            continue;
        }
        let parameter = cells[2];
        if parameter.is_empty()
            || !parameter
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-')
        {
            continue;
        }
        let Ok(value) = cells[1].parse::<u32>() else {
            continue;
        };
        evidence.current_values.insert(
            parameter.to_string(),
            RyzenAdjCurrentValue {
                parameter: parameter.to_string(),
                value,
            },
        );
    }

    if evidence.failure.is_none() && evidence.exit_code != Some(0) {
        evidence.failure = Some("ryzenadj --info exited unsuccessfully".into());
    }
    evidence
}

/// Diagnostic entries that make the blocked mutation decision explicit.
pub fn diagnostics() -> Vec<orbis_core::diagnostics::DiagnosticEntry> {
    vec![
        orbis_core::diagnostics::DiagnosticEntry::new(
            "backend.ryzenadj.contract",
            format!(
                "read-only candidate: executable={RYZENADJ_EXECUTABLE}; args=ryzenadj --info; no Orbis typed writer"
            ),
        ),
        orbis_core::diagnostics::DiagnosticEntry::new(
            "backend.ryzenadj.write-status",
            "blocked: --info has no authoritative min/max/step metadata and no proven read-back; no sudo/generic command path",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_info_preserves_version_module_and_failure_without_fake_values() {
        let evidence = parse_info_output(
            "detected compatible ryzen_smu kernel module\nVersion: v0.19.0\n",
            "Unable to get os_access Obj, check permission\nUnable to init ryzenadj\n",
            Some(1),
        );
        assert_eq!(evidence.version.as_deref(), Some("v0.19.0"));
        assert_eq!(evidence.kernel_module.as_deref(), Some("ryzen_smu"));
        assert!(evidence.current_values.is_empty());
        assert_eq!(evidence.failure.as_deref(), Some("Unable to init ryzenadj"));
        assert!(!evidence.has_authoritative_metadata());
        assert!(!evidence.supports_typed_mutation());
    }

    #[test]
    fn successful_table_is_structured_but_not_promoted_to_write_support() {
        let evidence = parse_info_output(
            "Version: v0.19.0\n| STAPM LIMIT | 25000 | stapm-limit |\n| PPT LIMIT FAST | 30000 | fast-limit |\n",
            "",
            Some(0),
        );
        assert_eq!(evidence.current_values["stapm-limit"].value, 25_000);
        assert_eq!(evidence.current_values["fast-limit"].value, 30_000);
        assert!(!evidence.has_authoritative_metadata());
        assert!(!evidence.supports_typed_mutation());
    }

    #[test]
    fn malformed_and_header_rows_are_ignored() {
        let evidence = parse_info_output(
            "| NAME | VALUE | PARAM |\n| --- | --- | --- |\n| row | nan | stapm-limit |\n| row | 12 | UpperCase |\n",
            "",
            Some(0),
        );
        assert!(evidence.current_values.is_empty());
    }

    #[test]
    fn contract_diagnostics_are_specific_and_fail_closed() {
        let entries = diagnostics();
        assert!(
            entries
                .iter()
                .any(|entry| entry.key == "backend.ryzenadj.contract"
                    && entry.value.contains("/usr/bin/ryzenadj"))
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.key == "backend.ryzenadj.write-status"
                    && entry.value.contains("blocked"))
        );
    }
}
