#!/usr/bin/env python3
"""Static fail-closed checks for the Automation execution boundary.

This checker uses only the Python standard library. It is not Rust type
validation. It protects the architectural barriers that must remain true while
Cargo/Clippy are unavailable in the review sandbox:

- every execution candidate is bound to a monotonic lifecycle revision;
- serialization checks lifecycle revision and capability generation and blocks
  replay of an already-admitted event;
- worker-owned preparation state is unique/non-cloneable and hardware-inert;
- the first mutation proof remains Performance-only and `cfg(test)`;
- Automation capability may advertise shadow/read support but never write.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REVISION = "crates/orbis-ui/src/automation_lifecycle_revision.rs"
SERIALIZATION = "crates/orbis-ui/src/automation_serialization.rs"
SCOPE = "crates/orbis-ui/src/automation_execution_scope.rs"
WORKER_RUNTIME = "crates/orbis-ui/src/automation_worker_runtime.rs"
PROOF = "crates/orbis-ui/src/automation_performance_executor.rs"
CAPABILITY = "crates/orbis-ui/src/automation_capability.rs"
LIB = "crates/orbis-ui/src/lib.rs"

REQUIRED: dict[str, tuple[str, ...]] = {
    REVISION: (
        "AutomationLifecycleRevision",
        "AutomationLifecycleClock",
        "SequenceExhausted",
        "AutomationRevisionCandidate",
        "LifecycleRevisionChanged",
        "revalidate_revision_candidate",
        "identities_still_match",
    ),
    SERIALIZATION: (
        "AutomationRevisionHandoff",
        "AutomationDryRunLease",
        "required_revision",
        "required_generation",
        "LifecycleRevisionChanged",
        "LifecycleRevisionAlreadyAdmitted",
        "CapabilityGenerationChanged",
        "last_admitted_revision",
        "current_revision",
        "current_generation",
        "pub fn admit",
        "pub fn finish",
    ),
    SCOPE: (
        "AutomationPreparedKind",
        "AutomationPreparedBatch",
        "DuplicatePerformanceAction",
        "UnsupportedAction",
        "AutomationAction::SetProfile",
        "required_revision",
        "required_generation",
    ),
    WORKER_RUNTIME: (
        "AutomationLifecycleClock",
        "AutomationSerializationCoordinator",
        "latest_candidate",
        "prepare_latest",
        "current_revision",
        "required_revision",
        "NoReadyCandidate",
    ),
    PROOF: (
        "Test-only proof",
        "PerformanceAutomationOwner",
        "run_performance_proof",
        "LifecycleRevisionChanged",
        "CapabilityGenerationChanged",
        "current_revision",
        "current_generation",
        "CommandError::ReadBack",
        "ReadBackAfterMutation",
        "UnexpectedApplyResult",
        "ReadBackMismatch",
        "outcome.result.is_applied()",
    ),
    CAPABILITY: (
        "CapabilityStatus::ReadOnly",
        "read: OperationCapability::new(CapabilityStatus::Supported)",
        "CapabilityStatus::Unsupported",
        "CapabilityConstraints::None",
    ),
    LIB: (
        "pub mod automation_capability;",
        "pub mod automation_execution_scope;",
        "pub mod automation_lifecycle_revision;",
        "pub mod automation_serialization;",
        "pub mod automation_worker_runtime;",
        "mod automation_performance_executor;",
    ),
}

INERT_FILES = (REVISION, SERIALIZATION, SCOPE, WORKER_RUNTIME, CAPABILITY)
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

TEST_ONLY_PROOF = re.compile(
    r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*"
    r"mod\s+automation_performance_executor\s*;",
    re.S,
)
DRY_LEASE_DERIVE = re.compile(
    r"#\s*\[\s*derive\((?P<traits>[^\)]*)\)\s*\]\s*"
    r"pub\s+struct\s+AutomationDryRunLease\b",
    re.S,
)
SERIALIZATION_DERIVE = re.compile(
    r"#\s*\[\s*derive\((?P<traits>[^\)]*)\)\s*\]\s*"
    r"pub\s+struct\s+AutomationSerializationCoordinator\b",
    re.S,
)
WORKER_RUNTIME_DERIVE = re.compile(
    r"#\s*\[\s*derive\((?P<traits>[^\)]*)\)\s*\]\s*"
    r"pub\s+struct\s+AutomationWorkerRuntime\b",
    re.S,
)
ADMIT_SIGNATURE = re.compile(
    r"pub\s+fn\s+admit\s*\(\s*&mut\s+self\s*,\s*"
    r"handoff\s*:\s*AutomationRevisionHandoff\s*,\s*"
    r"current_revision\s*:\s*AutomationLifecycleRevision\s*,\s*"
    r"current_generation\s*:\s*u64\s*,?\s*\)",
    re.S,
)


def read(root: Path, relative: str, errors: list[str]) -> str | None:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return None


def traits_from(pattern: re.Pattern[str], source: str) -> set[str] | None:
    match = pattern.search(source)
    if not match:
        return None
    return {item.strip() for item in match.group("traits").split(",")}


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

    for relative in INERT_FILES:
        source = sources.get(relative)
        if source is None:
            continue
        for token in FORBIDDEN_INERT:
            if token in source:
                errors.append(f"{relative}: forbidden mutation/process token {token!r}")

    serialization = sources.get(SERIALIZATION)
    if serialization is not None:
        lease_traits = traits_from(DRY_LEASE_DERIVE, serialization)
        if lease_traits is None:
            errors.append(f"{SERIALIZATION}: cannot locate AutomationDryRunLease derive")
        elif {"Clone", "Copy"} & lease_traits:
            errors.append(f"{SERIALIZATION}: dry-run lease must not be Clone/Copy")

        owner_traits = traits_from(SERIALIZATION_DERIVE, serialization)
        if owner_traits is None:
            errors.append(
                f"{SERIALIZATION}: cannot locate AutomationSerializationCoordinator derive"
            )
        elif {"Clone", "Copy"} & owner_traits:
            errors.append(f"{SERIALIZATION}: serialization owner must not be Clone/Copy")

        if not ADMIT_SIGNATURE.search(serialization):
            errors.append(
                f"{SERIALIZATION}: admit must consume revision handoff and take both current identities"
            )
        for marker in (
            "required_revision != current_revision",
            "required_generation != current_generation",
            "required_revision <= self.last_admitted_revision",
            "self.last_admitted_revision = required_revision",
            "self.active_lease = Some(id)",
            "self.active_lease = None",
        ):
            if marker not in serialization:
                errors.append(f"{SERIALIZATION}: missing admission/replay invariant {marker!r}")

    worker_runtime = sources.get(WORKER_RUNTIME)
    if worker_runtime is not None:
        runtime_traits = traits_from(WORKER_RUNTIME_DERIVE, worker_runtime)
        if runtime_traits is None:
            errors.append(f"{WORKER_RUNTIME}: cannot locate AutomationWorkerRuntime derive")
        elif {"Clone", "Copy"} & runtime_traits:
            errors.append(f"{WORKER_RUNTIME}: unique worker-owned runtime must not be Clone/Copy")
        if "self.latest_candidate = candidate.clone()" not in worker_runtime:
            errors.append(
                f"{WORKER_RUNTIME}: every confirmed event must replace/supersede previous candidate"
            )

    lib = sources.get(LIB)
    if lib is not None:
        if not TEST_ONLY_PROOF.search(lib):
            errors.append(f"{LIB}: Performance mutation proof must remain behind #[cfg(test)]")
        if re.search(r"\bpub(?:\(crate\))?\s+mod\s+automation_performance_executor\b", lib):
            errors.append(f"{LIB}: Performance mutation proof must not be production-exported")

    proof = sources.get(PROOF)
    if proof is not None:
        for token in (
            "set_gpu_mode(",
            "set_fan_curve(",
            "set_charge_limit(",
            "Command::new(",
            "std::process::Command",
        ):
            if token in proof:
                errors.append(f"{PROOF}: Performance proof escaped scope via {token!r}")
        if "current_revision != required_revision" not in proof:
            errors.append(f"{PROOF}: missing final lifecycle revision comparison")
        if "current_generation != required_generation" not in proof:
            errors.append(f"{PROOF}: missing final capability generation comparison")

    capability = sources.get(CAPABILITY)
    if capability is not None:
        risky_write = re.compile(
            r"write\s*:\s*OperationCapability::(?:new|with_reason)\(\s*"
            r"CapabilityStatus::Supported\b"
        )
        if risky_write.search(capability):
            errors.append(f"{CAPABILITY}: shadow capability must never advertise write support")

    # Until promotion, no production runtime path may reference the test proof.
    for relative in (
        "crates/orbis-ui/src/main.rs",
        "crates/orbis-ui/src/worker.rs",
        "crates/orbis-ui/src/automation_backend.rs",
        "crates/orbis-ui/src/automation_worker_runtime.rs",
        "crates/orbis-ui/src/quick_controls_backend.rs",
    ):
        source = read(root, relative, errors)
        if source is not None and "automation_performance_executor" in source:
            errors.append(f"{relative}: test-only Performance proof is wired into production")

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
