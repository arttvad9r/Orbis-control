#!/usr/bin/env python3
"""Independent F1-F4b property measurement on the c5ca-* (post-merge) set.

F1  dashboard content column uses the shared 28px page gutter
F2  performance has no phantom band (title card / fallback card adjacency)
F3  about paragraph text stays inside the card's right border
F4a nav icon vertically centred in its 44px row
F4b nav icon column + label column constant across all 10 rows

All measurements are on existing PNGs. No re-render.
"""
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")

SIDEBAR_W = 232          # sidebar occupies x 0..231 in the 980x680 window
NAV_ROWS = [
    ("Главная", 56, 100), ("Производительность", 106, 150),
    ("Питание", 154, 198), ("Охлаждение", 198, 242),
    ("Графика", 242, 286), ("Подсветка", 286, 330),
    ("Экран", 330, 374), ("Система", 374, 418),
    ("Настройки", 570, 614), ("О программе", 618, 662),
]


def arr(name):
    return np.asarray(Image.open(QA / name).convert("RGB"), dtype=np.int16)


def content_left_edge(a, y_lo=100, y_hi=640):
    """Leftmost x >= SIDEBAR_W where the content area stops being page background.

    Page background is sampled just right of the sidebar near the bottom, where
    no card is present.
    """
    bg = a[660, SIDEBAR_W + 4]
    band = a[y_lo:y_hi, SIDEBAR_W:, :]
    diff = np.abs(band - bg).sum(axis=2)
    colhit = (diff > 25).sum(axis=0)
    xs = np.where(colhit > 3)[0]
    # ignore the vertical scrollbar strip at the far right
    xs = xs[xs < (a.shape[1] - SIDEBAR_W - 12)]
    return (int(xs.min()) + SIDEBAR_W) if len(xs) else None


def right_most_content(a, y_lo=100, y_hi=640):
    bg = a[660, SIDEBAR_W + 4]
    band = a[y_lo:y_hi, SIDEBAR_W:, :]
    diff = np.abs(band - bg).sum(axis=2)
    colhit = (diff > 25).sum(axis=0)
    xs = np.where(colhit > 3)[0]
    xs = xs[xs < (a.shape[1] - SIDEBAR_W - 12)]
    return (int(xs.max()) + SIDEBAR_W) if len(xs) else None


def nav_geometry(a):
    bg = a[500, SIDEBAR_W // 2]
    diff = np.abs(a - bg).sum(axis=2)
    rows = []
    for name, y0, y1 in NAV_ROWS:
        band = diff[y0:y1, 0:SIDEBAR_W]
        fg = band > 60
        colcount = fg.sum(axis=0)
        fill = colcount > (y1 - y0) * 0.75
        glyph_cols = np.where(fg.any(axis=0) & (~fill))[0]
        if len(glyph_cols) == 0:
            rows.append((name, None, None, None))
            continue
        xs = sorted(set(int(c) for c in glyph_cols))
        icon_right = xs[0]
        for p, q in zip(xs, xs[1:]):
            if q - p >= 6:
                icon_right = p
                break
        label_x = next((x for x in xs if x > icon_right + 1), None)
        # vertical extent of the icon cluster (x <= icon_right)
        ys, _ = np.nonzero(fg[:, 0:icon_right + 1])
        icon_ytop, icon_ybot = (int(ys.min()), int(ys.max())) if len(ys) else (None, None)
        rows.append((name, xs[0], label_x, (icon_ytop, icon_ybot)))
    return rows


print("=" * 78)
print("F1/F4b -- page content column left edge (window x) and nav column, c5ca-*")
print("=" * 78)
hdr = f"{'file':32s} {'content_left':>12s} {'content_right':>13s}  icon_x set"
print(hdr)
for p in sorted(QA.glob("c5ca-*.png")):
    a = arr(p.name)
    if a.shape[1] != 980:
        continue
    cl = content_left_edge(a)
    cr = right_most_content(a)
    rows = nav_geometry(a)
    icons = sorted({r[1] for r in rows if r[1] is not None})
    labels = sorted({r[2] for r in rows if r[2] is not None})
    iset = f"{icons[0]}..{icons[-1]}" if icons else "-"
    lset = f"{labels[0]}..{labels[-1]}" if labels else "-"
    print(f"  {p.name:32s} {str(cl):>12s} {str(cr):>13s}  icon_x {iset} (spread "
          f"{icons[-1] - icons[0] if len(icons) > 1 else 0})  label_x {lset} (spread "
          f"{labels[-1] - labels[0] if len(labels) > 1 else 0})")

print()
print("=" * 78)
print("F4a -- vertical centring of the nav icon in its 44px row (c5ca-settings-dark-restore)")
print("=" * 78)
a = arr("c5ca-settings-dark-restore.png")
for name, y0, y1, (t, b) in [(n, y0, y1, v)
                             for n, y0, y1, v in [(r[0], r[1], r[2], r[3])
                                                  for r in nav_geometry(a)]]:
    if t is None:
        print(f"  {name:22s} no glyph pixels")
        continue
    row_mid = (y0 + y1) / 2
    icon_mid = (t + b) / 2
    print(f"  {name:22s} band y=[{y0},{y1}] mid={row_mid:6.1f}  icon y=[{t},{b}] "
          f"mid={icon_mid:6.1f}  offset={icon_mid - row_mid:+.1f}px")

print()
print("=" * 78)
print("F3 -- about page: rightmost text pixel vs card right border")
print("=" * 78)
for name in ("c5ca-about.png", "p980-about.png"):
    a = arr(name)
    bg = a[660, SIDEBAR_W + 4]
    diff = np.abs(a - bg).sum(axis=2)
    # about card region: find card border columns in y 80..600
    band = diff[80:600, SIDEBAR_W:, :]
    colhit = (band > 25).sum(axis=0)
    xs = np.where(colhit > 0)[0]
    print(f"  {name:20s} content x range [{xs.min() + SIDEBAR_W},"
          f"{xs.max() + SIDEBAR_W}] (window width 980)")

print()
print("=" * 78)
print("F2 -- performance page card-band map (row profile)")
print("=" * 78)
for name in ("c5ca-performance-reselect.png", "p980-performance.png"):
    a = arr(name)
    bg = a[660, SIDEBAR_W + 4]
    diff = np.abs(a - bg).sum(axis=2)
    rows = (diff[:, SIDEBAR_W:940] > 25).sum(axis=1)
    bands = []
    inb = False
    for y, c in enumerate(rows):
        if c > 0 and not inb:
            start = y
            inb = True
        elif c == 0 and inb:
            if y - start >= 4:
                bands.append((start, y - 1))
            inb = False
    print(f"  {name}:")
    print(f"    horizontal bands (y, height): "
          f"{[(s, e - s + 1) for s, e in bands]}")
    for s, e in bands:
        if e - s + 1 > 60:
            print(f"      gap after band ending {e}: next band starts "
                  f"{next((s2 for s2, _ in bands if s2 > e), 'END')}")
