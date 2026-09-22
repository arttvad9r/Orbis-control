#!/usr/bin/env python3
"""Summarize ink bands right of a boundary (paragraph line layout check)."""
import re
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from qa_about_overflow import decode_png, pixel  # noqa: E402
from collections import Counter  # noqa: E402


def main():
    png, boundary = sys.argv[1], int(sys.argv[2])
    width, height, channels, buf = decode_png(png)
    stride = width * channels
    counts = Counter()
    for y in range(height):
        for x in range(width):
            counts[pixel(buf, channels, stride, x, y)] += 1
    bg = counts.most_common(1)[0][0]
    rows = {}
    for y in range(30, height):
        xs = []
        for x in range(boundary + 1, width):
            r, g, b = pixel(buf, channels, stride, x, y)
            if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > 30:
                xs.append(x)
        if xs:
            rows[y] = (min(xs), max(xs))
    bands = []
    cur = None
    for y in sorted(rows):
        if cur is None:
            cur = [y, y, rows[y][0], rows[y][1]]
        elif y - cur[1] <= 3:
            cur[1] = y
            cur[2] = min(cur[2], rows[y][0])
            cur[3] = max(cur[3], rows[y][1])
        else:
            bands.append(cur)
            cur = [y, y, rows[y][0], rows[y][1]]
    if cur:
        bands.append(cur)
    print(f"{png}: ink bands right of x={boundary} (y>=30):")
    for y0, y1, x0, x1 in bands:
        print(f"  y={y0}..{y1}  x={x0}..{x1}")


if __name__ == "__main__":
    raise SystemExit(main())
