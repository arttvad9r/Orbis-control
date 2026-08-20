#!/usr/bin/env python3
"""Static fail-closed contract checks for Automation shadow lifecycle.

The checker intentionally uses only the Python standard library so it remains
useful in minimal review containers where Rust/Cargo are unavailable. It is not
a substitute for `cargo check`, tests or clippy. Its job is narrower: make
accidental hardware execution, optimistic runtime readiness, or removal of the
lifecycle/freshness/revalidation barriers fail visibly before executable
validation is available.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REQUIRED_MARKERS: dict[str, tuple[str, ...]] = {
    "crates/orbis-core/src/lifecycle.rs": (
        "ResumeTelemetryGate",
        "observe_prepare_for_sleep",
        "IgnoredUnpairedResume",
        "IgnoredPreResumeTelemetry",
        "IgnoredStaleTelemetry",
        "max_resume_wait",
    ),
    "crates/orbis-core/src/automation.rs": (
        "PowerSourceEdgeDetector",
        "BaselineEstablished",
        "IgnoredStale",
        "required_confirmations",
    ),
    "crates/orbis-config/src/automation_store.rs": (
        "load_automation_policy",
        "PreservedSource",
    ),
    "crates/orbis-config/src/automation_preflight.rs": (
        "AutomationRuntimeWriteUnavailable",
        "preflight_automation_plan",
        "actions_if_ready",
        "CustomCommandNotAllowed",
    ),
    "crates/orbis-ui/src/automation_shadow_runtime.rs": (
        "observe_resume_telemetry",
        "ResumePowerSourceUnknown",
        "ResumeTelemetryStale",
        "CapabilitySnapshotStale",
        "ReadyButExecutionDisabled",
        "preflight_automation_plan",
    ),
    "crates/orbis-ui/src/automation_execution_guard.rs": (
        "AutomationExecutionCandidate",
        "revalidate_automation_candidate",
        "CapabilityGenerationChanged",
        "PolicyChanged",
        "PreflightBlocked",
        "generation_still_matches",
    ),
    "crates/orbis-ui/src/automation_backend.rs": (
        "ResumeTelemetryGate",
        "observe_prepare_for_sleep",
        "persisted_policy",
        "set_runtime_ready(false)",
        "ReadyButExecutionDisabled",
    ),
    "crates/orbis-ui/src/resume_observer.rs": (
        "org.freedesktop.login1.Manager",
        "PrepareForSleep",
        "receive_signal",
        "upgrade_in_event_loop",
        "observe_automation_from_sysfs",
    ),
    "crates/orbis-ui/src/quick_controls_backend.rs": (
        "resume_observer::spawn",
        "SysfsTelemetryProvider",
        "AUTOMATION_READ_TIMEOUT",
        "automation_observing",
    ),
    "crates/orbis-ui/src/secondary_windows_backend.rs": (
        "observe_prepare_for_sleep",
        "observe_automation_telemetry",
        "replace_automation_capabilities",
    ),
}

HARDWARE_INERT_FILES = (
    "crates/orbis-core/src/lifecycle.rs",
    "crates/orbis-ui/src/automation_shadow_runtime.rs",
    "crates/orbis-ui/src/automation_execution_guard.rs",
    "crates/orbis-ui/src/automation_backend.rs",
    "crates/orbis-ui/src/resume_observer.rs",
    "crates/orbis-ui/src/secondary_windows_backend.rs",
)

FORBIDDEN_CODE = (
    "WorkerCommand::Set",
    "set_fan_curve(",
    "set_gpu_mode(",
    "set_charge_limit(",
    "set_performance(",
    "std::process::Command",
    "Command::new(",
    "unsafe {",
    "unsafe fn",
)


def strip_rust_non_code(source: str) -> str:
    """Replace Rust comments/string/char literal contents with spaces.

    This is deliberately a small lexer, not a parser. It preserves newlines and
    punctuation outside literals so forbidden executable tokens can be searched
    without self-scan tests or comments triggering false positives. Raw strings
    with arbitrary `#` counts are handled because source-level safety tests often
    contain code-looking text in literals.
    """

    out = list(source)
    n = len(source)
    i = 0
    block_depth = 0

    def blank(start: int, end: int) -> None:
        for pos in range(start, end):
            if out[pos] != "\n":
                out[pos] = " "

    while i < n:
        if block_depth:
            if source.startswith("/*", i):
                blank(i, i + 2)
                block_depth += 1
                i += 2
            elif source.startswith("*/", i):
                blank(i, i + 2)
                block_depth -= 1
                i += 2
            else:
                blank(i, i + 1)
                i += 1
            continue

        if source.startswith("//", i):
            end = source.find("\n", i)
            if end < 0:
                end = n
            blank(i, end)
            i = end
            continue
        if source.startswith("/*", i):
            blank(i, i + 2)
            block_depth = 1
            i += 2
            continue

        # Rust raw string: r"...", r#"..."#, br##"..."##, etc.
        raw_start = i
        if source.startswith("br", i):
            j = i + 2
        elif source.startswith("r", i):
            j = i + 1
        else:
            j = -1
        if j >= 0:
            hashes = 0
            while j < n and source[j] == "#":
                hashes += 1
                j += 1
            if j < n and source[j] == '"':
                terminator = '"' + ("#" * hashes)
                end = source.find(terminator, j + 1)
                end = n if end < 0 else end + len(terminator)
                blank(raw_start, end)
                i = end
                continue

        # Normal byte/string literal.
        quote_start = i
        if source.startswith('b"', i):
            i += 1
        if i < n and source[i] == '"':
            j = i + 1
            escaped = False
            while j < n:
                ch = source[j]
                if ch == '"' and not escaped:
                    j += 1
                    break
                if ch == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                j += 1
            blank(quote_start, j)
            i = j
            continue

        # Conservative char/byte-char literal handling. Lifetimes such as 'a
        # are left intact because they have no closing quote immediately after
        # one escaped-or-unescaped character sequence.
        char_start = i
        byte_char = source.startswith("b'", i)
        quote_pos = i + 1 if byte_char else i
        if quote_pos < n and source[quote_pos] == "'":
            j = quote_pos + 1
            escaped = False
            while j < n and source[j] != "\n":
                ch = source[j]
                if ch == "'" and not escaped:
                    j += 1
                    blank(char_start, j)
                    i = j
                    break
                if ch == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                j += 1
            else:
                i = char_start + 1
            continue

        i += 1

    return "".join(out)


def read_required(root: Path, relative: str, errors: list[str]) -> str | None:
    path = root / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read required file: {error}")
        return None


def check_required_markers(root: Path, errors: list[str]) -> None:
    for relative, markers in REQUIRED_MARKERS.items():
        source = read_required(root, relative, errors)
        if source is None:
            continue
        for marker in markers:
            if marker not in source:
                errors.append(f"{relative}: missing required Automation marker {marker!r}")


def check_hardware_inert_sources(root: Path, errors: list[str]) -> None:
    for relative in HARDWARE_INERT_FILES:
        source = read_required(root, relative, errors)
        if source is None:
            continue
        code = strip_rust_non_code(source)
        for token in FORBIDDEN_CODE:
            if token in code:
                errors.append(f"{relative}: forbidden execution token in code: {token!r}")


def check_runtime_readiness(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_backend.rs"
    source = read_required(root, relative, errors)
    if source is None:
        return
    code = strip_rust_non_code(source)
    if "set_runtime_ready(true)" in code:
        errors.append(f"{relative}: shadow backend must never set runtime-ready true")
    if "set_runtime_ready(false)" not in code:
        errors.append(f"{relative}: missing explicit fail-closed runtime-ready=false publication")


def check_execution_guard_api(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_execution_guard.rs"
    source = read_required(root, relative, errors)
    if source is None:
        return
    code = strip_rust_non_code(source)
    risky_public = re.compile(
        r"\bpub\s+(?:async\s+)?fn\s+(execute|apply|dispatch|commit|mutate|write)\b"
    )
    match = risky_public.search(code)
    if match:
        errors.append(
            f"{relative}: execution handoff exposes forbidden public method {match.group(1)!r}"
        )


def check_resume_observer(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/resume_observer.rs"
    source = read_required(root, relative, errors)
    if source is None:
        return
    code = strip_rust_non_code(source)
    if "Connection::system()" not in code:
        errors.append(f"{relative}: resume observer must use the system bus")
    if "receive_signal(" not in code:
        errors.append(f"{relative}: missing signal subscription")
    for suspicious in (".call(", ".call_method(", "request_name(", "ObjectServer"):
        if suspicious in code:
            errors.append(
                f"{relative}: resume observer contains unexpected D-Bus active operation {suspicious!r}"
            )


def check_manifests(root: Path, errors: list[str]) -> None:
    workspace = read_required(root, "Cargo.toml", errors)
    ui = read_required(root, "crates/orbis-ui/Cargo.toml", errors)
    if workspace is not None and 'futures-util = "0.3"' not in workspace:
        errors.append("Cargo.toml: missing pinned workspace futures-util dependency")
    if ui is not None and "futures-util = { workspace = true }" not in ui:
        errors.append("crates/orbis-ui/Cargo.toml: missing workspace futures-util dependency")


def run(root: Path) -> list[str]:
    errors: list[str] = []
    check_required_markers(root, errors)
    check_hardware_inert_sources(root, errors)
    check_runtime_readiness(root, errors)
    check_execution_guard_api(root, errors)
    check_resume_observer(root, errors)
    check_manifests(root, errors)
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        default=".",
        help="repository root (default: current directory)",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    root = Path(args.root).resolve()
    errors = run(root)
    if errors:
        for error in errors:
            print(f"automation-shadow-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("automation-shadow-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
