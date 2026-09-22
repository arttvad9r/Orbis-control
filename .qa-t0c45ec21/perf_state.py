#!/usr/bin/env python3
"""Decisive: which tree/state does c5ca-performance-reselect.png reproduce?

Compare it against
  baseline/performance-980x680-dark.png  (pre-fix tree, power-limits NOT ready)
  merged/performance-980x680-dark.png    (post-fix tree, power-limits ready)
  merged/performance-980x680-dark-unsupported.png (post-fix, unsupported)
and report the card-band geometry plus the fallback card's title text extent.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

PAIRS = [
    ("LIVE c5ca-performance-reselect", QA / "c5ca-performance-reselect.png"),
    ("baseline PREfix not-ready", ROOT / "docs/ui-audit-baseline/performance-980x680-dark.png"),
    ("merged POSTfix ready", ROOT / ".int-smoke/merged/performance-980x680-dark.png"),
    ("merged POSTfix unsupported", ROOT / ".int-smoke/merged/performance-980x680-dark-unsupported.png"),
]


def arr(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


live = arr(PAIRS[0][1])
print("=== pixel comparison of every reference against the LIVE capture ===")
for label, p in PAIRS[1:]:
    a = arr(p)
    if a.shape != live.shape:
        print(f"  {label:28s} shape mismatch")
        continue
    mask = (a != live).any(axis=2)
    ys, xs = np.nonzero(mask)
    bbox = f"x=[{xs.min()},{xs.max()}] y=[{ys.min()},{ys.max()}]" if len(xs) else "none"
    print(f"  {label:28s} diff_frac={mask.mean():.6f} n={mask.sum():6d} bbox {bbox}")

print()
print("=== text extent inside the fallback/limits card region ===")
for label, p in PAIRS:
    a = arr(p)
    bg = a[660, 500]
    # the limits card neighbourhood on the LIVE capture sits at y 471..525
    for y0, y1, tag in ((460, 540, "y460-540"),):
        reg = a[y0:y1, 232:980]
        nonbg = np.abs(reg - bg).sum(axis=2) > 30
        cols = np.nonzero(nonbg.any(axis=0))[0]
        rows = np.nonzero(nonbg.any(axis=1))[0]
        if len(cols):
            print(f"  {label:28s} {tag}: x=[{cols.min() + 232},{cols.max() + 232}] "
                  f"y=[{rows.min() + y0},{rows.max() + y0}]")
        else:
            print(f"  {label:28s} {tag}: (empty)")
