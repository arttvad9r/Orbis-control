#!/usr/bin/env python3
"""Static checks for provider-timeout and capability-refresh safety contracts.

This check is intentionally narrower than Cargo tests. It protects source-level
invariants that are easy to regress while executable Rust CI is unavailable:
canonical provider timeout ownership, bounded public probes, worker-owned
read-only refreshes, and one mutation-status requery path for both explicit and
periodic capability refresh. It does not claim mutation timeout safety.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

FILES = {
    "execution": "crates/orbis-providers/src/execution.rs",
    "bounded_probes": "crates/orbis-providers/src/bounded_probes.rs",
    "providers_lib": "crates/orbis-providers/src/lib.rs",
    "worker": "crates/orbis-ui/src/worker_runtime.rs",
    "cli": "crates/orbis-cli/src/main.rs",
}


def read(root: Path, relative: str, errors: list[str]) -> str:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return ""


def require(source: str, relative: str, markers: tuple[str, ...], errors: list[str]) -> None:
    for marker in markers:
        if marker not in source:
            errors.append(f"{relative}: missing required marker {marker!r}")


def forbid(source: str, relative: str, markers: tuple[str, ...], errors: list[str]) -> None:
    for marker in markers:
        if marker in source:
            errors.append(f"{relative}: forbidden marker present {marker!r}")


def production_prefix(source: str) -> str:
    return source.split("#[cfg(test)]", 1)[0]


def run(root: Path) -> list[str]:
    errors: list[str] = []
    sources = {key: read(root, path, errors) for key, path in FILES.items()}

    execution = sources["execution"]
    # The timeout boundary is owned by the lower-level `bounded_operation`
    # primitive, which applies the supplied deadline via Tokio and classifies
    # expiry as `ProviderError::Timeout`. `bounded_provider_call` delegates to it
    # with the provider-declared deadline. These structural markers verify the
    # real timeout contract without depending on a specific inline layout.
    require(
        execution,
        FILES["execution"],
        (
            "pub async fn bounded_operation",
            "tokio::time::timeout(limit, future).await",
            "ProviderError::Timeout",
            "pub async fn bounded_provider_call",
            "bounded_operation(provider.timeout()",
            "This function never retries",
            "never_completing_operation_becomes_timeout",
        ),
        errors,
    )
    # A generic retry loop in the timeout primitive would be unsafe for writes.
    forbid(production_prefix(execution), FILES["execution"], ("loop {", "retry("), errors)

    bounded = sources["bounded_probes"]
    require(
        bounded,
        FILES["bounded_probes"],
        (
            "struct BoundedPerformance",
            '"performance.profiles"',
            '"performance.current_profile"',
            "read-only bounded Performance probe adapter does not expose mutation",
            "CapabilityStatus::TemporarilyUnavailable",
            "bounded_single_read_probe",
            "timed_out_single_read_probe_is_temporarily_unavailable_not_unsupported",
            "performance_timeout_is_per_read_and_classified_locally",
        ),
        errors,
    )

    providers_lib = sources["providers_lib"]
    require(
        providers_lib,
        FILES["providers_lib"],
        (
            "pub mod bounded_probes;",
            "pub mod execution;",
            "pub use bounded_probes::{",
            "pub use execution::{bounded_operation, bounded_provider_call};",
        ),
        errors,
    )
    if "pub use probes::{" in providers_lib:
        errors.append(
            f"{FILES['providers_lib']}: raw probes must not shadow bounded public probe exports"
        )

    worker = sources["worker"]
    production_worker = production_prefix(worker)
    require(
        production_worker,
        FILES["worker"],
        (
            "async fn bounded_performance_state",
            "async fn bounded_charge_limit",
            "async fn bounded_gpu_capabilities",
            "async fn bounded_fan_curve",
            "tokio::join!(",
            "async fn refresh_capability_registry",
            "runtime.requery_mutation_statuses().await;",
        ),
        errors,
    )
    forbid(
        production_worker,
        FILES["worker"],
        (
            "runtime.gpu.refresh_gpu_capabilities().await",
            "runtime.battery.charge_limit().await",
            "runtime.performance.performance_state().await",
            "runtime.fan.fan_curve_for_profile",
            "run_capability_refresh",
        ),
        errors,
    )
    if production_worker.count("runtime.requery_mutation_statuses().await") != 1:
        errors.append(
            f"{FILES['worker']}: mutation-status requery must have exactly one canonical owner"
        )
    if production_worker.count("refresh_capability_registry(&mut runtime).await") != 2:
        errors.append(
            f"{FILES['worker']}: explicit and periodic refresh must both use the canonical helper"
        )

    # Mutation commands deliberately remain outside the generic timeout helper
    # until unknown-outcome recovery is modeled end-to-end.
    for mutation in ("set_performance", "set_gpu_mode", "set_charge_limit", "set_fan_curve"):
        suspicious = f"bounded_provider_call(runtime.{mutation}"
        if suspicious in production_worker:
            errors.append(
                f"{FILES['worker']}: mutation {mutation} was naively routed through generic timeout"
            )

    cli = sources["cli"]
    require(
        cli,
        FILES["cli"],
        (
            "use orbis_providers::bounded_provider_call;",
            '"battery.charge_limit"',
            '"performance.current"',
            '"gpu.power"',
        ),
        errors,
    )

    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(argv)
    errors = run(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"provider-timeout-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("provider-timeout-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
