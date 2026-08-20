#!/usr/bin/env python3
"""Static fail-closed checks for the future Automation executor boundary.

This checker intentionally uses only the Python standard library. It does not
claim Rust type correctness; it protects the architectural boundary while the
sandbox lacks Cargo. Serialization/scope code must remain hardware-inert, and
the first real Performance mutation proof must stay test-only until executable
validation and worker-generation serialization are available.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

SERIALIZATION = "crates/orbis-ui/src/automation_serialization.rs"
SCOPE = "crates/orbis-ui/src/automation_execution_scope.rs"
PROOF = "crates/orbis-ui/src/automation_performance_executor.rs"
GUARD = "crates/orbis-ui/src/automation_execution_guard.rs"
LIB = "crates/orbis-ui/src/lib.rs"

REQUIRED = {
    SERIALIZATION: (
        "AutomationSerializationCoordinator",
        "AutomationDryRunLease",
        "AutomationExecutionHandoff",
        "CapabilityGenerationChanged",
        "Busy",
        "SequenceExhausted",
        "current_generation",
        "checked_add",
        "pub fn admit",
        "pub fn finish",
    ),
    SCOPE: (
        "AutomationPreparedKind",
        "AutomationPreparedBatch",
        "DuplicatePerformanceAction",
        "UnsupportedAction",
        "prepare_automation_execution_scope",
        "AutomationAction::SetProfile",
    ),
    PROOF: (
        "Test-only proof",
        "PerformanceAutomationOwner",
        "run_performance_proof",
        "CommandError::ReadBack",
        "ReadBackAfterMutation",
        "UnexpectedApplyResult",
        "ReadBackMismatch",
        "outcome.result.is_applied()",
    ),
    GUARD: (
        "AutomationExecutionCandidate",
        "revalidate_automation_candidate",
        "generation_still_matches",
    ),
    LIB: (
        "pub mod automation_serialization;",
        "pub mod automation_execution_scope;",
        "mod automation_performance_executor;",
    ),
}

# Build dangerous spellings from fragments so this file does not self-match.
FORBIDDEN_INERT = tuple(
    "".join(parts)
    for parts in (
        ("WorkerCommand::", "Set"),
        ("set_", "performance("),
        ("set_", "profile("),
        ("set_", "gpu_mode("),
        ("set_", "fan_curve("),
        ("set_", "charge_limit("),
        ("std::process::", "Command"),
        ("Command", "::new("),
        ("unsafe", " {"),
        ("unsafe", " fn"),
    )
)

RISKY_PUBLIC = re.compile(
    r"\bpub\s+(?:async\s+)?fn\s+(execute|apply|dispatch|commit|mutate|write)\b"
)
LEASE_DERIVE = re.compile(
    r"#\s*\[\s*derive\((?P<traits>[^\)]*)\)\s*\]\s*"
    r"pub\s+struct\s+AutomationDryRunLease\b",
    re.S,
)
ADMIT_SIGNATURE = re.compile(
    r"pub\s+fn\s+admit\s*\(\s*&mut\s+self\s*,\s*"
    r"handoff\s*:\s*AutomationExecutionHandoff\s*,\s*"
    r"current_generation\s*:\s*u64\s*,?\s*\)",
    re.S,
)
FINISH_SIGNATURE = re.compile(
    r"pub\s+fn\s+finish\s*\(\s*&mut\s+self\s*,\s*"
    r"lease\s*:\s*AutomationDryRunLease\s*\)",
    re.S,
)
TEST_ONLY_PROOF = re.compile(
    r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*"
    r"mod\s+automation_performance_executor\s*;",
    re.S,
)


def read(root: Path, relative: str, errors: list[str]) -> str | None:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return None


def run(root: Path) -> list[str]:
    errors: list[str] = []
    sources: dict[str, str] = {}

    for relative, markers in REQUIRED.items():
        source = read(root, relative, errors)
        if source is None:
            continue
        sources[relative] = source
        for marker in markers:
            if marker not in source:
                errors.append(f"{relative}: missing required marker {marker!r}")

    for relative in (SERIALIZATION, SCOPE):
        source = sources.get(relative)
        if source is None:
            continue
        for token in FORBIDDEN_INERT:
            if token in source:
                errors.append(f"{relative}: forbidden execution token {token!r}")
        risky = RISKY_PUBLIC.search(source)
        if risky:
            errors.append(
                f"{relative}: forbidden public execution method {risky.group(1)!r}"
            )

    serialization = sources.get(SERIALIZATION)
    if serialization is not None:
        derive = LEASE_DERIVE.search(serialization)
        if not derive:
            errors.append(f"{SERIALIZATION}: cannot locate AutomationDryRunLease derive")
        else:
            traits = {item.strip() for item in derive.group("traits").split(",")}
            if "Clone" in traits or "Copy" in traits:
                errors.append(f"{SERIALIZATION}: dry-run lease must not be Clone/Copy")

        if not ADMIT_SIGNATURE.search(serialization):
            errors.append(
                f"{SERIALIZATION}: admit must consume AutomationExecutionHandoff and take current generation"
            )
        if not FINISH_SIGNATURE.search(serialization):
            errors.append(
                f"{SERIALIZATION}: finish must consume the exact AutomationDryRunLease"
            )
        if "required != current_generation" not in serialization:
            errors.append(
                f"{SERIALIZATION}: missing final generation comparison before slot ownership"
            )
        if "self.active_lease = Some(id)" not in serialization:
            errors.append(f"{SERIALIZATION}: missing exclusive slot ownership marker")
        if "self.active_lease = None" not in serialization:
            errors.append(f"{SERIALIZATION}: missing explicit lease release marker")

    lib = sources.get(LIB)
    if lib is not None:
        if not TEST_ONLY_PROOF.search(lib):
            errors.append(
                f"{LIB}: Performance executor proof must remain behind #[cfg(test)]"
            )
        if "pub mod automation_performance_executor" in lib:
            errors.append(
                f"{LIB}: Performance executor proof must not be publicly exported"
            )

    # Until promotion, production runtime owners must not reference the proof
    # module at all. Reading these files is cheap and catches accidental wiring.
    for relative in (
        "crates/orbis-ui/src/main.rs",
        "crates/orbis-ui/src/worker.rs",
        "crates/orbis-ui/src/automation_backend.rs",
        "crates/orbis-ui/src/quick_controls_backend.rs",
    ):
        source = read(root, relative, errors)
        if source is not None and "automation_performance_executor" in source:
            errors.append(
                f"{relative}: test-only Performance executor proof is wired into production"
            )

    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(argv)
    errors = run(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"automation-executor-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("automation-executor-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
