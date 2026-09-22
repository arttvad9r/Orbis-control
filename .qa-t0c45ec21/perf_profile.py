#!/usr/bin/env python3
"""Robust ink-band profile of the Performance page content column.

Page background is sampled from the right gutter (x 962..978), which is always
outside the cards (28px page gutter). Ink = pixels differing from that
background. Contiguous ink rows form bands; gaps between bands are the empty
space between cards. D-AUD-G02 shows up as a gap of ~170px.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")

FILES = [
    ("baseline PRE-fix performance normal      ", R / "docs/ui-audit-baseline/performance-980x680-dark.png"),
    ("baseline PRE-fix performance unsupported ", R / "docs/ui-audit-baseline/performance-980x680-dark-unsupported.png"),
    ("merged CANDIDATE performance normal      ", R / ".int-smoke/merged/performance-980x680-dark.png"),
    ("merged CANDIDATE performance unsupported ", R / ".int-smoke/merged/performance-980x680-dark-unsupported.png"),
    ("LIVE verdict-set c5ca-performance-reselect", QA / "c5ca-performance-reselect.png"),
    ("LIVE verdict-set c5ca-perf (== dashboard)", QA / "c5ca-perf.png"),
    ("CONTROL pre-merge p980-performance       ", QA / "p980-performance.png"),
]


def profile(path, label):
    A = np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)
    h, w, _ = A.shape
    # background: right gutter, sampled per row and median-filtered
    gutter = A[:, w - 18:w - 2, :].reshape(-1, 3)
    bg = np.median(gutter, axis=0)
    # content column: from sidebar edge (x=216) to right gutter (w-18)
    col = A[:, 220:w - 18, :]
    d = np.abs(col - bg).sum(axis=2)
    ink = (d > 14).sum(axis=1)
    rows = ink > 2          # a row counts as inked if >2 px differ from bg
    bands, inb, s = [], False, 0
    for y in range(h):
        if rows[y] and not inb:
            s, inb = y, True
        elif not rows[y] and inb:
            if y - 1 - s >= 2:
                bands.append((s, y - 1))
            inb = False
    if inb and h - 1 - s >= 2:
        bands.append((s, h - 1))
    gaps = [(bands[i + 1][0] - bands[i][1] - 1, bands[i][1] + 1, bands[i + 1][0] - 1)
            for i in range(len(bands) - 1)]
    print(f"  {label}  ({path.name})")
    print(f"     ink bands: {bands}")
    big = [g for g in gaps if g[0] >= 100]
    print(f"     PHANTOM GAPS >=100px: {big if big else 'none'}")
    print(f"     all gaps: {[g[0] for g in gaps]}")


for label, p in FILES:
    if p.exists():
        profile(p, label)
    else:
        print(f"  {label}  MISSING {p}")
