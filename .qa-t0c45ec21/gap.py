#!/usr/bin/env python3
"""Decisive F2 (D-AUD-G02) probe.

Measure the vertical gap between the "Режим производительности" profile card and
the "Лимиты мощности" card on the Performance page, across the live c5ca set and
the reference renders. Pre-fix (defect present) => ~172px hole; post-fix => 16px.

Method: in the content column, find the background-coloured horizontal bands
(>=8px tall) that span the full content width. A card is a non-background band.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

CONTENT_X0, CONTENT_X1 = 232, 980
Y0, Y1 = 50, 660
BG_TOL = 14
MIN_BAND = 8


def bands(path):
    a = np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)
    bg = a[660, 500]
    region = a[Y0:Y1, CONTENT_X0:CONTENT_X1]
    nonbg = (np.abs(region - bg).sum(axis=2) > BG_TOL).sum(axis=1)
    rows = nonbg > 20
    out = []
    start = None
    for i, r in enumerate(rows):
        if r and start is None:
            start = i
        elif not r and start is not None:
            if i - start >= MIN_BAND:
                out.append((start + Y0, i - 1 + Y0))
            start = None
    if start is not None:
        out.append((start + Y0, len(rows) - 1 + Y0))
    return out


TARGETS = [
    ("LIVE c5ca-performance-reselect", QA / "c5ca-performance-reselect.png"),
    ("LIVE p980-performance (pre-merge)", QA / "p980-performance.png"),
    ("REF merged/performance-980x680-dark", ROOT / ".int-smoke/merged/performance-980x680-dark.png"),
    ("REF merged/performance-980x680-dark-unsupported", ROOT / ".int-smoke/merged/performance-980x680-dark-unsupported.png"),
    ("REF baseline/performance-980x680-dark (pre-fix)", ROOT / "docs/ui-audit-baseline/performance-980x680-dark.png"),
]

for label, p in TARGETS:
    if not p.exists():
        print(f"-- {label}: MISSING")
        continue
    bs = bands(p)
    print(f"-- {label}")
    prev_end = None
    for (a0, a1) in bs:
        gap = "" if prev_end is None else f"gap_before={a0 - prev_end - 1:4d}px"
        print(f"     card-band y=[{a0:3d},{a1:3d}] height={a1 - a0 + 1:3d}  {gap}")
        prev_end = a1
    print()
