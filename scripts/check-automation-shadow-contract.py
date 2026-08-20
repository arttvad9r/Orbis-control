#!/usr/bin/env python3
"""Static fail-closed checks for Automation lifecycle/execution design.

Stdlib-only by design: useful when Rust/Slint tooling is unavailable. This is a
structural safety gate, not a substitute for cargo check/test/clippy.
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
        "required_confirmations",
        "rebaseline",
        "break_candidate_continuity",
        "reset_candidate",
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
        "break_power_source_candidate",
        "break_candidate_continuity",
        "observe_resume_telemetry",
        "ReadyButExecutionDisabled",
        "policy.enabled && policy.on_resume",
        "rebaseline(ac_online)",
    ),
    "crates/orbis-ui/src/automation_execution_guard.rs": (
        "AutomationExecutionCandidate",
        "revalidate_automation_candidate",
        "CapabilityGenerationChanged",
        "PolicyChanged",
        "PreflightBlocked",
    ),
    "crates/orbis-ui/src/automation_lifecycle_revision.rs": (
        "AutomationLifecycleClock",
        "AutomationLifecycleRevision",
        "AutomationRevisionCandidate",
        "LifecycleRevisionChanged",
        "AutomationRevisionHandoff",
        "revalidate_revision_candidate",
        "SequenceExhausted",
    ),
    "crates/orbis-ui/src/automation_serialization.rs": (
        "AutomationSerializationCoordinator",
        "AutomationDryRunLease",
        "LifecycleRevisionAlreadyAdmitted",
        "last_admitted_revision",
        "required_revision <= self.last_admitted_revision",
        "CapabilityGenerationChanged",
        "Busy",
    ),
    "crates/orbis-ui/src/automation_execution_scope.rs": (
        "AutomationPreparedBatch",
        "AutomationPreparedKind::Performance",
        "UnsupportedAction",
        "DuplicatePerformanceAction",
        "required_revision",
        "required_generation",
    ),
    "crates/orbis-ui/src/automation_worker_runtime.rs": (
        "AutomationWorkerRuntime",
        "break_power_source_candidate",
        "if start",
        "policy.enabled && policy.on_resume",
        "prepare_latest",
        "self.latest_candidate = None",
        "AutomationSerializationCoordinator",
    ),
    "crates/orbis-ui/src/automation_recovery.rs": (
        "AutomationRecoveryBarrier",
        "mark_performance_unknown",
        "reconcile_performance",
        "RecoveredAtDifferent",
    ),
    "crates/orbis-ui/src/automation_worker_coordinator.rs": (
        "AutomationWorkerCoordinator",
        "RecoveryRequired",
        "finish_performance_unknown",
        "reconcile_performance",
    ),
    "crates/orbis-ui/src/automation_capability.rs": (
        "CapabilityStatus::ReadOnly",
        "CapabilityStatus::Unsupported",
        "automation_shadow_capability",
    ),
    "crates/orbis-ui/src/automation_backend.rs": (
        "persisted_policy",
        "set_runtime_ready(false)",
        "ReadyButExecutionDisabled",
    ),
    "crates/orbis-ui/src/resume_observer.rs": (
        "org.freedesktop.login1.Manager",
        "PrepareForSleep",
        "receive_signal",
        "ordered_stream::OrderedStreamExt",
        "upgrade_in_event_loop",
    ),
}

HARDWARE_INERT_FILES = (
    "crates/orbis-core/src/lifecycle.rs",
    "crates/orbis-ui/src/automation_shadow_runtime.rs",
    "crates/orbis-ui/src/automation_execution_guard.rs",
    "crates/orbis-ui/src/automation_lifecycle_revision.rs",
    "crates/orbis-ui/src/automation_serialization.rs",
    "crates/orbis-ui/src/automation_execution_scope.rs",
    "crates/orbis-ui/src/automation_worker_runtime.rs",
    "crates/orbis-ui/src/automation_recovery.rs",
    "crates/orbis-ui/src/automation_worker_coordinator.rs",
    "crates/orbis-ui/src/automation_backend.rs",
    "crates/orbis-ui/src/resume_observer.rs",
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
    """Blank comments/string/char literals while preserving code punctuation."""
    out = list(source)
    n = len(source)
    i = 0
    block_depth = 0

    def blank(start: int, end: int) -> None:
        for pos in range(start, min(end, n)):
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
            end = n if end < 0 else end
            blank(i, end)
            i = end
            continue
        if source.startswith("/*", i):
            blank(i, i + 2)
            block_depth = 1
            i += 2
            continue

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
                terminator = '"' + "#" * hashes
                end = source.find(terminator, j + 1)
                end = n if end < 0 else end + len(terminator)
                blank(raw_start, end)
                i = end
                continue

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
                escaped = ch == "\\" and not escaped
                if ch != "\\":
                    escaped = False
                j += 1
            blank(quote_start, j)
            i = j
            continue

        char_start = i
        byte_char = source.startswith("b'", i)
        quote_pos = i + 1 if byte_char else i
        if quote_pos < n and source[quote_pos] == "'":
            j = quote_pos + 1
            escaped = False
            closed = False
            while j < n and source[j] != "\n":
                ch = source[j]
                if ch == "'" and not escaped:
                    j += 1
                    closed = True
                    break
                escaped = ch == "\\" and not escaped
                if ch != "\\":
                    escaped = False
                j += 1
            if closed:
                blank(char_start, j)
                i = j
                continue

        i += 1

    return "".join(out)


def read_required(root: Path, relative: str, errors: list[str]) -> str | None:
    try:
        return (root / relative).read_text(encoding="utf-8")
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


def derive_includes_clone(source: str, type_name: str) -> bool:
    pattern = re.compile(
        rf"#\s*\[\s*derive\s*\((?P<traits>[^\]]*)\)\s*\]\s*pub\s+(?:struct|enum)\s+{re.escape(type_name)}\b",
        re.MULTILINE,
    )
    match = pattern.search(source)
    return bool(match and re.search(r"\bClone\b", match.group("traits")))


def check_unique_owners_and_replay(root: Path, errors: list[str]) -> None:
    unique_types = (
        ("crates/orbis-ui/src/automation_lifecycle_revision.rs", "AutomationRevisionHandoff"),
        ("crates/orbis-ui/src/automation_serialization.rs", "AutomationSerializationCoordinator"),
        ("crates/orbis-ui/src/automation_worker_runtime.rs", "AutomationWorkerRuntime"),
        ("crates/orbis-ui/src/automation_recovery.rs", "AutomationRecoveryBarrier"),
        ("crates/orbis-ui/src/automation_worker_coordinator.rs", "AutomationWorkerCoordinator"),
    )
    for relative, type_name in unique_types:
        source = read_required(root, relative, errors)
        if source is not None and derive_includes_clone(source, type_name):
            errors.append(f"{relative}: unique owner {type_name} must not derive Clone")

    serialization = read_required(root, "crates/orbis-ui/src/automation_serialization.rs", errors)
    if serialization is not None:
        for marker in (
            "last_admitted_revision: AutomationLifecycleRevision",
            "required_revision <= self.last_admitted_revision",
            "self.last_admitted_revision = required_revision",
        ):
            if marker not in serialization:
                errors.append(f"automation_serialization.rs: missing replay barrier {marker!r}")

    worker = read_required(root, "crates/orbis-ui/src/automation_worker_runtime.rs", errors)
    if worker is not None:
        for marker in (
            "if start {",
            "self.shadow.break_power_source_candidate();",
            "let resume_enabled = policy.enabled && policy.on_resume;",
            "self.latest_candidate = None;",
        ):
            if marker not in worker:
                errors.append(f"automation_worker_runtime.rs: missing lifecycle guard {marker!r}")


def check_runtime_readiness(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_backend.rs"
    source = read_required(root, relative, errors)
    if source is None:
        return
    code = strip_rust_non_code(source)
    if "set_runtime_ready(true)" in code:
        errors.append(f"{relative}: backend must not publish runtime-ready true yet")
    if "set_runtime_ready(false)" not in code:
        errors.append(f"{relative}: missing explicit runtime-ready=false publication")


def check_public_execution_surfaces(root: Path, errors: list[str]) -> None:
    inert = (
        "crates/orbis-ui/src/automation_execution_guard.rs",
        "crates/orbis-ui/src/automation_lifecycle_revision.rs",
        "crates/orbis-ui/src/automation_serialization.rs",
        "crates/orbis-ui/src/automation_execution_scope.rs",
        "crates/orbis-ui/src/automation_worker_runtime.rs",
        "crates/orbis-ui/src/automation_recovery.rs",
        "crates/orbis-ui/src/automation_worker_coordinator.rs",
    )
    risky = re.compile(r"\bpub\s+(?:async\s+)?fn\s+(execute|apply|dispatch|commit|mutate|write)\b")
    for relative in inert:
        source = read_required(root, relative, errors)
        if source is None:
            continue
        match = risky.search(strip_rust_non_code(source))
        if match:
            errors.append(f"{relative}: exposes forbidden public method {match.group(1)!r}")


def check_resume_observer(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/resume_observer.rs"
    source = read_required(root, relative, errors)
    if source is None:
        return
    code = strip_rust_non_code(source)
    if "Connection::system()" not in code or "receive_signal(" not in code:
        errors.append(f"{relative}: missing system-bus signal-only observer contract")
    for suspicious in (".call(", ".call_method(", "request_name(", "ObjectServer"):
        if suspicious in code:
            errors.append(f"{relative}: unexpected active D-Bus operation {suspicious!r}")


def check_proof_executor_is_test_only(root: Path, errors: list[str]) -> None:
    lib_relative = "crates/orbis-ui/src/lib.rs"
    proof_relative = "crates/orbis-ui/src/automation_performance_executor.rs"
    lib = read_required(root, lib_relative, errors)
    proof = read_required(root, proof_relative, errors)
    if lib is not None:
        if not re.search(
            r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*mod\s+automation_performance_executor\s*;",
            lib,
        ):
            errors.append(f"{lib_relative}: Performance executor proof must remain cfg(test)-only")
        if re.search(r"\bpub\s+mod\s+automation_performance_executor\b", lib):
            errors.append(f"{lib_relative}: proof executor must not be publicly exported")
    if proof is not None:
        for marker in (
            "run_performance_proof",
            "LifecycleRevisionChanged",
            "CapabilityGenerationChanged",
            "ReadBackAfterMutation",
            "ReadBackMismatch",
            "set_performance_for_automation",
        ):
            if marker not in proof:
                errors.append(f"{proof_relative}: missing proof marker {marker!r}")
        code = strip_rust_non_code(proof)
        for forbidden in ("set_gpu_mode(", "set_fan_curve(", "set_charge_limit(", "Command::new(", "unsafe {"):
            if forbidden in code:
                errors.append(f"{proof_relative}: forbidden Performance-proof surface {forbidden!r}")


def check_automation_capability_fail_closed(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_capability.rs"
    source = read_required(root, relative, errors)
    if source is None:
        return
    code = strip_rust_non_code(source)
    if "write: OperationCapability::new(CapabilityStatus::Supported)" in code:
        errors.append(f"{relative}: Automation write support must remain disabled")
    if "write: OperationCapability::with_reason(CapabilityStatus::Supported" in code:
        errors.append(f"{relative}: Automation write support must remain disabled")


def check_manifests(root: Path, errors: list[str]) -> None:
    workspace = read_required(root, "Cargo.toml", errors)
    ui = read_required(root, "crates/orbis-ui/Cargo.toml", errors)
    if workspace is not None:
        if re.search(r"(?m)^\s*futures-util\s*=", workspace):
            errors.append("Cargo.toml: redundant direct futures-util dependency")
        if 'zbus = "5"' not in workspace:
            errors.append("Cargo.toml: missing workspace zbus dependency")
    if ui is not None:
        if re.search(r"(?m)^\s*futures-util\s*=", ui):
            errors.append("crates/orbis-ui/Cargo.toml: redundant direct futures-util dependency")
        if "zbus = { workspace = true }" not in ui:
            errors.append("crates/orbis-ui/Cargo.toml: missing workspace zbus dependency")


def run(root: Path) -> list[str]:
    errors: list[str] = []
    check_required_markers(root, errors)
    check_hardware_inert_sources(root, errors)
    check_unique_owners_and_replay(root, errors)
    check_runtime_readiness(root, errors)
    check_public_execution_surfaces(root, errors)
    check_resume_observer(root, errors)
    check_proof_executor_is_test_only(root, errors)
    check_automation_capability_fail_closed(root, errors)
    check_manifests(root, errors)
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".", help="repository root")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    root = Path(parse_args(argv).root).resolve()
    errors = run(root)
    if errors:
        for error in errors:
            print(f"automation-shadow-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("automation-shadow-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
