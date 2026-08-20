#!/usr/bin/env python3
"""Static production-UI contract checks for Orbis Control.

This intentionally uses only the Python standard library so it can run in
minimal containers even when Rust/Slint/Nix are unavailable. It is not a
replacement for compiling Slint; it catches the regressions that previously
made the UI look functional while only mutating local preview state.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

PRODUCTION_SURFACES = {
    "ui/audited/common.slint": [
        "WidgetPalette",
        "color-scheme",
        "changed observed-mode",
    ],
    "ui/audited/main-window.slint": [
        "display-state-ready",
        "display-control-ready",
        "display-mode-requested",
        "keyboard-state-ready",
        "keyboard-control-ready",
        "keyboard-brightness-requested",
        "preferences-clicked",
        "diagnostics-clicked",
    ],
    "ui/audited/preferences-window.slint": [
        "ThemeBridge {",
        "RequestToggleRow",
        "startup-changed",
        "start-minimized-changed",
        "remember-position-changed",
        "close-action-changed",
    ],
    "ui/audited/automation-window.slint": [
        "ThemeBridge {",
        "RequestToggleRow",
        "backend-ready",
        "save-requested",
        "reset-requested",
    ],
    "ui/audited/extra-window.slint": [
        "ThemeBridge {",
        "backend-ready",
        "reload-requested",
        "apply-requested",
    ],
    "ui/audited/diagnostics-window.slint": [
        "refresh-requested",
        "copy-summary-requested",
        "open-logs-requested",
        "export-report-requested",
    ],
    "ui/audited/updates-window.slint": [
        "ThemeBridge {",
        "backend-ready",
        "check-requested",
        "install-requested",
        "channel-requested",
    ],
    "ui/audited/fans-window.slint": [
        "ThemeBridge {",
        "mutation-safety-blocked",
        "curve-enabled-known",
        "fan-apply-clicked",
    ],
    "ui/audited/preview-dialog-window.slint": [
        "confirm-clicked",
        "dismiss-clicked",
    ],
    "ui/components/request-toggle-row.slint": [
        "toggled(!root.checked)",
    ],
}

FORBIDDEN_PRODUCT_PHRASES = (
    "visual-only",
    "preview only",
    "local preview",
    "simulated locally",
    "staged locally",
    "applied locally",
    "saved locally",
    "previewed",
)

THEME_TOKENS = {
    "window-background",
    "titlebar-background",
    "surface-default",
    "surface-hover",
    "surface-pressed",
    "surface-selected",
    "surface-disabled",
    "border-default",
    "border-strong",
    "text-primary",
    "text-secondary",
    "text-disabled",
    "text-on-accent",
    "accent-default",
    "warning",
    "error",
    "success",
    "silent",
    "balanced",
    "turbo",
    "eco",
    "standard",
    "ultimate",
    "optimized",
}

SECONDARY_MIN_HEIGHT = {
    "ui/audited/preferences-window.slint": 450,
    "ui/audited/automation-window.slint": 530,
    "ui/audited/extra-window.slint": 620,
    "ui/audited/diagnostics-window.slint": 560,
    "ui/audited/fans-window.slint": 540,
    "ui/audited/updates-window.slint": 340,
    "ui/audited/preview-dialog-window.slint": 210,
}


def fail(errors: list[str], message: str) -> None:
    errors.append(message)


def read(path: Path, errors: list[str]) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        fail(errors, f"{path}: cannot read: {exc}")
        return ""


def strip_strings_and_comments(text: str) -> str:
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        if text.startswith("//", i):
            j = text.find("\n", i)
            if j < 0:
                break
            out.append("\n")
            i = j + 1
            continue
        if text.startswith("/*", i):
            j = text.find("*/", i + 2)
            if j < 0:
                out.append(" " * (n - i))
                break
            out.append(" " * (j + 2 - i))
            i = j + 2
            continue
        if text[i] == '"':
            out.append('"')
            i += 1
            while i < n:
                if text[i] == "\\":
                    out.append("  ")
                    i += 2
                    continue
                if text[i] == '"':
                    out.append('"')
                    i += 1
                    break
                out.append("\n" if text[i] == "\n" else " ")
                i += 1
            continue
        out.append(text[i])
        i += 1
    return "".join(out)


def check_delimiters(path: Path, text: str, errors: list[str]) -> None:
    clean = strip_strings_and_comments(text)
    pairs = {"}": "{", ")": "(", "]": "["}
    stack: list[tuple[str, int]] = []
    line = 1
    for ch in clean:
        if ch == "\n":
            line += 1
        elif ch in "{([":
            stack.append((ch, line))
        elif ch in "})]":
            if not stack or stack[-1][0] != pairs[ch]:
                fail(errors, f"{path}:{line}: unmatched {ch}")
                return
            stack.pop()
    if stack:
        ch, at = stack[-1]
        fail(errors, f"{path}:{at}: unmatched {ch}")


def check_imports(root: Path, path: Path, text: str, errors: list[str]) -> None:
    for match in re.finditer(r'from\s+"([^"]+)"', text):
        target = match.group(1)
        if target == "std-widgets.slint":
            continue
        resolved = (path.parent / target).resolve()
        try:
            resolved.relative_to(root.resolve())
        except ValueError:
            fail(errors, f"{path}: import escapes repository: {target}")
            continue
        if not resolved.is_file():
            fail(errors, f"{path}: unresolved import: {target}")


def has_identifier(text: str, marker: str) -> bool:
    if " " in marker or "(" in marker:
        return marker in text
    pattern = rf"(?<![A-Za-z0-9_-]){re.escape(marker)}(?![A-Za-z0-9_-])"
    return re.search(pattern, text) is not None


def check_surface_markers(root: Path, errors: list[str]) -> None:
    for relative, markers in PRODUCTION_SURFACES.items():
        path = root / relative
        text = read(path, errors)
        for marker in markers:
            if not has_identifier(text, marker):
                fail(errors, f"{relative}: missing production marker {marker!r}")


def check_forbidden_phrases(root: Path, errors: list[str]) -> None:
    for path in sorted((root / "ui").rglob("*.slint")):
        lowered = read(path, errors).lower()
        for phrase in FORBIDDEN_PRODUCT_PHRASES:
            if phrase in lowered:
                fail(errors, f"{path.relative_to(root)}: forbidden product phrase {phrase!r}")


def check_request_only_controls(root: Path, errors: list[str]) -> None:
    main = read(root / "ui/audited/main-window.slint", errors)
    required = [
        "selected: root.display-state-ready && root.display-mode == 0",
        "selected: root.display-state-ready && root.display-mode == 1",
        "selected: root.display-state-ready && root.display-mode == 2",
        "disabled: !root.display-control-ready",
        "selected: root.keyboard-state-ready && root.keyboard-brightness == 0",
        "selected: root.keyboard-state-ready && root.keyboard-brightness == 1",
        "selected: root.keyboard-state-ready && root.keyboard-brightness == 2",
        "selected: root.keyboard-state-ready && root.keyboard-brightness == 3",
        "disabled: !root.keyboard-control-ready",
    ]
    for marker in required:
        if marker not in main:
            fail(errors, f"ui/audited/main-window.slint: missing read/write readiness contract {marker!r}")

    prefs = read(root / "ui/audited/preferences-window.slint", errors)
    automation = read(root / "ui/audited/automation-window.slint", errors)
    request_toggle = read(root / "ui/components/request-toggle-row.slint", errors)
    if "root.checked =" in request_toggle:
        fail(errors, "ui/components/request-toggle-row.slint: request-only toggle mutates checked locally")
    if "startup-changed(!root.startup)" not in prefs:
        fail(errors, "ui/audited/preferences-window.slint: startup toggle must emit requested inverse state")
    if "start-minimized-changed(!root.start-minimized)" not in prefs:
        fail(errors, "ui/audited/preferences-window.slint: start-minimized toggle must emit request only")
    if "enabled-requested(!root.enabled)" not in automation:
        fail(errors, "ui/audited/automation-window.slint: automation enabled toggle must emit request only")


def parse_theme(path: Path, errors: list[str]) -> dict[str, str]:
    text = read(path, errors)
    return {
        name: value
        for name, value in re.findall(
            r"out\s+property\s+<color>\s+([a-z0-9-]+)\s*:\s*(#[0-9A-Fa-f]{6})",
            text,
        )
    }


def srgb_channel(value: int) -> float:
    normalized = value / 255.0
    return normalized / 12.92 if normalized <= 0.04045 else ((normalized + 0.055) / 1.055) ** 2.4


def luminance(hex_color: str) -> float:
    raw = hex_color.lstrip("#")
    r, g, b = (int(raw[i : i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * srgb_channel(r) + 0.7152 * srgb_channel(g) + 0.0722 * srgb_channel(b)


def contrast(a: str, b: str) -> float:
    la, lb = luminance(a), luminance(b)
    light, dark = max(la, lb), min(la, lb)
    return (light + 0.05) / (dark + 0.05)


def check_themes(root: Path, errors: list[str]) -> None:
    dark = parse_theme(root / "ui/themes/dark.slint", errors)
    light = parse_theme(root / "ui/themes/light.slint", errors)
    for label, theme in (("dark", dark), ("light", light)):
        missing = sorted(THEME_TOKENS - theme.keys())
        extra = sorted(theme.keys() - THEME_TOKENS)
        if missing:
            fail(errors, f"ui/themes/{label}.slint: missing theme tokens: {', '.join(missing)}")
        if extra:
            fail(errors, f"ui/themes/{label}.slint: unexpected theme tokens: {', '.join(extra)}")
        if not missing:
            for token in ("text-primary", "text-secondary"):
                ratio = contrast(theme[token], theme["window-background"])
                if ratio < 4.5:
                    fail(
                        errors,
                        f"ui/themes/{label}.slint: {token} contrast {ratio:.2f}:1 < 4.5:1",
                    )
    if dark.keys() != light.keys():
        fail(errors, "theme token parity mismatch between dark and light")


def extract_px_property(text: str, property_name: str) -> int | None:
    match = re.search(rf"\b{re.escape(property_name)}\s*:\s*(\d+)px\s*;", text)
    return int(match.group(1)) if match else None


def check_geometry(root: Path, errors: list[str]) -> None:
    main_path = root / "ui/audited/main-window.slint"
    main = read(main_path, errors)
    width = extract_px_property(main, "width")
    height = extract_px_property(main, "height")
    if width is None or not 400 <= width <= 500:
        fail(errors, f"ui/audited/main-window.slint: width must be 400..500px, got {width}")
    if height is None or not 400 <= height <= 600:
        fail(errors, f"ui/audited/main-window.slint: height must be 400..600px, got {height}")

    for relative, minimum in SECONDARY_MIN_HEIGHT.items():
        text = read(root / relative, errors)
        height = extract_px_property(text, "height")
        if height is None or height < minimum:
            fail(errors, f"{relative}: height must be >= {minimum}px, got {height}")


def check_theme_bridge(root: Path, errors: list[str]) -> None:
    common = read(root / "ui/audited/common.slint", errors)
    for marker in (
        "export component ThemeBridge",
        "WidgetPalette.color-scheme",
        "init => { root.sync-widget-theme(); }",
        "changed observed-mode => { root.sync-widget-theme(); }",
    ):
        if marker not in common:
            fail(errors, f"ui/audited/common.slint: incomplete ThemeBridge contract: {marker!r}")

    for relative in (
        "ui/audited/preferences-window.slint",
        "ui/audited/automation-window.slint",
        "ui/audited/extra-window.slint",
        "ui/audited/updates-window.slint",
        "ui/audited/fans-window.slint",
    ):
        text = read(root / relative, errors)
        if "ThemeBridge {" not in text:
            fail(errors, f"{relative}: missing instantiated ThemeBridge")


def check_widget_style(root: Path, errors: list[str]) -> None:
    build = read(root / "crates/orbis-ui/build.rs", errors)
    if 'with_style("fluent".into())' not in build:
        fail(errors, 'crates/orbis-ui/build.rs: standard widget style must stay pinned to "fluent"')


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    errors: list[str] = []

    for path in sorted((root / "ui").rglob("*.slint")):
        text = read(path, errors)
        check_delimiters(path, text, errors)
        check_imports(root, path, text, errors)

    check_surface_markers(root, errors)
    check_forbidden_phrases(root, errors)
    check_request_only_controls(root, errors)
    check_themes(root, errors)
    check_geometry(root, errors)
    check_theme_bridge(root, errors)
    check_widget_style(root, errors)

    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1

    print("UI contract checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
