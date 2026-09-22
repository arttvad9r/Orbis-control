#!/usr/bin/env python3
"""Crop an arbitrary region of a capture for close inspection."""
import sys

from PIL import Image

path = sys.argv[1]
x0, y0, x1, y1 = (int(a) for a in sys.argv[2:6])
scale = int(sys.argv[6]) if len(sys.argv) > 6 else 2
im = Image.open(path).convert("RGB")
region = im.crop((x0, y0, x1, y1))
if scale != 1:
    region = region.resize((region.width * scale, region.height * scale), Image.LANCZOS)
out = path.rsplit(".", 1)[0] + f"-crop{x0}_{y0}_{x1}_{y1}.png"
region.save(out)
print(out, region.size)
