#!/usr/bin/env python3
"""Find card rectangles in the content column and report vertical gaps.

Content column: x in [gutter_left, gutter_right] where the page gutter is 28px
from the content-area edges. We detect card boundaries by looking for rows whose
pixel content differs from the page background across a wide x span.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
SIDEBAR = 230
CONTENT_X0 = 232
CONTENT_X1 = 980


def analyse(name, theme="dark"):
    a = np.asarray(Image.open(QA / name).convert("RGB"), dtype=np.int16)
    h, w, _ = a.shape
    if w != 980:
        return None
    # page background: sample just right of the content gutter inside a gap-free area
    bg = a[660, 500] if theme == "dark" else a[660, 500]
    diff = np.abs(a[:, CONTENT_X0:CONTENT_X1] - bg).sum(axis=2)
    # card interiors are lighter than the page bg; count columns differing
    rowdiff = (diff > 12).sum(axis=1)
    print(f"\n=== {name} (page bg sample {bg.tolist()} at y=660,x=500) ===")
    bands = []
    inb = False
    for y in range(50, h):
        c = rowdiff[y] > 200
        if c and not inb:
            start = y
            inb = True
        elif not c and inb:
            bands.append((start, y - 1))
            inb = False
    if inb:
        bands.append((start, h - 1))
    for (y0, y1) in bands:
        print(f"   row-band y=[{y0:3d},{y1:3d}] height={y1 - y0 + 1:3d}")
    # gaps between bands
    for (a1, b1), (a2, b2) in zip(bands, bands[1:]):
        print(f"   GAP between y={b1} and y={a2} = {a2 - b1 - 1}px")
    return bands


for n in ("c5ca-performance-reselect.png", "c5ca-perf.png", "c5ca-dashboard-early.png",
          "c5ca-cooling.png", "c5ca-settings-dark-restore.png", "c5ca-about.png"):
    analyse(n)

# Also measure in the offscreen reference states for calibration
for p, label in ((Path("/home/artt/Orbis-control-implementation/.int-smoke/merged/performance-980x680-dark.png"), "merged/performance-980x680-dark"),
                 (Path("/home/artt/Orbis-control-implementation/.int-smoke/merged/performance-980x680-dark-unsupported.png"), "merged/performance-unsupported")):
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    h, w, _ = a.shape
    bg = a[660, 500]
    diff = np.abs(a[:, CONTENT_X0:CONTENT_X1] - bg).sum(axis=2)
    rowdiff = (diff > 12).sum(axis=1)
    bands = []
    inb = False
    for y in range(50, h):
        c = rowdiff[y] > 200
        if c and not inb:
            start = y
            inb = True
        elif not c and inb:
            bands.append((start, y - 1))
            inb = False
    if inb:
        bands.append((start, h - 1))
    print(f"\n=== REF {label} ===")
    for (y0, y1) in bands:
        print(f"   row-band y=[{y0:3d},{y1:3d}] height={y1 - y0 + 1:3d}")
    for (a1, b1), (a2, b2) in zip(bands, bands[1:]):
        print(f"   GAP between y={b1} and y={a2} = {a2 - b1 - 1}px")
