#!/usr/bin/env python3
"""F4a (icon vertical centering) + robustness of the F4b icon-column measure.

F4a expected after fix: each nav row's icon is vertically centered in the 44px
row, i.e. |icon_center_y - row_center_y| small and constant.

F4b: also re-measure the icon COLUMN using the icon ink centroid (robust to
glyph geometry) rather than the leftmost ink pixel, and inspect the light-theme
row that reported left=4 to decide whether it is a real anomaly or a fill
artifact.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")

ROWS = [
    ("Главная", 56, 100), ("Производительность", 100, 144), ("Питание", 144, 188),
    ("Охлаждение", 188, 232), ("Графика", 232, 276), ("Подсветка", 276, 320),
    ("Экран", 320, 364), ("Система", 364, 408),
    ("Настройки", 570, 614), ("О программе", 614, 658),
]


def load(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def rows_geometry(p, label):
    A = load(p)
    # sidebar background from a quiet area
    bg = np.median(A[430:470, 140:200].reshape(-1, 3), axis=0)
    d = np.abs(A - bg).sum(axis=2)
    print(f"  -- {label}")
    cols, centers, dy = [], [], []
    for name, y0, y1 in ROWS:
        band = d[y0:y1, 8:228]
        fg = band > 70
        colcount = fg.sum(axis=0)
        # a full-row selection/hover fill spans nearly the whole band height
        fill = colcount > (y1 - y0) * 0.75
        glyph = fg & ~fill
        ys, xs = np.nonzero(glyph)
        if len(xs) == 0:
            print(f"       {name:22s} no-glyph")
            continue
        # icon = ink cluster left of the first gap >= 6px
        ux = sorted(set(xs.tolist()))
        icon_right = ux[0]
        for a1, b1 in zip(ux, ux[1:]):
            if b1 - a1 >= 6:
                icon_right = a1
                break
        sel = xs <= icon_right
        ix = xs[sel]
        iy = ys[sel]
        col_centroid = float(ix.mean()) + 8
        col_left = int(ix.min()) + 8
        icon_cy = float(iy.mean()) + y0
        row_cy = (y0 + y1) / 2
        cols.append(col_centroid)
        centers.append(icon_cy - row_cy)
        print(f"       {name:22s} icon_left={col_left:3d} centroid={col_centroid:6.2f} "
              f"icon_cy-row_cy={icon_cy - row_cy:+6.2f} fillpx={int(fill.sum())}")
    if cols:
        print(f"       -> centroid spread={max(cols) - min(cols):.2f}  "
              f"F4a |dy| max={max(abs(c) for c in centers):.2f}")


print("### F4a vertical centering + robust F4b column (verdict set)")
for f in ["c5ca-perf.png", "c5ca-performance-reselect.png", "c5ca-dashboard-light.png",
          "c5ca-settings-light.png", "c5ca-about.png", "c5ca-after-rapid-nav.png"]:
    rows_geometry(QA / f, f)

print()
print("### control (pre-fix tree)")
for f in ["p980-performance.png", "p980-settings.png"]:
    rows_geometry(QA / f, f)

print()
print("### references")
rows_geometry(R / ".int-smoke/merged/performance-980x680-dark.png", "merged/perf (post-fix)")
rows_geometry(R / "docs/ui-audit-baseline/performance-980x680-dark.png", "baseline/perf (pre-fix)")

print()
print("### light-theme row-1 raw pixels (is left=4 a fill artifact?)")
A = load(QA / "c5ca-dashboard-light.png")
for y in range(56, 100, 6):
    row = A[y, 0:40]
    print("    y=%3d " % y + " ".join("%02x%02x%02x" % tuple(px) for px in row[::4]))
