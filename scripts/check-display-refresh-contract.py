#!/usr/bin/env python3
"""Static fail-closed checks for the developing DisplayRefresh backend contract.

This checker does not prove a compositor implementation. It protects the current
boundary: `wl_output` is observation-only, mutation target identity is owned by a
future compositor adapter, validated requests cannot be forged from output names,
and production UI/Automation remain write-disabled until that owner is proven.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

FILES = {
    "core": "crates/orbis-core/src/display_refresh.rs",
    "request": "crates/orbis-core/src/display_refresh_request.rs",
    "output": "crates/orbis-core/src/display_output.rs",
    "provider": "crates/orbis-providers/src/wayland_output.rs",
    "owner": "crates/orbis-providers/src/display_refresh_owner.rs",
    "preflight": "crates/orbis-config/src/automation_preflight.rs",
    "quick": "crates/orbis-ui/src/quick_controls_backend.rs",
    "composition": "crates/orbis-ui/src/composition.rs",
}

SHELL_FALLBACKS = (
    "std::process::Command",
    "Command::new(",
    "wlr-randr",
    "kscreen-doctor",
    "xrandr",
    "/bin/sh",
)


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
                "closest_refresh",
            ),
            errors,
        )
        if re.search(r"use\s+crate::display_output::\{?[^;]*DisplayOutputId", core):
            errors.append(
                f"{FILES['core']}: compositor wl_output identity must not become mutation target identity"
            )

    request = sources.get("request")
    if request is not None:
        require(
            request,
            FILES["request"],
            (
                "pub struct DisplayRefreshRequest",
                "target: DisplayRefreshTargetId",
                "preset_target: DisplayRefreshPresetTarget",
                "pub fn from_constraints",
                "constraints.writable_target_for(preset)?",
            ),
            errors,
        )
        if re.search(r"pub\s+target\s*:\s*DisplayRefreshTargetId", request):
            errors.append(f"{FILES['request']}: request target field must remain private")
        if re.search(r"pub\s+preset_target\s*:\s*DisplayRefreshPresetTarget", request):
            errors.append(f"{FILES['request']}: request preset target field must remain private")
        if "DisplayOutputId" in request:
            errors.append(f"{FILES['request']}: wl_output identity must not enter mutation request")

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

    owner = sources.get("owner")
    if owner is not None:
        require(
            owner,
            FILES["owner"],
            (
                "pub trait DisplayRefreshMutationOwner",
                "display_refresh_evidence",
                "set_display_refresh",
                "validate_display_refresh_request",
                "DisplayRefreshTargetRole::InternalPanelProven",
                "fresh.writable_target_for(request.preset())",
            ),
            errors,
        )
        for token in SHELL_FALLBACKS:
            if token in owner:
                errors.append(
                    f"{FILES['owner']}: typed owner contract contains shell/process fallback {token!r}"
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

    # No concrete implementation may enter production while this contract says
    # the feature is write-disabled. Test fixtures should stay local to tests;
    # the current tree intentionally has no `impl DisplayRefreshMutationOwner`.
    for path in (root / "crates").rglob("*.rs"):
        if path.name == "display_refresh_owner.rs":
            continue
        try:
            source = path.read_text(encoding="utf-8")
        except OSError:
            continue
        if re.search(r"impl(?:<[^>]*>)?\s+DisplayRefreshMutationOwner\s+for\b", source):
            errors.append(
                f"{path.relative_to(root)}: concrete DisplayRefresh owner appeared before promotion gate update"
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
