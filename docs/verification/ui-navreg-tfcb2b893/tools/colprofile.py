#!/usr/bin/env python3
"""Debug: column ink profile for given y-bands (x=0..60), vs two bg refs."""
import sys
from PIL import Image

im = Image.open(sys.argv[1]).convert("RGB")
px = im.load()
w, h = im.size
bands = [(int(a), int(b)) for a, b in (p.split(":") for p in sys.argv[2].split(","))]

def d2(c, ref):
    return sum((a - b) ** 2 for a, b in zip(c[:3], ref))

for y0, y1 in bands:
    # sample refs: titlebar bg at top of slab, "fill" at (y0+21, x=16)
    bg_tb = px[112, max(0, y0 - 6)]
    fill = px[16, min(h - 1, y0 + 21)]
    print(f"band y={y0}..{y1} bg_tb={bg_tb} fill_sample(x=16)={fill}")
    for x in range(0, 60):
        cnt_tb = sum(1 for y in range(y0, min(y1, h)) if d2(px[x, y], bg_tb) > 900)
        cnt_fl = sum(1 for y in range(y0, min(y1, h)) if d2(px[x, y], fill) > 900)
        print(f"  x={x:3d} ink_vs_tb={cnt_tb:3d} ink_vs_fill={cnt_fl:3d} sample={px[x,(y0+y1)//2]}")
