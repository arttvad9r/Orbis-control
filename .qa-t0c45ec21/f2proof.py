#!/usr/bin/env python3
"""Headline F2 verification, with a robust page-background reference.

Background is taken from a region that is empty page background in EVERY
image compared: the area below all cards and to the right of the card column
is not safe across images, so instead each image's page background is taken as
the MODE colour of the whole content column (the dominant colour of an almost
empty page area is the page background by definition).
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")

LIVE = QA / "c5ca-performance-reselect.png"
PRE = R / "docs/ui-audit-baseline/performance-980x680-dark.png"
POST = R / ".int-smoke/merged/performance-980x680-dark.png"
CTRL = QA / "p980-performance.png"


def A(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def page_bg(a):
    """Dominant colour of the content column = page background."""
    col = a[:, 220:960, :].reshape(-1, 3)
    vals, counts = np.unique(col, axis=0, return_counts=True)
    return vals[int(np.argmax(counts))]


def report(path, label):
    a = A(path)
    bg = page_bg(a)
    d = np.abs(a[:, 220:960, :] - bg).sum(axis=2)
    inked = d > 10
    # empty-band test over the region between profile card and limits card
    band = inked[302:467, :]
    n = int(band.sum())
    print(f"  {label}")
    print(f"      page bg = {bg.tolist()}")
    print(f"      y=302..466 (the D-AUD-G02 band) -> non-background px = {n} "
          f"of {band.size} ({100 * band.mean():.3f}%)")
    if n:
        ys, xs = np.nonzero(band)
        print(f"        located at y=[{ys.min() + 302},{ys.max() + 302}] "
              f"x=[{xs.min() + 220},{xs.max() + 220}]")

    # column ink bands (card structure)
    rowink = inked.sum(axis=1)
    rows = rowink > 2
    out, inb, s = [], False, 0
    for y in range(a.shape[0]):
        if rows[y] and not inb:
            s, inb = y, True
        elif not rows[y] and inb:
            if y - 1 - s >= 2:
                out.append((s, y - 1))
            inb = False
    if inb:
        out.append((s, a.shape[0] - 1))
    gaps = [out[i + 1][0] - out[i][1] - 1 for i in range(len(out) - 1)]
    print(f"      ink bands = {out}")
    print(f"      gaps     = {gaps}   phantom(>=100) = "
          f"{[g for g in gaps if g >= 100] or 'none'}")
    return out, gaps


print("### 1. D-AUD-G02 band emptiness")
res = {}
for p, l in [(LIVE, "LIVE c5ca-performance-reselect (verdict set)"),
             (CTRL, "CONTROL p980-performance (pre-fix tree)"),
             (PRE, "REF baseline/performance (pre-fix tree)"),
             (POST, "REF merged/performance (candidate tree, post-fix)")]:
    res[l] = report(p, l)
    print()

print("### 2. Structural comparison of the Performance page")
live_bands, live_gaps = res["LIVE c5ca-performance-reselect (verdict set)"]
pre_bands, _ = res["REF baseline/performance (pre-fix tree)"]
post_bands, _ = res["REF merged/performance (candidate tree, post-fix)"]
ctrl_bands, _ = res["CONTROL p980-performance (pre-fix tree)"]
print(f"  LIVE bands        = {live_bands}")
print(f"  PRE-FIX bands     = {pre_bands}")
print(f"  POST-FIX bands    = {post_bands}")
print(f"  CONTROL bands     = {ctrl_bands}")
print()
print(f"  LIVE identical to PRE-FIX structure?     {live_bands == pre_bands}")
print(f"  LIVE identical to CONTROL structure?     {live_bands == ctrl_bands}")
print(f"  LIVE identical to POST-FIX structure?    {live_bands == post_bands}")
