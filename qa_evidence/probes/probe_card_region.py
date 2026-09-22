#!/usr/bin/env python3
"""Post-fix card-region probe: max ink x strictly right of boundary, y >= min_y."""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from qa_about_overflow import decode_png, pixel  # noqa: E402
from collections import Counter  # noqa: E402


def main():
    png, boundary, label = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    min_y = int(sys.argv[4]) if len(sys.argv) > 4 else 30
    width, height, channels, buf = decode_png(png)
    stride = width * channels
    counts = Counter()
    for y in range(height):
        for x in range(width):
            counts[pixel(buf, channels, stride, x, y)] += 1
    bg = counts.most_common(1)[0][0]
    max_x = None
    ys = []
    cols = set()
    for y in range(min_y, height):
        for x in range(boundary + 1, width):
            r, g, b = pixel(buf, channels, stride, x, y)
            if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > 30:
                max_x = x if max_x is None else max(max_x, x)
                ys.append(y)
                cols.add(x)
    if max_x is None:
        print(f"PASS {label} {width}x{height}: no ink right of x={boundary} for y>={min_y}")
        return 0
    print(
        f"FAIL {label} {width}x{height}: ink right of x={boundary}, max_x={max_x}, "
        f"y=[{min(ys)}..{max(ys)}], cols={len(cols)}"
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
