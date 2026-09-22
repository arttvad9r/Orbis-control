#!/usr/bin/env python3
"""Light-theme nav anomaly + residual checks.

Two live shots measure row-1 icon left = 4 instead of 24-27:
c5ca-dashboard-light.png and c5ca-settings-light.png.
Check whether the fixed tree's OWN light render shows the same value (then it is
a tree-wide property of the light theme, not a live-only regression).
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")

ROWS = [("Главная", 56, 100), ("Производительность", 100, 144), ("Питание", 144, 188),
        ("Охлаждение", 188, 232), ("Графика", 232, 276), ("Подсветка", 276, 320),
        ("Экран", 320, 364), ("Система", 364, 408), ("Настройки", 570, 614),
        ("О программе", 614, 658)]


def A(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def nav_lefts(path, x_lo=0, x_hi=228):
    a = A(path)
    # sidebar background: sample a quiet area of the sidebar, left of labels
    bg = np.median(a[430:470, 120:200].reshape(-1, 3), axis=0)
    out = []
    for name, y0, y1 in ROWS:
        band = a[y0:y1, x_lo:x_hi, :]
        d = np.abs(band - bg).sum(axis=2)
        fg = d > 60
        colcount = fg.sum(axis=0)
        fill = colcount > (y1 - y0) * 0.75
        cols = np.where(fg.any(axis=0) & (~fill))[0]
        if len(cols) == 0:
            out.append((name, None))
            continue
        xs = sorted(set(int(c) for c in cols))
        out.append((name, xs[0] + x_lo))
    return out


print("### nav icon left x per row (light theme focus)")
TARGETS = [
    ("LIVE c5ca-dashboard-light", QA / "c5ca-dashboard-light.png"),
    ("LIVE c5ca-settings-light", QA / "c5ca-settings-light.png"),
    ("REF merged/dashboard-980x680-light (candidate tree)", R / ".int-smoke/merged/dashboard-980x680-light.png"),
    ("REF merged/settings-980x680-dark (candidate tree)", R / ".int-smoke/merged/settings-980x680-dark.png"),
]
for lbl, p in TARGETS:
    if not p.exists():
        print(f"  {lbl}: MISSING")
        continue
    lefts = nav_lefts(p)
    vals = [v for _, v in lefts if v is not None]
    print(f"  {lbl}")
    print(f"      {[(n, v) for n, v in lefts]}")
    print(f"      distinct={sorted(set(vals))} spread={max(vals) - min(vals) if vals else 0}")

print()
print("### row-1 band detail: what ink sits at x<20 in the light shots?")
for lbl, p in [("LIVE c5ca-dashboard-light", QA / "c5ca-dashboard-light.png"),
               ("LIVE c5ca-settings-light", QA / "c5ca-settings-light.png"),
               ("REF merged/dashboard-980x680-light", R / ".int-smoke/merged/dashboard-980x680-light.png")]:
    a = A(p)
    bg = np.median(a[430:470, 120:200].reshape(-1, 3), axis=0)
    band = a[50:105, 0:40, :]
    d = np.abs(band - bg).sum(axis=2)
    m = d > 60
    n = int(m.sum())
    print(f"  {lbl}: ink px in y=50..104 x=0..39 -> {n}")
    if n:
        ys, xs = np.nonzero(m)
        print(f"      y=[{ys.min() + 50},{ys.max() + 50}] x=[{xs.min()},{xs.max()}] "
              f"colours={np.unique(band[m], axis=0)[:4].tolist()}")
    print(f"      sidebar bg sample = {bg.tolist()}")
