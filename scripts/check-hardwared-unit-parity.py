#!/usr/bin/env python3
"""Static contract: standalone hardwared unit matches the NixOS module sandbox.

#126 requires the standalone deployment path (packaging/orbis-hardwared.service,
installed by packaging/deploy-dev-hardwared.sh) to carry the identical effective
sandbox and write surface as the NixOS-module unit asserted by the VM checks.
These are two independent definitions of the same security boundary, so drift
between them must fail loudly at source level instead of silently weakening the
standalone deployment.

This check is fail-fast source inspection only. It does not execute systemd,
prove a booted unit's effective properties (the VM checks do that for the
module path), or replace live standalone-host validation.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

UNIT_FILE = "packaging/orbis-hardwared.service"
MODULE_FILE = "packaging/nix/module.nix"

# Sandbox/write-surface directives that must stay identical in both units.
WATCHED_KEYS = (
    "Type",
    "BusName",
    "Restart",
    "RestartSec",
    "NoNewPrivileges",
    "ProtectSystem",
    "ProtectHome",
    "PrivateTmp",
    "PrivateDevices",
    "ProtectControlGroups",
    "RestrictAddressFamilies",
    "MemoryDenyWriteExecute",
    "AmbientCapabilities",
    "CapabilityBoundingSet",
    "ReadOnlyPaths",
    "ReadWritePaths",
)


def read(root: Path, relative: str) -> str:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"{relative}: cannot read: {error}")


def parse_unit(root: Path) -> dict[str, list[str]]:
    """Parse [Service] key=value lines from the systemd unit template."""
    service = re.search(r"^\[Service\]\n(.*?)(?=^\[|\Z)", read(root, UNIT_FILE), re.S | re.M)
    if not service:
        raise SystemExit(f"{UNIT_FILE}: no [Service] section found")
    values: dict[str, list[str]] = {}
    for line in service.group(1).splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith(("#", "[")):
            continue
        key, sep, raw = stripped.partition("=")
        if not sep:
            raise SystemExit(f"{UNIT_FILE}: unparsable directive {stripped!r}")
        # systemd repeats list-valued directives by repeating assignments.
        values.setdefault(key.strip(), []).extend(
            item.strip() for item in raw.split() if item.strip()
        )
    return values


def parse_module_service_config(root: Path) -> dict[str, list[str]]:
    """Extract the orbis-hardwared serviceConfig mapping from module.nix."""
    text = read(root, MODULE_FILE)
    anchor = text.find("systemd.services.orbis-hardwared")
    if anchor == -1:
        raise SystemExit(f"{MODULE_FILE}: orbis-hardwared service not found")
    scope_end = text.find("environment.systemPackages", anchor)
    if scope_end == -1:
        raise SystemExit(f"{MODULE_FILE}: cannot bound the hardwared service block")
    block = text[anchor:scope_end]

    config_match = re.search(r"serviceConfig\s*=\s*\{(.*?)\n\s*\};", block, re.S)
    if not config_match:
        raise SystemExit(f"{MODULE_FILE}: serviceConfig block not parsed")

    values: dict[str, list[str]] = {}
    # Collapse multi-line Nix lists onto one line so every directive parses
    # as `key = value;`.
    collapsed = re.sub(
        r"=\s*\[([^]]*)\]",
        lambda match: "= [" + " ".join(match.group(1).split()) + "]",
        config_match.group(1),
    )
    for raw_line in collapsed.splitlines():
        pair = re.match(r"\s*(\w+)\s*=\s*(.+?);\s*$", raw_line)
        if not pair:
            continue
        key, raw_value = pair.group(1), pair.group(2).strip()
        if key not in WATCHED_KEYS:
            continue
        if raw_value.startswith("[") and raw_value.endswith("]"):
            inner = raw_value[1:-1].strip()
            items = re.findall(r'"([^"]*)"', inner) or (inner.split() if inner else [])
        else:
            quoted = re.fullmatch(r'"([^"]*)"', raw_value)
            items = [quoted.group(1)] if quoted else [raw_value]
        items = [item for item in items if item != ""]
        values.setdefault(key, []).extend(items)
    return values


def normalize(values: dict[str, list[str]]) -> dict[str, list[str]]:
    return {
        key: sorted(item for item in entries if item)
        for key, entries in values.items()
        if key in WATCHED_KEYS
    }


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    unit = normalize(parse_unit(root))
    module = normalize(parse_module_service_config(root))

    failures: list[str] = []
    for key in WATCHED_KEYS:
        unit_value = unit.get(key)
        module_value = module.get(key)
        if unit_value is None and module_value is None:
            failures.append(f"{key}: missing from BOTH definitions")
        elif unit_value is None or module_value is None:
            failures.append(
                f"{key}: present only in {'unit' if module_value is None else 'module'} "
                f"(unit={unit.get(key)!r} module={module.get(key)!r})"
            )
        elif sorted(unit_value) != sorted(module_value):
            failures.append(
                f"{key}: unit={sorted(unit_value)!r} differs from module={sorted(module_value)!r}"
            )

    if failures:
        print("hardwared-unit-parity: FAIL", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1

    print("hardwared-unit-parity: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
