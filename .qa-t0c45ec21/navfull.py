#!/usr/bin/env python3
"""Independent F4b measurement over the FULL live capture sets.

For every c5ca-* (verdict set, post-merge 5f0d3da) and every p980-* (control set,
pre-merge 5ca36ab) shot that shows the sidebar, measure the leftmost x of each
nav row's icon glyph, using a method that derives row bands from the image itself
(dark-row profile of the sidebar) rather than hard-coded y values.

Report per-file: icon left x per row, the set of distinct lefts, and the spread.
Contract expectation: c5ca-* lefts in a narrow corridor (planner measured 25..27,
spread=2); p980-* control spread ~47..67 (pre-fix zigzag).
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
SIDEBAR_W = 216          # sidebar column width in the 980x680 layout
X_LO, X_HI = 4, 200      # search window for glyphs inside the sidebar


def nav_lefts(path):
    """Return (icon_lefts, row_count) for the sidebar nav rows of one shot."""
    a = np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)
    h, w, _ = a.shape
    if w < 400:
        return None, None
    sb = a[:, 0:SIDEBAR_W, :]
    # sidebar background = most common colour in the sidebar
    flat = sb.reshape(-1, 3)
    vals, counts = np.unique(flat, axis=0, return_counts=True)
    bg = vals[int(np.argmax(counts))]
    fg = np.abs(sb - bg).sum(axis=2) > 60
    # row profile: rows that contain any glyph pixel
    rows = fg[:, X_LO:X_HI].any(axis=1)
    # segment into bands separated by >=3 empty rows
    bands = []
    start = None
    empty = 0
    for y, has in enumerate(rows):
        if has:
            if start is None:
                start = y
            empty = 0
        else:
            if start is not None:
                empty += 1
                if empty >= 3:
                    bands.append((start, y - empty + 1))
                    start = None
    if start is not None:
        bands.append((start, len(rows)))
    lefts = []
    for y0, y1 in bands:
        if y1 - y0 < 8:          # too thin to be a nav row
            continue
        band = fg[y0:y1, X_LO:X_HI]
        cols = np.where(band.any(axis=0))[0]
        if len(cols) == 0:
            continue
        lefts.append(int(cols[0]) + X_LO)
    return lefts, len(bands)


def report(files, tag):
    print(f"===== {tag}: {len(files)} files =====")
    alllefts = []
    for p in sorted(files):
        lefts, nb = nav_lefts(p)
        if not lefts:
            print(f"  {p.name:34s} (no sidebar rows detected)")
            continue
        uniq = sorted(set(lefts))
        spread = uniq[-1] - uniq[0] if len(uniq) > 1 else 0
        alllefts.extend(lefts)
        print(f"  {p.name:34s} rows={len(lefts):2d} lefts={lefts} distinct={uniq} spread={spread}")
    if alllefts:
        print(f"  --> POOLED: n={len(alllefts)} min={min(alllefts)} max={max(alllefts)} "
              f"spread={max(alllefts) - min(alllefts)}")
    print()


report(list(QA.glob("c5ca-*.png")), "VERDICT SET c5ca-* (post-merge 5f0d3da)")
report(list(QA.glob("p980-*.png")), "CONTROL SET p980-* (pre-merge 5ca36ab)")
