#!/usr/bin/env python3
"""Objective page identification: match each live c5ca shot's TITLE BAND against
every reference render's title band.

The page header (section title + subtitle) is unique per page and is drawn in the
same place on every render, so a near-zero band diff identifies the page even when
the card body differs (different fixture/state).

Title band: y 56..92, x 250..800 (content area, below the 44px title bar).
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

SECTIONS = ["dashboard", "performance", "power", "cooling", "graphics",
            "backlight", "display", "system", "settings", "about"]
REFS = {
    "merged": ROOT / ".int-smoke/merged",
    "base": ROOT / ".int-smoke/base",
    "baseline": ROOT / "docs/ui-audit-baseline",
}

Y0, Y1, X0, X1 = 56, 92, 250, 800


def band(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)[Y0:Y1, X0:X1]


def frac(a, b):
    if a.shape != b.shape:
        return None
    return float((a != b).any(axis=2).mean())


refbands = {}
for label, d in REFS.items():
    for s in SECTIONS:
        f = d / f"{s}-980x680-dark.png"
        if f.exists():
            refbands[(label, s)] = band(f)

print(f"loaded {len(refbands)} reference title bands")
print()
print("=== live c5ca shot -> best-matching reference title band ===")
for p in sorted(QA.glob("c5ca-*.png")):
    with Image.open(p) as im:
        if im.size != (980, 680):
            print(f"  {p.name:34s} ({im.size[0]}x{im.size[1]}) skipped (not 980x680)")
            continue
    b = band(p)
    scores = sorted(
        ((frac(b, rb), f"{lab}/{s}") for (lab, s), rb in refbands.items() if rb.shape == b.shape),
        key=lambda t: t[0],
    )
    best, second = scores[0], scores[1]
    tag = "TITLE-MATCH" if best[0] < 0.02 else ("weak" if best[0] < 0.15 else "NO-MATCH")
    print(f"  {p.name:34s} {tag:11s} best={best[1]:22s} {best[0]:.4f}  2nd={second[1]:22s} {second[0]:.4f}")
