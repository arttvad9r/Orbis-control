#!/usr/bin/env python3
"""Sample pixel sums in the nav icon slab to debug is_ink thresholds."""
import sys

from PIL import Image

path = sys.argv[1]
im = Image.open(path).convert("RGB")
w, h = im.size
px = im.load()
for y in range(60, 460, 6):
    row = []
    for x in range(12, 46, 2):
        c = px[x, y]
        row.append(f"{x}:{sum(c)}")
    print(y, " ".join(row))
