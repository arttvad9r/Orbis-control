#!/usr/bin/env python3
"""Static source-of-truth checks for current Orbis documentation.

The checker prevents known stale status claims from reappearing while executable
Rust/Nix CI is unavailable. It validates documentation/repository consistency
only; it does not prove runtime behavior.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

FILES = {
    "root": "README.md",
    "index": "docs/README.md",
    "state": "docs/current-state.md",
    "architecture": "docs/architecture.md",
    "backend": "docs/backend-completion-status.md",
    "roadmap": "docs/roadmap.md",
    "beta": "docs/beta-acceptance-checklist.md",
}

REQUIRED = {
    "root": (
        "chatgpt/ui-refresh-ghelper-20260820",
        "worker_runtime.rs",
        "status --json",
        "scripts/verify-static",
        "#125",
        "#123",
    ),
    "index": (
        "Current branch vs release baseline",
        "backend-completion-status.md",
        "beta-acceptance-checklist.md",
        "Source inspection proves at most `IMPLEMENTED`",
    ),
    "state": (
        "Source snapshot: `chatgpt/ui-refresh-ghelper-20260820`",
        "#110 closed",
        "#111 closed",
        "#121 closed",
        "bounded_provider_call",
        "aggregate capability still CPU-centric",
        "Effective product-policy truth (#120 complete)",
        "Draft integration PR: #129",
    ),
    "architecture": (
        "worker_runtime.rs",
        "Explicit and periodic refresh now use the same canonical path",
        "GPU primitive reads run concurrently",
        "Autostart read/write/read-back (#110 closed)",
        "DiagnosticsRuntime",
    ),
    "backend": (
        "Provider execution hardening",
        "status --json",
        "#110 source-complete/closed",
        "#111 closed",
        "#121 closed",
        "tokio::join!",
        "Effective product write truth (#120 complete)",
    ),
    "roadmap": (
        "Autostart (#110 closed)",
        "Diagnostics Refresh/Export/Copy (#111 closed",
        "#121 closed",
        "#112 is source-complete",
        "#120 closed",
        "#125 GUI root boundary",
        "Draft PR #129",
    ),
    "beta": (
        "Current checklist for the active integration branch",
        "Draft PR #129",
        "#106",
        "#120:",
        "#125",
        "#109",
        "#115",
    ),
}

FORBIDDEN = {
    "state": (
        "Run on Startup | PARTIAL",
        "Diagnostics | TESTED FOUNDATION / FAIL-CLOSED UI",
        "Window lifecycle | PARTIAL",
        "Lifecycle wiring: #110, #111, #121",
        "#120 consumer/support-matrix policy truth",
    ),
    "architecture": (
        "its UI remains disabled until #110 is completed",
        "lifecycle/refresh wiring is completed (#111)",
        "does not yet enforce it generically (#123)",
        "#112 records the current stale-status defect",
    ),
    "roadmap": (
        "#110 — finish XDG Run on Startup lifecycle wiring",
        "#111 — finish Diagnostics open/refresh lifecycle wiring",
        "#121 — finish window-state, close and tray semantics",
        "#112 — make explicit capability refresh",
        "#120 effective policy truth",
    ),
    "beta": (
        "PR #19",
        "PR #21",
        "PR #23",
        "Draft, not merged",
        "stacked on #19",
        "[ ] #120:",
    ),
}


def read(root: Path, relative: str, errors: list[str]) -> str:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: cannot read: {error}")
        return ""


def run(root: Path) -> list[str]:
    errors: list[str] = []
    sources = {key: read(root, path, errors) for key, path in FILES.items()}

    for key, markers in REQUIRED.items():
        source = sources[key]
        for marker in markers:
            if marker not in source:
                errors.append(f"{FILES[key]}: missing current-status marker {marker!r}")

    for key, markers in FORBIDDEN.items():
        source = sources[key]
        for marker in markers:
            if marker in source:
                errors.append(f"{FILES[key]}: stale status marker present {marker!r}")

    obsolete_trigger = root / ".github/keyboard-backlight-probe-validation-trigger"
    if obsolete_trigger.exists():
        errors.append(
            f"{obsolete_trigger.relative_to(root)}: obsolete one-off validation trigger must stay removed"
        )

    workflows = root / ".github/workflows"
    if workflows.is_dir():
        workflow_files = sorted(path.name for path in workflows.iterdir() if path.is_file())
        if workflow_files != ["ci.yml"]:
            errors.append(
                ".github/workflows: expected only canonical ci.yml, found "
                + ", ".join(workflow_files)
            )

    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(argv)
    errors = run(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"docs-status-contract: FAIL: {error}", file=sys.stderr)
        return 1
    print("docs-status-contract: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
