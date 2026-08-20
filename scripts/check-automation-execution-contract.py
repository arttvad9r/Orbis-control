#!/usr/bin/env python3
"""Static fail-closed checks for Automation execution-boundary scaffolding.

This checker is intentionally standard-library-only. It does not prove Rust
semantics or replace `cargo check/test/clippy`; it protects the architectural
barriers that must remain true while executable validation is unavailable:

- lifecycle revision supersedes stale events independently of capabilities;
- serialization carries both revision and capability generation;
- the worker-owned state machine is single-owner and hardware-inert;
- the only mutation proof is Performance-only and compiled under `cfg(test)`;
- canonical Automation capability is read/shadow-supported but write-disabled.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REQUIRED: dict[str, tuple[str, ...]] = {
    "crates/orbis-ui/src/automation_lifecycle_revision.rs": (
        "AutomationLifecycleRevision",
        "SequenceExhausted",
        "LifecycleRevisionChanged",
        "revalidate_revision_candidate",
        "identities_still_match",
    ),
    "crates/orbis-ui/src/automation_serialization.rs": (
        "required_revision",
        "required_generation",
        "LifecycleRevisionChanged",
        "current_revision",
        "current_generation",
        "AutomationDryRunLease",
    ),
    "crates/orbis-ui/src/automation_execution_scope.rs": (
        "AutomationPreparedKind::Performance",
        "UnsupportedAction",
        "DuplicatePerformanceAction",
        "required_revision",
        "required_generation",
    ),
    "crates/orbis-ui/src/automation_worker_runtime.rs": (
        "AutomationLifecycleClock",
        "latest_candidate",
        "prepare_latest",
        "AutomationSerializationCoordinator",
        "current_revision",
        "required_revision",
    ),
    "crates/orbis-ui/src/automation_performance_executor.rs": (
        "LifecycleRevisionChanged",
        "CapabilityGenerationChanged",
        "current_revision",
        "current_generation",
        "set_performance_for_automation",
        "ReadBackAfterMutation",
        "ReadBackMismatch",
        "is_applied()",
    ),
    "crates/orbis-ui/src/automation_capability.rs": (
        "CapabilityStatus::ReadOnly",
        "read: OperationCapability::new(CapabilityStatus::Supported)",
        "CapabilityStatus::Unsupported",
        "CapabilityConstraints::None",
    ),
}

HARDWARE_INERT = (
    "crates/orbis-ui/src/automation_lifecycle_revision.rs",
    "crates/orbis-ui/src/automation_serialization.rs",
    "crates/orbis-ui/src/automation_execution_scope.rs",
    "crates/orbis-ui/src/automation_worker_runtime.rs",
    "crates/orbis-ui/src/automation_capability.rs",
)

FORBIDDEN_INERT_CODE = (
    "WorkerCommand::Set",
    "set_performance(",
    "set_gpu_mode(",
    "set_fan_curve(",
    "set_charge_limit(",
    "std::process::Command",
    "Command::new(",
    "unsafe {",
    "unsafe fn",
)


def read(root: Path, relative: str, errors: list[str]) -> str | None:
    path = root / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return None


def strip_comments_and_strings(source: str) -> str:
    """Small Rust-oriented lexer sufficient for executable-token checks."""

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

        raw_prefix = None
        if source.startswith("br", i):
            raw_prefix = 2
        elif source.startswith("r", i):
            raw_prefix = 1
        if raw_prefix is not None:
            j = i + raw_prefix
            hashes = 0
            while j < n and source[j] == "#":
                hashes += 1
                j += 1
            if j < n and source[j] == '"':
                terminator = '"' + "#" * hashes
                end = source.find(terminator, j + 1)
                end = n if end < 0 else end + len(terminator)
                blank(i, end)
                i = end
                continue

        literal_start = i
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
            blank(literal_start, j)
            i = j
            continue

        i += 1

    return "".join(out)


def check_required(root: Path, errors: list[str]) -> None:
    for relative, markers in REQUIRED.items():
        source = read(root, relative, errors)
        if source is None:
            continue
        for marker in markers:
            if marker not in source:
                errors.append(f"{relative}: missing required marker {marker!r}")


def check_inert_files(root: Path, errors: list[str]) -> None:
    for relative in HARDWARE_INERT:
        source = read(root, relative, errors)
        if source is None:
            continue
        code = strip_comments_and_strings(source)
        for token in FORBIDDEN_INERT_CODE:
            if token in code:
                errors.append(f"{relative}: forbidden mutation/process token {token!r}")


def check_single_owner_runtime(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_worker_runtime.rs"
    source = read(root, relative, errors)
    if source is None:
        return
    clone_derive = re.compile(
        r"#\[derive\([^\]]*\bClone\b[^\]]*\)\]\s*pub\s+struct\s+AutomationWorkerRuntime\b",
        re.MULTILINE,
    )
    if clone_derive.search(source):
        errors.append(f"{relative}: worker-owned serialization runtime must not be Clone")


def check_test_only_executor(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/lib.rs"
    source = read(root, relative, errors)
    if source is None:
        return
    expected = "#[cfg(test)]\nmod automation_performance_executor;"
    if expected not in source:
        errors.append(
            f"{relative}: Performance Automation executor proof must remain cfg(test)-only"
        )
    if re.search(r"\bpub(?:\(crate\))?\s+mod\s+automation_performance_executor\b", source):
        errors.append(f"{relative}: Performance proof executor must not be production-exported")


def check_performance_only_proof(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_performance_executor.rs"
    source = read(root, relative, errors)
    if source is None:
        return
    code = strip_comments_and_strings(source)
    for token in (
        "set_gpu_mode(",
        "set_fan_curve(",
        "set_charge_limit(",
        "Command::new(",
        "std::process::Command",
    ):
        if token in code:
            errors.append(f"{relative}: proof executor escaped Performance-only scope via {token!r}")
    revision_check = re.search(r"current_revision\s*!=\s*required_revision", code)
    generation_check = re.search(r"current_generation\s*!=\s*required_generation", code)
    if not revision_check:
        errors.append(f"{relative}: missing immediate lifecycle revision check")
    if not generation_check:
        errors.append(f"{relative}: missing immediate capability generation check")


def check_shadow_capability(root: Path, errors: list[str]) -> None:
    relative = "crates/orbis-ui/src/automation_capability.rs"
    source = read(root, relative, errors)
    if source is None:
        return
    code = strip_comments_and_strings(source)
    if re.search(
        r"write\s*:\s*OperationCapability::(?:new|with_reason)\(\s*CapabilityStatus::Supported\b",
        code,
    ):
        errors.append(f"{relative}: Automation shadow capability must not advertise write support")


def run(root: Path) -> list[str]:
    errors: list[str] = []
    check_required(root, errors)
    check_inert_files(root, errors)
    check_single_owner_runtime(root, errors)
    check_test_only_executor(root, errors)
    check_performance_only_proof(root, errors)
    check_shadow_capability(root, errors)
    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(argv)
    errors = run(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"automation-execution-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("automation-execution-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
