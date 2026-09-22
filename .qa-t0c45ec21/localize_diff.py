#!/usr/bin/env python3
"""Localize WHERE c5ca-* and p980-* differ (constant 0.014389 across pages)."""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")


def arr(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


for sec in ["power", "cooling", "graphics", "display", "about"]:
    a = arr(QA / f"c5ca-{sec}.png")
    b = arr(QA / f"p980-{sec}.png")
    if a.shape != b.shape:
        print(f"{sec}: shape {a.shape} vs {b.shape}")
        continue
    mask = (a != b).any(axis=2)
    ys, xs = np.nonzero(mask)
    print(f"\n{sec}: n_diff={mask.sum()} bbox x=[{xs.min()},{xs.max()}] y=[{ys.min()},{ys.max()}]")
    # column profile: which x bands carry differences
    colcount = mask.sum(axis=0)
    rowcount = mask.sum(axis=1)
    bands = []
    inband = False
    for x, c in enumerate(colcount):
        if c > 0 and not inband:
            start = x; inband = True
        elif c == 0 and inband:
            bands.append((start, x - 1)); inband = False
    if inband:
        bands.append((start, len(colcount) - 1))
    print(f"  x-bands with diffs ({len(bands)}): {bands[:14]}{' ...' if len(bands) > 14 else ''}")
    rbands = []
    inband = False
    for y, c in enumerate(rowcount):
        if c > 0 and not inband:
            start = y; inband = True
        elif c == 0 and inband:
            rbands.append((start, y - 1)); inband = False
    if inband:
        rbands.append((start, len(rowcount) - 1))
    print(f"  y-bands with diffs ({len(rbands)}): {rbands[:14]}{' ...' if len(rbands) > 14 else ''}")
    # how many diff pixels are inside the sidebar vs the content area
    sidebar = mask[:, 0:232].sum()
    content = mask[:, 232:].sum()
    print(f"  sidebar(0-232)={sidebar} content(232-)={content}")
