#!/usr/bin/env python3
"""Static fail-closed checks for the developing DisplayRefresh backend contract.

This checker does not prove a compositor implementation. It protects the current
boundary: `wl_output` is observation-only; `zwlr_output_manager_v1` discovery is
transport evidence only; head identity/modes are hardware-inert evidence; target
role requires independent typed DRM/KMS correlation; validated requests cannot be
forged from output names; and post-write success requires authoritative active-
policy read-back. Production UI/Automation remain write-disabled until a concrete
owner is executable-validated.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

FILES = {
    "core": "crates/orbis-core/src/display_refresh.rs",
    "identity": "crates/orbis-core/src/display_refresh_identity.rs",
    "request": "crates/orbis-core/src/display_refresh_request.rs",
    "state": "crates/orbis-core/src/display_refresh_state.rs",
    "output": "crates/orbis-core/src/display_output.rs",
    "core_lib": "crates/orbis-core/src/lib.rs",
    "provider": "crates/orbis-providers/src/wayland_output.rs",
    "transport": "crates/orbis-providers/src/wayland_output_management.rs",
    "head": "crates/orbis-providers/src/wlr_output_head.rs",
    "provider_lib": "crates/orbis-providers/src/lib.rs",
    "owner": "crates/orbis-providers/src/display_refresh_owner.rs",
    "service": "crates/orbis-ui/src/display_refresh_service.rs",
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

TRANSPORT_MUTATION_TOKENS = (
    "create_configuration(",
    "enable_head(",
    "disable_head(",
    "set_mode(",
    "set_custom_mode(",
    "set_position(",
    "set_transform(",
    "set_scale(",
    "set_adaptive_sync(",
    ".apply(",
    ".test(",
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


def production_prefix(source: str) -> str:
    return source.split("#[cfg(test)]", 1)[0]


def run(root: Path) -> list[str]:
    errors: list[str] = []
    sources: dict[str, str] = {}
    for key, relative in FILES.items():
        source = read(root, relative, errors)
        if source is not None:
            sources[key] = source

    core = sources.get("core")
    if core is not None:
        require(core, FILES["core"], (
            "DisplayRefreshConstraints", "DisplayRefreshEvidence", "DisplayRefreshTargetId",
            "InternalPanelProven", "writable_target_for", "same hardware width/height",
            "auto_supported", "closest_refresh",
        ), errors)
        if re.search(r"use\s+crate::display_output::\{?[^;]*DisplayOutputId", core):
            errors.append(f"{FILES['core']}: wl_output identity must not become mutation target identity")

    identity = sources.get("identity")
    if identity is not None:
        require(identity, FILES["identity"], (
            "DisplaySinkIdentity", "DisplayPhysicalSizeMm", "DrmConnectorTypeEvidence",
            "DrmDisplayConnectorEvidence", "CompositorDisplayHeadEvidence",
            "DisplayTargetIdentityProof", "DisplayTargetIdentityBlock",
            "prove_display_target_identity", "AmbiguousDrmConnectorMatch",
            "ConnectorTypeUnclassified", "InternalPanelProven",
            "connector.connected", "connector.sink.as_ref() == Some(sink)",
            "connector.physical_size == Some(physical_size)",
        ), errors)
        production = production_prefix(identity)
        for shortcut in ('"eDP-1"', 'starts_with("eDP', 'contains("eDP'):
            if shortcut in production:
                errors.append(f"{FILES['identity']}: name-based internal-panel shortcut {shortcut!r}")
        if "DisplayOutputId" in production:
            errors.append(f"{FILES['identity']}: wl_output identity must not enter role proof")

    request = sources.get("request")
    if request is not None:
        require(request, FILES["request"], (
            "pub struct DisplayRefreshRequest", "target: DisplayRefreshTargetId",
            "preset_target: DisplayRefreshPresetTarget", "pub fn from_constraints",
            "constraints.writable_target_for(preset)?",
        ), errors)
        if re.search(r"pub\s+target\s*:\s*DisplayRefreshTargetId", request):
            errors.append(f"{FILES['request']}: request target field must remain private")
        if re.search(r"pub\s+preset_target\s*:\s*DisplayRefreshPresetTarget", request):
            errors.append(f"{FILES['request']}: request preset target field must remain private")
        if "DisplayOutputId" in request:
            errors.append(f"{FILES['request']}: wl_output identity must not enter mutation request")

    state = sources.get("state")
    if state is not None:
        require(state, FILES["state"], (
            "DisplayRefreshActivePolicy", "Auto", "Fixed", "Unknown",
            "DisplayRefreshAppliedState", "internal_panel_is_proven",
        ), errors)
        if "`Unknown` never confirms mutation" not in state:
            errors.append(f"{FILES['state']}: Unknown active policy must be non-confirming")

    output = sources.get("output")
    if output is not None:
        require(output, FILES["output"], (
            "do not assume that the name is a reflection of an underlying",
            "Non-current modes are deprecated",
        ), errors)

    core_lib = sources.get("core_lib")
    if core_lib is not None:
        for marker in (
            "pub mod display_refresh_identity;",
            "prove_display_target_identity",
            "DrmConnectorTypeEvidence",
        ):
            if marker not in core_lib:
                errors.append(f"{FILES['core_lib']}: missing identity export {marker!r}")

    provider = sources.get("provider")
    if provider is not None:
        require(provider, FILES["provider"], (
            "Read-only Wayland display output provider", "никакого modeset/configuration API",
            "impl<S> DisplayOutputProvider",
        ), errors)
        production = production_prefix(provider)
        for token in ("set_mode(", "set_refresh(", "apply_mode(", "wlr_output_manager"):
            if token in production:
                errors.append(f"{FILES['provider']}: unexpected mutation token {token!r}")

    transport = sources.get("transport")
    if transport is not None:
        require(transport, FILES["transport"], (
            "WLR_OUTPUT_MANAGER_INTERFACE", '"zwlr_output_manager_v1"',
            "WLR_OUTPUT_MANAGER_CLIENT_MAX_VERSION", "WaylandRegistryGlobal",
            "WlrOutputManagementSupport", "classify_wlr_output_management_globals",
            "WaylandWlrOutputManagementSource", "registry_queue_init", "GlobalListContents",
            "compatible_version", "ProviderError::Unsupported",
        ), errors)
        production = production_prefix(transport)
        for token in TRANSPORT_MUTATION_TOKENS + SHELL_FALLBACKS:
            if token in production:
                errors.append(f"{FILES['transport']}: transport probe crossed boundary via {token!r}")
        if "DisplayRefreshMutationOwner" in production:
            errors.append(f"{FILES['transport']}: transport probe must not claim owner authority")
        if "globals.bind" in production or ".bind::<" in production:
            errors.append(f"{FILES['transport']}: transport probe must not bind configuration objects")

    head = sources.get("head")
    if head is not None:
        require(head, FILES["head"], (
            "WlrOutputHeadState", "WlrOutputHeadEvent", "WlrOutputModeKey",
            "CompositorDisplayHeadEvidence", "DisplaySinkIdentity",
            "identity_evidence", "refresh_evidence", "CurrentMode",
            "ModeRemoved", "auto_supported", "preferred_modes",
        ), errors)
        production = production_prefix(head)
        for token in TRANSPORT_MUTATION_TOKENS + SHELL_FALLBACKS:
            if token in production:
                errors.append(f"{FILES['head']}: pure head evidence crossed mutation/process boundary via {token!r}")
        if "wayland_protocols_wlr" in production:
            errors.append(f"{FILES['head']}: pure head model must not depend on generated protocol binding")
        if re.search(r"name[^\n]{0,120}InternalPanelProven", production):
            errors.append(f"{FILES['head']}: head name must not prove internal role")

    provider_lib = sources.get("provider_lib")
    if provider_lib is not None:
        for marker in (
            "pub mod wayland_output_management;", "pub use wayland_output_management::*;",
            "pub mod wlr_output_head;", "pub use wlr_output_head::*;",
        ):
            if marker not in provider_lib:
                errors.append(f"{FILES['provider_lib']}: missing Display evidence export {marker!r}")

    owner = sources.get("owner")
    if owner is not None:
        require(owner, FILES["owner"], (
            "pub trait DisplayRefreshMutationOwner", "display_refresh_evidence",
            "display_refresh_applied_state", "set_display_refresh",
            "request: &DisplayRefreshRequest", "validate_display_refresh_request",
            "validate_display_refresh_readback", "DisplayRefreshTargetRole::InternalPanelProven",
            "fresh.writable_target_for(request.preset())", "DisplayRefreshActivePolicy::Auto",
            "DisplayRefreshActivePolicy::Fixed", "after.current.width != before.current.width",
            "after.current.height != before.current.height",
        ), errors)
        if re.search(r"async\s+fn\s+set_display_refresh\s*\([^\)]*request\s*:\s*DisplayRefreshRequest", owner, re.S):
            errors.append(f"{FILES['owner']}: setter must borrow exact validated request")
        for token in SHELL_FALLBACKS:
            if token in production_prefix(owner):
                errors.append(f"{FILES['owner']}: typed owner contains shell fallback {token!r}")

    service = sources.get("service")
    if service is not None:
        require(service, FILES["service"], (
            "pub struct DisplayRefreshCommandOutcome", "pub enum DisplayRefreshCommandError",
            "PreflightRead", "StaleRequest", "Command(ProviderError)", "ReadBack",
            "ReadBackMismatch", "pub async fn apply_display_refresh",
            "display_refresh_evidence()", "validate_display_refresh_request(request, &before)",
            "set_display_refresh(request)", "display_refresh_applied_state()",
            "validate_display_refresh_readback(request, &before, &state)",
        ), errors)
        production = production_prefix(service)
        for token in SHELL_FALLBACKS:
            if token in production:
                errors.append(f"{FILES['service']}: application orchestration contains fallback {token!r}")
        if "app.set_display_control_ready(true)" in production:
            errors.append(f"{FILES['service']}: service must not directly enable UI writes")

    preflight = sources.get("preflight")
    if preflight is not None:
        require(preflight, FILES["preflight"], (
            "preflight_automation_plan_with_display_constraints",
            "DisplayRefreshTargetRole::InternalPanelProven",
            "DisplayRefreshPolicyNotRepresentable", "TargetIdentityNotProven",
            "preflight_automation_plan_with_display_constraints(plan, capabilities, None)",
        ), errors)

    quick = sources.get("quick")
    if quick is not None:
        if "app.set_display_control_ready(false)" not in quick:
            errors.append(f"{FILES['quick']}: Display Quick Control must remain write-disabled")
        if "production display backend is read-only" not in quick:
            errors.append(f"{FILES['quick']}: missing fail-closed Display request status")

    composition = sources.get("composition")
    if composition is not None and re.search(r"\.add\(\s*(?:orbis_core::)?FeatureId::DisplayRefresh", composition):
        errors.append(f"{FILES['composition']}: production registry must not expose DisplayRefresh from evidence-only layers")

    for path in (root / "crates").rglob("*.rs"):
        if path.name == "display_refresh_owner.rs":
            continue
        try:
            source = path.read_text(encoding="utf-8")
        except OSError:
            continue
        production = production_prefix(source)
        if re.search(r"impl(?:<[^>]*>)?\s+DisplayRefreshMutationOwner\s+for\b", production):
            errors.append(f"{path.relative_to(root)}: concrete DisplayRefresh owner appeared before promotion gate update")

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
