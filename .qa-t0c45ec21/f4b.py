#!/usr/bin/env python3
"""Locate the true nav-row bands in the sidebar by row-profile clustering.

Then measure F4a (icon vertical centre vs label vertical centre within the row)
and F4b (constant icon/label x column) with the measured bands.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
SIDEBAR_W = 232


def arr(name):
    return np.asarray(Image.open(QA / name).convert("RGB"), dtype=np.int16)


def sidebar_profile(name):
    a = arr(name)
    bg = a[500, 116]
    diff = np.abs(a[:, 0:SIDEBAR_W] - bg).sum(axis=2)
    fg = diff > 30
    prof = fg[:, 0:220].sum(axis=1)
    return a, fg, prof


def bands(prof, y_start=50, min_gap=9, min_h=8):
    out = []
    inb = False
    start = y_start
    for y in range(y_start, len(prof)):
        if prof[y] > 0 and not inb:
            start = y
            inb = True
        elif prof[y] == 0 and inb:
            out.append((start, y - 1))
            inb = False
    if inb:
        out.append((start, len(prof) - 1))
    merged = []
    for r in out:
        if merged and r[0] - merged[-1][1] <= min_gap:
            merged[-1] = [merged[-1][0], r[1]]
        else:
            merged.append(list(r))
    return [(int(s), int(e)) for s, e in merged if e - s + 1 >= min_h]


for name in ("c5ca-settings-dark-restore.png", "c5ca-dashboard-early.png",
             "p980-settings.png"):
    a, fg, prof = sidebar_profile(name)
    print("=" * 88)
    print(f"{name}: sidebar foreground row profile (y: count) where count>0")
    bl = bands(prof)
    print(f"  bands: {bl}")
    for (y0, y1) in bl:
        sub = fg[y0:y1 + 1, 0:220]
        cols = sorted(set(int(c) for c in np.where(sub.any(axis=0))[0]))
        if not cols:
            continue
        icon_x0 = cols[0]
        icon_right = icon_x0
        for p, q in zip(cols, cols[1:]):
            if q - p >= 6:
                icon_right = p
                break
        label_cols = [c for c in cols if c > icon_right + 1]
        label_x0 = label_cols[0] if label_cols else None

        def yb(c0, c1):
            s = fg[y0:y1 + 1, c0:c1 + 1]
            ys = np.where(s.any(axis=1))[0]
            return (int(ys.min()) + y0, int(ys.max()) + y0) if len(ys) else (None, None)

        iy = yb(icon_x0, icon_right)
        ly = yb(label_x0, max(label_cols)) if label_x0 else (None, None)
        imid = (iy[0] + iy[1]) / 2 if iy[0] is not None else None
        lmid = (ly[0] + ly[1]) / 2 if ly[0] is not None else None
        print(f"   band y=[{y0:3d},{y1:3d}] h={y1 - y0 + 1:3d}  icon x=[{icon_x0},{icon_right}] "
              f"y={iy} mid={imid}   label x0={label_x0} y={ly} mid={lmid}  "
              f"delta(icon-label)={'-' if imid is None or lmid is None else format(imid - lmid, '+.1f')}")
    print()
