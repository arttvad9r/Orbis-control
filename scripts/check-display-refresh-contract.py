#!/usr/bin/env python3
"""Static fail-closed checks for the developing DisplayRefresh backend contract.

This checker does not prove a compositor implementation. It protects the current
boundary: `wl_output` is observation-only, product refresh targets use an opaque
future owner identity, and production UI/Automation remain write-disabled until
that owner supplies typed target evidence.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

FILES = {
    "core": "crates/orbis-core/src/display_refresh.rs",
    "output": "crates/orbis-core/src/display_output.rs",
    "provider": "crates/orbis-providers/src/wayland_output.rs",
    "preflight": "crates/orbis-config/src/automation_preflight.rs",
    "quick": "crates/orbis-ui/src/quick_controls_backend.rs",
    "composition": "crates/orbis-ui/src/composition.rs",
}


def read(root: Path, relative: str, errors: list[str]) -> str | None:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return None


def require(source: str, relative: str, markers: tuple[str, ...], errors: list[str]) -> None:
    for marker in markers:
        if marker not in source:
            errors.append(f"{relative}: missing required marker {marker!r}")


def run(root: Path) -> list[str]:
    errors: list[str] = []
    sources: dict[str, str] = {}
    for key, relative in FILES.items():
        source = read(root, relative, errors)
        if source is not None:
            sources[key] = source

    core = sources.get("core")
    if core is not None:
        require(
            core,
            FILES["core"],
            (
                "DisplayRefreshConstraints",
                "DisplayRefreshEvidence",
                "DisplayRefreshTargetId",
                "InternalPanelProven",
                "writable_target_for",
                "same hardware width/height",
                "auto_supported",
            ),
            errors,
        )
        if re.search(r"use\s+crate::display_output::\{?[^;]*DisplayOutputId", core):
            errors.append(
                f"{FILES['core']}: compositor wl_output identity must not become mutation target identity"
            )

    output = sources.get("output")
    if output is not None:
        require(
            output,
            FILES["output"],
            (
                "do not assume that the name is a reflection of an underlying",
                "Non-current modes are deprecated",
            ),
            errors,
        )

    provider = sources.get("provider")
    if provider is not None:
        require(
            provider,
            FILES["provider"],
            (
                "Read-only Wayland display output provider",
                "никакого modeset/configuration API",
                "impl<S> DisplayOutputProvider",
            ),
            errors,
        )
        for token in ("set_mode(", "set_refresh(", "apply_mode(", "wlr_output_manager"):
            if token in provider:
                errors.append(
                    f"{FILES['provider']}: read-only wl_output provider contains unexpected mutation token {token!r}"
                )

    preflight = sources.get("preflight")
    if preflight is not None:
        require(
            preflight,
            FILES["preflight"],
            (
                "preflight_automation_plan_with_display_constraints",
                "DisplayRefreshTargetRole::InternalPanelProven",
                "DisplayRefreshPolicyNotRepresentable",
                "TargetIdentityNotProven",
                "preflight_automation_plan_with_display_constraints(plan, capabilities, None)",
            ),
            errors,
        )

    quick = sources.get("quick")
    if quick is not None:
        if "app.set_display_control_ready(false)" not in quick:
            errors.append(
                f"{FILES['quick']}: Display Quick Control must remain write-disabled"
            )
        if "production display backend is read-only" not in quick:
            errors.append(
                f"{FILES['quick']}: missing fail-closed Display request handler status"
            )

    composition = sources.get("composition")
    if composition is not None:
        # A future real owner may deliberately add this entry, at which point
        # this checker must be updated together with executable validation.
        if re.search(r"\.add\(\s*(?:orbis_core::)?FeatureId::DisplayRefresh", composition):
            errors.append(
                f"{FILES['composition']}: production registry must not expose DisplayRefresh before owner validation"
            )

    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(argv)
    errors = run(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"display-refresh-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("display-refresh-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
