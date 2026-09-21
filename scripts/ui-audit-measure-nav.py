#!/usr/bin/env python3
"""Measure sidebar nav geometry from audit screenshots (evidence for t_5fc90d28).

For each nav row band: bounding box of foreground pixels (icon+label),
icon x-range (first glyph cluster), and vertical centers of icon vs label.
"""
from PIL import Image
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "/home/artt/.hermes/profiles/ui/cache/scratch/audit-baseline/settings-980x680-dark.png"
# Row bands (y) for the 980x680 window; label row heights 44px pitch 48/44.
rows = [
    ("Главная", 56, 100),
    ("Производительность", 106, 150),
    ("Питание", 154, 198),
    ("Охлаждение", 198, 242),
    ("Графика", 242, 286),
    ("Подсветка", 286, 330),
    ("Экран", 330, 374),
    ("Система", 374, 418),
    ("Настройки", 570, 614),
    ("О программе", 618, 662),
]

img = Image.open(path).convert("RGB")
bg = img.getpixel((150, 500))  # sidebar empty area
print(f"image={path} size={img.size} sidebar-bg={bg}")

def fg_pixels(band):
    y0, y1 = band
    pts = []
    for y in range(y0, y1):
        for x in range(8, 222):
            p = img.getpixel((x, y))
            if sum(abs(a - b) for a, b in zip(p, bg)) > 40:
                pts.append((x, y))
    return pts

for name, y0, y1 in rows:
    pts = fg_pixels((y0, y1))
    if not pts:
        print(f"{name:22s} <no fg pixels — band guess wrong>")
        continue
    xs = [p[0] for p in pts]; ys = [p[1] for p in pts]
    minx, maxx, miny, maxy = min(xs), max(xs), min(ys), max(ys)
    # Icon cluster: pixels left of the first >=6px horizontal gap after minx.
    xsorted = sorted(set(xs))
    gap_x = None
    for a, b in zip(xsorted, xsorted[1:]):
        if b - a >= 6:
            gap_x = b
            break
    icon_right = gap_x if gap_x else minx + 18
    icon_pts = [p for p in pts if p[0] <= icon_right]
    label_pts = [p for p in pts if p[0] > icon_right]
    if icon_pts and label_pts:
        icy = (min(p[1] for p in icon_pts) + max(p[1] for p in icon_pts)) / 2
        lcy = (min(p[1] for p in label_pts) + max(p[1] for p in label_pts)) / 2
        print(f"{name:22s} row_y=[{y0},{y1}] icon_x=[{minx},{icon_right}] "
              f"icon_y=[{min(p[1] for p in icon_pts)},{max(p[1] for p in icon_pts)}] "
              f"label_x0={min(p[0] for p in label_pts)} "
              f"icon_cy={icy:.1f} label_cy={lcy:.1f} icon_offset={icy - lcy:+.1f}px")
    else:
        print(f"{name:22s} row_y=[{y0},{y1}] fg_x=[{minx},{maxx}] fg_y=[{miny},{maxy}] (single cluster)")
