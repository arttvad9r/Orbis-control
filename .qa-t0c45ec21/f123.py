#!/usr/bin/env python3
"""Decisive F1/F2/F3 measurement on the live verdict set (c5ca-*) and controls.

F1 = dashboard content column uses 28px gutter (pre-fix: 14px).
     Measure: leftmost x of dashboard page-header/content text, and content
     column left/right edges, in live c5ca dashboard captures vs
     docs/ui-audit-baseline/dashboard-980x680-dark.png (pre-fix) and
     .int-smoke/merged/dashboard-980x680-dark.png (post-fix).

F2 = performance page must show NO phantom band when power-limits are not ready.
     Measure: vertical empty-band heights in the content column between the
     profile card and the limits card.

F3 = about paragraphs must stay inside the card right border.
     Measure: rightmost text x vs card border x on the about page.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")
BASE = ROOT / "docs/ui-audit-baseline"
MERGED = ROOT / ".int-smoke/merged"
MAIN_X = 216          # sidebar width; content starts to the right of it


def load(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def content_col(a, y0, y1):
    """Leftmost and rightmost x of foreground pixels in the content area."""
    h, w, _ = a.shape
    region = a[y0:y1, MAIN_X:w, :]
    flat = region.reshape(-1, 3)
    vals, counts = np.unique(flat, axis=0, return_counts=True)
    bg = vals[int(np.argmax(counts))]
    fg = np.abs(region - bg).sum(axis=2) > 60
    cols = np.where(fg.any(axis=0))[0]
    rows = np.where(fg.any(axis=1))[0]
    if len(cols) == 0:
        return None
    return (int(cols[0]) + MAIN_X, int(cols[-1]) + MAIN_X)


def text_rows(a, y0, y1, x0=None, x1=None):
    """y-bands containing foreground inside a horizontal window."""
    h, w, _ = a.shape
    x0 = MAIN_X if x0 is None else x0
    x1 = w if x1 is None else x1
    region = a[y0:y1, x0:x1, :]
    flat = region.reshape(-1, 3)
    vals, counts = np.unique(flat, axis=0, return_counts=True)
    bg = vals[int(np.argmax(counts))]
    fg = np.abs(region - bg).sum(axis=2) > 60
    rowhas = fg.any(axis=1)
    bands = []
    start = None
    empty = 0
    for i, has in enumerate(rowhas):
        if has:
            if start is None:
                start = i
            empty = 0
        else:
            if start is not None:
                empty += 1
                if empty >= 6:
                    bands.append((start + y0, i - empty + y0 + 1))
                    start = None
    if start is not None:
        bands.append((start + y0, len(rowhas) + y0))
    return bands


def gaps(bands):
    out = []
    for (a0, a1), (b0, b1) in zip(bands, bands[1:]):
        out.append(b0 - a1)
    return out


print("################ F1 :: dashboard gutter ################")
for label, p in [
    ("PRE-FIX  audit-baseline/dashboard-980x680-dark", BASE / "dashboard-980x680-dark.png"),
    ("POST-FIX merged/dashboard-980x680-dark", MERGED / "dashboard-980x680-dark.png"),
    ("LIVE     c5ca-dashboard-early", QA / "c5ca-dashboard-early.png"),
    ("LIVE     c5ca-dashboard-top-again", QA / "c5ca-dashboard-top-again.png"),
    ("LIVE     c5ca-dashboard-light", QA / "c5ca-dashboard-light.png"),
    ("LIVE     c5ca-after-nastroit", QA / "c5ca-after-nastroit.png"),
    ("CONTROL  p980-graphics (pre-merge)", QA / "p980-graphics.png"),
]:
    if not p.exists():
        print(f"  {label}: MISSING")
        continue
    a = load(p)
    span = content_col(a, 40, 660)
    print(f"  {label:52s} content-x span = {span}")

print()
print("################ F2 :: performance phantom band ################")
for label, p in [
    ("PRE-FIX  audit-baseline/performance-980x680-dark", BASE / "performance-980x680-dark.png"),
    ("POST-FIX merged/performance-980x680-dark", MERGED / "performance-980x680-dark.png"),
    ("POST-FIX merged/performance unsupported", MERGED / "performance-980x680-dark-unsupported.png"),
    ("LIVE     c5ca-performance-reselect", QA / "c5ca-performance-reselect.png"),
    ("LIVE     c5ca-perf", QA / "c5ca-perf.png"),
    ("LIVE     c5ca-dashboard-early", QA / "c5ca-dashboard-early.png"),
    ("CONTROL  p980-performance (pre-merge)", QA / "p980-performance.png"),
]:
    if not p.exists():
        print(f"  {label}: MISSING")
        continue
    a = load(p)
    bands = text_rows(a, 40, 665)
    g = gaps(bands)
    print(f"  {label:52s}")
    print(f"       bands={bands}")
    print(f"       gaps ={g}   max_gap={max(g) if g else 0}")

print()
print("################ F3 :: about paragraph overflow ################")
for label, p in [
    ("PRE-FIX  audit-baseline/about-980x680-dark", BASE / "about-980x680-dark.png"),
    ("POST-FIX merged/about-980x680-dark", MERGED / "about-980x680-dark.png"),
    ("LIVE     c5ca-about", QA / "c5ca-about.png"),
    ("CONTROL  p980-about (pre-merge)", QA / "p980-about.png"),
]:
    if not p.exists():
        print(f"  {label}: MISSING")
        continue
    a = load(p)
    span = content_col(a, 40, 665)
    print(f"  {label:52s} content-x span = {span}")
