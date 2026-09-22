#!/usr/bin/env python3
"""Comprehensive live-vs-reference matcher.

For every live c5ca-*.png at 980x680, compare against EVERY reference render
(.int-smoke/merged, .int-smoke/base, docs/ui-audit-baseline; all sections/themes/
states) and report the best matches with diff_frac. This identifies what state and
tree each live capture actually reproduces, without trusting filenames.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")
REFDIRS = {
    "merged": ROOT / ".int-smoke/merged",
    "base": ROOT / ".int-smoke/base",
    "baseline": ROOT / "docs/ui-audit-baseline",
}

refs = []
for tag, d in REFDIRS.items():
    if d.is_dir():
        for f in sorted(d.glob("*.png")):
            refs.append((f"{tag}/{f.name}", f))
print(f"reference renders: {len(refs)}")


def arr(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


cache = {}
def refarr(p):
    if p not in cache:
        cache[p] = arr(p)
    return cache[p]


lives = sorted(QA.glob("c5ca-*.png"))
print(f"live captures: {len(lives)}")
print()
for p in lives:
    with Image.open(p) as im:
        w, h = im.size
    if (w, h) != (980, 680):
        print(f"  {p.name:34s} {w}x{h}  (not 980x680 - skipped)")
        continue
    live = arr(p)
    scores = []
    for name, rp in refs:
        ra = refarr(rp)
        if ra.shape != live.shape:
            continue
        scores.append((float((ra != live).any(axis=2).mean()), name))
    scores.sort()
    top = scores[:3]
    print(f"  {p.name}")
    for frac, name in top:
        print(f"        {frac:.6f}  {name}")
