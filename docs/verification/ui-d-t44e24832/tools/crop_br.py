#!/usr/bin/env python3
"""Crop the bottom-right region of a capture (plan's trusted 4-region measure)."""
import sys

from PIL import Image

path = sys.argv[1]
im = Image.open(path)
w, h = im.size
x0, y0 = (w * 2) // 3, (h * 2) // 3
out = path.rsplit(".", 1)[0] + "-br.png"
im.crop((x0, y0, w, h)).save(out)
print(out, im.size, "->", (w - x0, h - y0))
