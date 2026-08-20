# Orbis Control privacy audit — 2026-08-19

> **HISTORICAL SNAPSHOT — key finding remediated.** The preferences logging leak
> described below was valid for the audited state, but current code implements
> fixed `PreferencesWarningKind::log_category()` values and a redacted `Debug`
> representation. Use [`current-state.md`](current-state.md) and
> [`history.md`](history.md) for current privacy status. The original finding is
> preserved below as the rationale for that invariant.

Scope: backlog item #76. Read-only review of diagnostics Draft sources plus current preferences/config logging. This document does not claim export functionality that does not yet exist.

## Result

Privacy posture is mostly narrow-by-construction, but the audit is **not fully green**: one real logging leak exists in the current preferences UI stack and must be remediated before treating privacy as complete.

## Diagnostics data sources

### Hardware identity

The diagnostics hardware identity provider uses an explicit DMI allowlist only:

- vendor;
- product name;
- board name;
- BIOS version;
- BIOS date.

Serial numbers, UUIDs, asset tags and other unique identifiers are deliberately excluded, with regression coverage asserting that serial paths are never queried.

### System/session metadata

The diagnostics system metadata provider reads only:

- `/proc/sys/kernel/osrelease`;
- compile/runtime architecture;
- `XDG_SESSION_TYPE`;
- `WAYLAND_DISPLAY`;
- `DISPLAY`.

The environment access is allowlisted. It does not enumerate the full process environment and deliberately does not use `XDG_CURRENT_DESKTOP` as compositor identity.

### Other diagnostics sources

The current diagnostics Draft chain is typed snapshot data: capabilities, service presence, GPU primitives, telemetry, display outputs and ASUS read status. No diagnostics branch currently implements journal capture, arbitrary file export, command execution or generic filesystem attachment collection.

## Export status

Backlog items #15–#17 (safe text export, safe JSON export, Copy Summary) are not implemented in the inspected branch set. Therefore there is currently no production diagnostics export path to classify as safe or unsafe.

When they are implemented, the export contract must remain an explicit field allowlist over the typed snapshot/DTO. It must not add:

- full environment dumps;
- raw journal/system logs;
- arbitrary file paths or file contents;
- serial/UUID/asset-tag identifiers;
- shell-command output;
- configuration file contents;
- secrets or authentication material.

## Confirmed logging issue

The preferences storage model preserves parser/schema diagnostics in typed warning payloads such as `MalformedToml(String)`, `InvalidSchema(String)` and `LegacyImport(String)`.

The current UI startup stack logs `PreferencesWarningKind` with debug formatting (`kind = ?warning.kind`). Theme persistence failure formatting also includes the warning with debug formatting. A parser/serde error string may contain a source excerpt from the malformed user TOML. Consequently arbitrary fragments of a corrupted preferences/config file can be copied into the application log/journal.

This violates the privacy requirement even though the application never intentionally exports the configuration file.

### Required remediation

Production logs must record only a fixed warning category, for example:

- `malformed_toml`;
- `missing_schema_version`;
- `invalid_schema_version`;
- `unsupported_older_version`;
- `invalid_schema`;
- `legacy_import_failed`.

Do not log the raw parser/schema diagnostic payload. A regression test should use a synthetic secret marker embedded in malformed TOML and assert that the formatted/loggable warning representation does not contain that marker.

The underlying typed diagnostic may remain available to controlled in-process code if needed, but production logging/export must pass through a redacted representation.

## Paths and filesystem errors

Filesystem errors currently include operation and path context. This audit does not classify ordinary local XDG path logging as an arbitrary-file-content leak, but diagnostics exports must not automatically copy such paths into support bundles without an explicit privacy decision.

## Evidence classification

- Hardware identity allowlist: source/test evidence.
- Environment allowlist: source/test evidence.
- Absence of diagnostics journal/arbitrary-file export: source/branch inspection only.
- Preferences log leak: confirmed source-level finding.

No live services, journal, system/session bus or hardware were accessed during this audit.
