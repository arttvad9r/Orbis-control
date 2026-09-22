#!/usr/bin/env python3
"""Pixel diff between baseline and fixed renders: bbox + count + row histogram."""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from qa_about_overflow import decode_png, pixel  # noqa: E402


def main():
    a, b = sys.argv[1], sys.argv[2]
    wa, ha, ca, ba = decode_png(a)
    wb, hb, cb, bb = decode_png(b)
    if (wa, ha) != (wb, hb):
        print(f"SIZES DIFFER: {wa}x{ha} vs {wb}x{hb}")
        return 1
    stride = wa * ca
    diff_rows = {}
    total = 0
    for y in range(ha):
        for x in range(wa):
            ra, ga, b_ = pixel(ba, ca, stride, x, y)
            rb, gb, b2 = pixel(bb, cb, stride, x, y)
            if abs(ra - rb) + abs(ga - gb) + abs(b_ - b2) > 12:
                total += 1
                diff_rows.setdefault(y, [x, x])[1] = x
                diff_rows[y][0] = min(diff_rows[y][0], x)
    if not diff_rows:
        print("IDENTICAL")
        return 0
    ys = sorted(diff_rows)
    xs0 = min(v[0] for v in diff_rows.values())
    xs1 = max(v[1] for v in diff_rows.values())
    print(f"diff pixels={total} bbox x=[{xs0}..{xs1}] y=[{ys[0]}..{ys[-1]}] rows={len(ys)}")
    gaps = [
        (ys[i], ys[i + 1]) for i in range(len(ys) - 1) if ys[i + 1] - ys[i] > 3
    ]
    print(f"row-band gaps: {gaps if gaps else 'none (contiguous)'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
