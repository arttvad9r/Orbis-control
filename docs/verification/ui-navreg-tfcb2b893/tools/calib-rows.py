#!/usr/bin/env python3
"""Row-grid calibration: find nav row y-ranges by scanning a x-slab that only
nav rows cross (x 40..220), excluding group headers (short bands)."""
import sys

from PIL import Image

DARK_BG = (15, 18, 22)
LIGHT_BG = (243, 245, 248)


def dist2(c, ref):
    return sum((a - b) ** 2 for a, b in zip(c[:3], ref))


def main():
    path = sys.argv[1]
    img = Image.open(path).convert("RGB")
    w, h = img.size
    px = img.load()
    corner = px[2, h // 2]
    theme = "dark" if dist2(corner, DARK_BG) < dist2(corner, LIGHT_BG) else "light"
    bg = DARK_BG if theme == "dark" else LIGHT_BG
    # Ink rows in the label slab (x 40..220)
    rows = []
    for y in range(56, h - 8):
        ink = False
        for x in range(40, 220):
            if dist2(px[x, y], bg) > 900:
                ink = True
                break
        rows.append((y, ink))
    bands = []
    start = None
    gap = 0
    for y, ink in rows:
        if ink:
            if start is None:
                start = y
            gap = 0
        elif start is not None:
            gap += 1
            if gap > 2:
                end = y - gap
                if end - start + 1 >= 8:
                    bands.append((start, end, end - start + 1))
                start = None
                gap = 0
    if start is not None:
        bands.append((start, rows[-1][0], rows[-1][0] - start + 1))
    print(f"theme={theme}")
    for b in bands:
        print(f"band y={b[0]}..{b[1]} h={b[2]}")


if __name__ == "__main__":
    main()
