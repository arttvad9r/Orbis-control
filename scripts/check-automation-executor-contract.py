#!/usr/bin/env python3
"""Static fail-closed checks for worker-owned Automation execution.

This script is intentionally not a substitute for Rust type checking. It protects
architecture invariants that are reviewable without Cargo:

- Automation lifecycle/policy/revision/serialization/recovery state is owned by
  the same sequential worker module as application mutations;
- logind lifecycle observations reach that worker through a typed channel;
- the only compiled unattended mutation executor is Performance-only;
- policy/lifecycle/capability identities are rechecked immediately before the
  owner call;
- unknown post-mutation outcomes enter typed recovery;
- this exact revision remains promotion-gated and the canonical Automation
  capability remains read-only/write-Unsupported.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

CAPABILITY = "crates/orbis-ui/src/automation_capability.rs"
REVISION = "crates/orbis-ui/src/automation_lifecycle_revision.rs"
SERIALIZATION = "crates/orbis-ui/src/automation_serialization.rs"
SCOPE = "crates/orbis-ui/src/automation_execution_scope.rs"
RECOVERY = "crates/orbis-ui/src/automation_recovery.rs"
WORKER_RUNTIME_STATE = "crates/orbis-ui/src/automation_worker_runtime.rs"
WORKER_COORDINATOR = "crates/orbis-ui/src/automation_worker_coordinator.rs"
WORKER_DRIVER = "crates/orbis-ui/src/automation_worker_driver.rs"
EXECUTOR = "crates/orbis-ui/src/automation_performance_executor.rs"
WORKER = "crates/orbis-ui/src/worker_runtime.rs"
RESUME = "crates/orbis-ui/src/resume_observer.rs"
LIB = "crates/orbis-ui/src/lib.rs"

REQUIRED = {
    CAPABILITY: (
        "CapabilityStatus::ReadOnly",
        "read: OperationCapability::new(CapabilityStatus::Supported)",
        "CapabilityStatus::Unsupported",
        "augment_snapshot_with_automation_shadow",
        "executable_validation_passed",
    ),
    REVISION: (
        "AutomationLifecycleRevision",
        "AutomationLifecycleClock",
        "AutomationRevisionCandidate",
        "revalidate_revision_candidate",
    ),
    SERIALIZATION: (
        "AutomationDryRunLease",
        "AutomationSerializationCoordinator",
        "required_revision",
        "required_generation",
        "last_admitted_revision",
        "pub fn admit",
        "pub fn finish",
    ),
    SCOPE: (
        "AutomationPreparedKind",
        "AutomationPreparedBatch",
        "AutomationAction::SetProfile",
        "UnsupportedAction",
    ),
    RECOVERY: (
        "AutomationRecoveryBarrier",
        "mark_performance_unknown",
        "reconcile_performance",
        "RecoveredAtRequested",
        "RecoveredAtDifferent",
    ),
    WORKER_RUNTIME_STATE: (
        "AutomationWorkerRuntime",
        "latest_candidate",
        "prepare_latest",
        "AutomationSerializationCoordinator",
    ),
    WORKER_COORDINATOR: (
        "AutomationWorkerCoordinator",
        "RecoveryRequired",
        "finish_performance_unknown",
        "reconcile_performance",
    ),
    WORKER_DRIVER: (
        "AutomationWorkerDriver",
        "AutomationPolicyRevision",
        "AutomationWorkerPreparedEnvelope",
        "required_policy_revision",
        "prepare_latest_dry_run",
        "finish_performance_unknown",
        "replace_persisted_policy",
        "clear_persisted_policy",
    ),
    EXECUTOR: (
        "Production-ready Performance-only Automation execution boundary",
        "PerformanceAutomationOwner",
        "execute_prepared_performance",
        "PolicyRevisionChanged",
        "LifecycleRevisionChanged",
        "CapabilityGenerationChanged",
        "CommandError::ReadBack",
        "RecoveryRequired",
        "outcome.result.is_applied()",
        "ReadBackMismatch",
    ),
    WORKER: (
        "AutomationWorkerDriver::new()",
        "sync_persisted_automation_policy",
        "publish_prepare_for_sleep",
        "observe_prepare_for_sleep",
        "observe_automation_telemetry",
        "augment_snapshot_with_automation_shadow",
        "prepare_latest_dry_run",
        "execute_prepared_performance",
        "finish_performance_unknown",
        "reconcile_performance",
        "AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED: bool = false",
    ),
    RESUME: (
        "PrepareForSleep",
        "receive_signal",
        "publish_prepare_for_sleep",
    ),
    LIB: (
        "pub mod automation_performance_executor;",
        '#[path = "worker_runtime.rs"]',
        "pub mod worker;",
    ),
}

INERT_FILES = (
    CAPABILITY,
    REVISION,
    SERIALIZATION,
    SCOPE,
    RECOVERY,
    WORKER_RUNTIME_STATE,
    WORKER_COORDINATOR,
    WORKER_DRIVER,
)

FORBIDDEN_INERT = tuple(
    "".join(parts)
    for parts in (
        ("set_", "performance("),
        ("set_", "gpu_mode("),
        ("set_", "fan_curve("),
        ("set_", "charge_limit("),
        ("Command", "::new("),
        ("unsafe", " fn"),
    )
)

NONCLONE_TYPES = {
    SERIALIZATION: (
        "AutomationDryRunLease",
        "AutomationSerializationCoordinator",
    ),
    RECOVERY: ("AutomationRecoveryBarrier",),
    WORKER_RUNTIME_STATE: ("AutomationWorkerRuntime",),
    WORKER_COORDINATOR: ("AutomationWorkerCoordinator",),
    WORKER_DRIVER: (
        "AutomationWorkerDriver",
        "AutomationWorkerPreparedEnvelope",
    ),
}


def read(root: Path, relative: str, errors: list[str]) -> str | None:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return None


def derive_traits(source: str, type_name: str) -> set[str] | None:
    pattern = re.compile(
        r"#\s*\[\s*derive\((?P<traits>[^\)]*)\)\s*\]\s*"
        + r"pub\s+struct\s+"
        + re.escape(type_name)
        + r"\b",
        re.S,
    )
    match = pattern.search(source)
    if match is None:
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
                errors.append(f"{relative}: inert layer contains mutation/process token {token!r}")

    for relative, type_names in NONCLONE_TYPES.items():
        source = sources.get(relative)
        if source is None:
            continue
        for type_name in type_names:
            traits = derive_traits(source, type_name)
            if traits is None:
                errors.append(f"{relative}: cannot locate derive for {type_name}")
                continue
            if {"Clone", "Copy"} & traits:
                errors.append(f"{relative}: {type_name} must not be Clone/Copy")

    capability = sources.get(CAPABILITY, "")
    risky_capability_tokens = (
        "write: OperationCapability::new(CapabilityStatus::Supported)",
        "write: OperationCapability::with_reason(CapabilityStatus::Supported",
    )
    for token in risky_capability_tokens:
        if token in capability:
            errors.append(f"{CAPABILITY}: Automation write was promoted by static source")

    worker = sources.get(WORKER, "")
    if worker:
        if "const AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED: bool = false;" not in worker:
            errors.append(f"{WORKER}: exact revision must remain promotion-gated false")
        dry_run = worker.find("if !AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED")
        execute = worker.find("execute_prepared_performance(")
        if dry_run < 0 or execute < 0 or dry_run > execute:
            errors.append(f"{WORKER}: promotion gate must precede executor call")
        for marker in (
            "driver.current_policy_revision()",
            "driver.current_revision()",
            "capabilities.generation()",
        ):
            if marker not in worker:
                errors.append(f"{WORKER}: final executor identity missing {marker!r}")
        if "tokio::spawn(async move" not in worker:
            errors.append(f"{WORKER}: expected existing telemetry background snapshot task")

    executor = sources.get(EXECUTOR, "")
    if executor:
        owner_call = executor.find("owner.set_performance_for_automation(profile).await")
        checks = (
            executor.find("current_policy_revision != required_policy_revision"),
            executor.find("current_lifecycle_revision != required_revision"),
            executor.find("current_generation != required_generation"),
        )
        if owner_call < 0:
            errors.append(f"{EXECUTOR}: Performance owner call missing")
        elif any(index < 0 or index > owner_call for index in checks):
            errors.append(f"{EXECUTOR}: all identity checks must precede Performance owner call")

        for forbidden in (
            "set_gpu_mode(",
            "set_fan_curve(",
            "set_charge_limit(",
            "Command::new(",
            "std::process::Command",
        ):
            if forbidden in executor:
                errors.append(f"{EXECUTOR}: escaped Performance-only scope via {forbidden!r}")

    coordinator = sources.get(WORKER_COORDINATOR, "")
    if coordinator:
        mark = coordinator.find("mark_performance_unknown(prepared.lease(), requested)")
        release = coordinator.find("self.runtime.finish(prepared)", max(mark, 0))
        if mark < 0 or release < 0 or mark > release:
            errors.append(
                f"{WORKER_COORDINATOR}: recovery must be marked before serialization lease release"
            )

    recovery = sources.get(RECOVERY, "")
    if recovery:
        for generic_clear in ("pub fn clear(", "pub fn reset("):
            if generic_clear in recovery:
                errors.append(f"{RECOVERY}: unknown-outcome barrier exposes generic clear/reset")
        if "self.pending = None" not in recovery:
            errors.append(f"{RECOVERY}: typed reconciliation does not clear pending recovery")

    resume = sources.get(RESUME, "")
    if resume:
        for forbidden in (
            "set_performance(",
            "set_gpu_mode(",
            "set_fan_curve(",
            "set_charge_limit(",
            "Command::new(",
        ):
            if forbidden in resume:
                errors.append(f"{RESUME}: lifecycle observer contains mutation surface {forbidden!r}")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()

    errors = run(args.root.resolve())
    if errors:
        print("Automation executor contract: FAIL", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print("Automation executor contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
