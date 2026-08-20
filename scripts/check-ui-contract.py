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
        "display-mode-requested",
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


def rgb(hex_color: str) -> tuple[float, float, float]:
    h = hex_color.lstrip("#")
    return tuple(int(h[i : i + 2], 16) / 255.0 for i in (0, 2, 4))


def luminance(hex_color: str) -> float:
    values = []
    for c in rgb(hex_color):
        values.append(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4)
    return 0.2126 * values[0] + 0.7152 * values[1] + 0.0722 * values[2]


def contrast(a: str, b: str) -> float:
    l1, l2 = sorted((luminance(a), luminance(b)), reverse=True)
    return (l1 + 0.05) / (l2 + 0.05)


def theme_values(text: str, dark: bool) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in THEME_TOKENS:
        if dark:
            pattern = rf"property\s+<color>\s+{re.escape(token)}:.*?:\s*(#[0-9A-Fa-f]{{6}})\s*;"
        else:
            pattern = rf"property\s+<color>\s+{re.escape(token)}:\s*(#[0-9A-Fa-f]{{6}})\s*;"
        match = re.search(pattern, text)
        if match:
            values[token] = match.group(1)
    return values


def check_themes(root: Path, errors: list[str]) -> None:
    dark_path = root / "ui/themes/dark.slint"
    light_path = root / "ui/themes/light.slint"
    dark_text = read(dark_path, errors)
    light_text = read(light_path, errors)
    dark = theme_values(dark_text, dark=True)
    light = theme_values(light_text, dark=False)

    for name, values in (("dark", dark), ("light", light)):
        missing = sorted(THEME_TOKENS - values.keys())
        if missing:
            fail(errors, f"{name} theme missing tokens: {', '.join(missing)}")
            continue
        for text_token in ("text-primary", "text-secondary"):
            ratio = contrast(values[text_token], values["window-background"])
            if ratio < 4.5:
                fail(errors, f"{name} {text_token} contrast {ratio:.2f}:1 < 4.5:1")


def declared_geometry(text: str) -> tuple[int, int] | None:
    width = re.search(r"\bwidth:\s*(\d+)px", text)
    height = re.search(r"\bheight:\s*(\d+)px", text)
    if not width or not height:
        return None
    return int(width.group(1)), int(height.group(1))


def check_main_geometry(text: str, errors: list[str]) -> None:
    geometry = declared_geometry(text)
    if geometry is None:
        fail(errors, "main window must declare fixed compact width/height")
        return
    w, h = geometry
    if w > 500 or h > 600:
        fail(errors, f"main window too large for compact contract: {w}x{h}")
    if w < 400 or h < 400:
        fail(errors, f"main window too small for planned production controls: {w}x{h}")


def check_secondary_geometry(root: Path, errors: list[str]) -> None:
    for rel, minimum_height in SECONDARY_MIN_HEIGHT.items():
        text = read(root / rel, errors)
        geometry = declared_geometry(text)
        if geometry is None:
            fail(errors, f"{rel}: must declare fixed width/height")
            continue
        w, h = geometry
        if h < minimum_height:
            fail(errors, f"{rel}: height {h}px below reviewed content budget {minimum_height}px")
        if w > 760 or h > 760:
            fail(errors, f"{rel}: window too large for compact secondary-surface contract: {w}x{h}")


def check_authoritative_controls(root: Path, errors: list[str]) -> None:
    for rel in (
        "ui/audited/preferences-window.slint",
        "ui/audited/automation-window.slint",
    ):
        text = read(root / rel, errors)
        if re.search(r"\bToggleRow\s*\{", text):
            fail(errors, f"{rel}: backend-owned toggles must use RequestToggleRow")


def check_widget_style(root: Path, errors: list[str]) -> None:
    build = read(root / "crates/orbis-ui/build.rs", errors)
    if '.with_style("fluent".into())' not in build:
        fail(errors, "crates/orbis-ui/build.rs: standard widget style must stay pinned to fluent")


def run(root: Path) -> list[str]:
    errors: list[str] = []
    ui_root = root / "ui"
    if not ui_root.is_dir():
        return [f"{ui_root}: missing UI directory"]

    for path in sorted(ui_root.rglob("*.slint")):
        text = read(path, errors)
        check_delimiters(path, text, errors)
        check_imports(root, path, text, errors)

    for rel, required in PRODUCTION_SURFACES.items():
        path = root / rel
        text = read(path, errors)
        lowered = text.lower()
        for phrase in FORBIDDEN_PRODUCT_PHRASES:
            if phrase in lowered:
                fail(errors, f"{rel}: forbidden fake-success/preview phrase: {phrase!r}")
        for marker in required:
            if not has_identifier(text, marker):
                fail(errors, f"{rel}: missing frontend contract marker {marker!r}")

    main = read(root / "ui/audited/main-window.slint", errors)
    check_main_geometry(main, errors)
    check_secondary_geometry(root, errors)
    check_authoritative_controls(root, errors)
    check_widget_style(root, errors)
    check_themes(root, errors)
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", help="repository root")
    args = parser.parse_args()
    errors = run(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"UI CONTRACT FAIL: {error}", file=sys.stderr)
        return 1
    print("UI contract checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
