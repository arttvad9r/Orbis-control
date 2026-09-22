#!/usr/bin/env python3
"""Precise F2 (performance phantom band) + F3 (about text overflow) probes.

F2: locate the "Лимиты мощности" card's top edge y on the Performance page and
compare across live captures and reference renders. If the ready-card is hidden
with `visible:` instead of `if`, it still reserves its height, so the fallback
card is pushed ~160-170px lower than the layout position it would occupy.

F3: on the About page, locate the card's right border x and the rightmost text
pixel inside the card; report the overhang.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")


def rows_with_content(a, bg, x0=232, x1=980, thr=12, mincols=60):
    diff = np.abs(a[:, x0:x1] - bg).sum(axis=2)
    rowdiff = (diff > thr).sum(axis=1)
    bands = []
    inb = False
    start = 50
    for y in range(50, a.shape[0]):
        c = rowdiff[y] > mincols
        if c and not inb:
            start = y
            inb = True
        elif not c and inb:
            bands.append((start, y - 1))
            inb = False
    if inb:
        bands.append((start, a.shape[0] - 1))
    out = [b for b in bands if b[1] - b[0] >= 3]
    return out


PERF_TARGETS = [
    ("LIVE c5ca-performance-reselect", QA / "c5ca-performance-reselect.png"),
    ("LIVE p980-performance (pre-fix)", QA / "p980-performance.png"),
    ("REF merged/performance-980x680-dark", ROOT / ".int-smoke/merged/performance-980x680-dark.png"),
    ("REF base/performance-980x680-dark", ROOT / ".int-smoke/base/performance-980x680-dark.png"),
    ("REF merged/performance-unsupported", ROOT / ".int-smoke/merged/performance-980x680-dark-unsupported.png"),
    ("REF baseline/performance-980x680-dark", ROOT / "docs/ui-audit-baseline/performance-980x680-dark.png"),
    ("REF baseline/performance-980x680-dark-unsupported", ROOT / "docs/ui-audit-baseline/performance-980x680-dark-unsupported.png"),
]

print("=" * 92)
print("F2 probe — Performance page card bands (content column x 232..980)")
print("=" * 92)
for label, p in PERF_TARGETS:
    if not p.exists():
        print(f"{label:44s} MISSING {p}")
        continue
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    bg = a[660, 500]
    bands = rows_with_content(a, bg)
    print(f"\n{label}  ({a.shape[1]}x{a.shape[0]})  bg={bg.tolist()}")
    prev = None
    for (y0, y1) in bands:
        gapline = f"   gap_before={y0 - prev - 1:4d}px" if prev is not None else "   gap_before=  --"
        print(f"   band y=[{y0:3d},{y1:3d}] h={y1 - y0 + 1:3d}{gapline}")
        prev = y1

print()
print("=" * 92)
print("F3 probe — About page: card right border vs rightmost text pixel")
print("=" * 92)
ABOUT_TARGETS = [
    ("LIVE c5ca-about (dark)", QA / "c5ca-about.png", 951),
    ("REF merged/about-980x680-dark", ROOT / ".int-smoke/merged/about-980x680-dark.png", None),
    ("REF baseline/about-980x680-dark (pre-fix)", ROOT / "docs/ui-audit-baseline/about-980x680-dark.png", None),
    ("REF merged/about-980x680-light", ROOT / ".int-smoke/merged/about-980x680-light.png", None),
    ("REF baseline/about-980x680-light (pre-fix)", ROOT / "docs/ui-audit-baseline/about-980x680-light.png", None),
]
for label, p, card_right in ABOUT_TARGETS:
    if not p.exists():
        print(f"{label:44s} MISSING")
        continue
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    h, w, _ = a.shape
    # card border: the content column's card spans gutter..(w-gutter); find the
    # rightmost column that is a vertical border line across most of the card band
    bg = a[660, 500]
    diff = np.abs(a - bg).sum(axis=2)
    # look at the about card band, detect vertical lines
    band = diff[100:min(h, 640), :]
    colcount = (band > 12).sum(axis=0)
    full = np.where(colcount > (band.shape[0] * 0.6))[0]
    lines = [int(x) for x in full if x > 232]
    print(f"\n{label} ({w}x{h})")
    print(f"   full-height vertical lines x>232 (candidates for card borders): "
          f"{sorted(set(lines))[:12]}")
    # rightmost foreground (text) pixel in the content area, upper card band
    sub = diff[110:300, 240:w - 5]
    ys, xs = np.nonzero(sub > 20)
    if len(xs):
        print(f"   rightmost content pixel (y110-300) = x={int(xs.max()) + 240}")
    sub2 = diff[300:640, 240:w - 5]
    ys2, xs2 = np.nonzero(sub2 > 20)
    if len(xs2):
        print(f"   rightmost content pixel (y300-640) = x={int(xs2.max()) + 240}")
