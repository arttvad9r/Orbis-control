#!/usr/bin/env python3
"""Group the c5ca-* set by visual content (which page each capture really shows).

Strategy: cluster by pairwise diff_frac among the 980x680 c5ca shots only, so
page identity does not depend on the offscreen reference states.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")


def arr(p):
    im = Image.open(p).convert("RGB")
    return im.size, np.asarray(im, dtype=np.int16)


files = sorted(p.name for p in QA.glob("c5ca-*.png"))
data = {f: arr(QA / f) for f in files}

print("=== pairwise diff_frac within the c5ca set (lower = same page/state) ===")
names = [f for f in files if data[f][0] == (980, 680)]
hdr = "".join(f"{i:>6d}" for i in range(len(names)))
print(f"{'':38s}{hdr}")
for i, a in enumerate(names):
    row = f"{a:38s}"
    for j, b in enumerate(names):
        sa, xa = data[a]
        sb, xb = data[b]
        d = float((xa != xb).any(axis=2).mean())
        row += f"{d:6.3f}"
    print(row)

print()
print("=== identical (byte-equal pixel content) pairs ===")
for i, a in enumerate(names):
    for b in names[i + 1:]:
        _, xa = data[a]
        _, xb = data[b]
        if (xa == xb).all():
            print(f"   {a}  ==  {b}")
