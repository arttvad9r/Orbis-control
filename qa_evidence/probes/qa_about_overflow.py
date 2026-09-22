#!/usr/bin/env python3
"""D-AUD-G03 overflow probe (reconstruction of QA qa_about_*.py).

Usage: qa_about_overflow.py <png> <boundary_x> <theme>

Scans the strip strictly right of the card's right content boundary and
reports any pixel that differs from the window background (text or border
ink). Exit 0 = no ink right of the boundary, exit 1 = overflow (with the
max offending x and per-column histogram).
"""
import struct
import sys
import zlib
from collections import Counter


def decode_png(path):
    with open(path, "rb") as fh:
        data = fh.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise RuntimeError(f"not a PNG: {path}")
    pos = 8
    width = height = 0
    bitdepth = colortype = interlace = None
    idat = bytearray()
    while pos < len(data):
        length = struct.unpack(">I", data[pos:pos + 4])[0]
        ctype = data[pos + 4:pos + 8]
        chunk = data[pos + 8:pos + 8 + length]
        if ctype == b"IHDR":
            width, height, bitdepth, colortype, _comp, _filt, interlace = struct.unpack(
                ">IIBBBBB", chunk
            )
        elif ctype == b"IDAT":
            idat.extend(chunk)
        elif ctype == b"IEND":
            break
        pos += 12 + length
    if width is None or width == 0 or height == 0:
        raise RuntimeError("missing IHDR")
    if bitdepth != 8 or interlace != 0:
        raise RuntimeError(f"unsupported PNG: depth={bitdepth} interlace={interlace}")
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[colortype]
    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    expected = (stride + 1) * height
    if len(raw) != expected:
        raise RuntimeError(f"unexpected IDAT size {len(raw)} != {expected}")
    out = bytearray(height * stride)
    prev = bytearray(stride)
    for y in range(height):
        ftype = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        if ftype == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif ftype == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif ftype == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif ftype == 4:
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return width, height, channels, bytes(out)


def pixel(buf, channels, stride, x, y):
    o = y * stride + x * channels
    return buf[o], buf[o + 1], buf[o + 2]


def main():
    png, boundary, theme = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    width, height, channels, buf = decode_png(png)
    stride = width * channels
    if channels == 1:
        raise RuntimeError("grayscale PNG unexpected")
    x0 = boundary + 1
    if x0 >= width:
        print(f"PASS {png}: boundary {boundary} at/past right edge {width}")
        return 0
    # Background reference: most frequent color in the scanned strip.
    counts = Counter()
    for y in range(height):
        for x in range(x0, width):
            counts[pixel(buf, channels, stride, x, y)] += 1
    bg = counts.most_common(1)[0][0]
    threshold = 30  # sum of per-channel deltas vs background
    offenders = []
    col_hist = Counter()
    for y in range(height):
        for x in range(x0, width):
            r, g, b = pixel(buf, channels, stride, x, y)
            if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > threshold:
                offenders.append((x, y, (r, g, b)))
                col_hist[x] += 1
    if not offenders:
        print(
            f"PASS {png} [{theme}] {width}x{height}: no ink right of x={boundary} "
            f"(bg={bg})"
        )
        return 0
    max_x = max(x for x, _y, _c in offenders)
    ys = [y for _x, y, _c in offenders]
    colors = Counter(c for _x, _y, c in offenders).most_common(6)
    cols = sorted(col_hist)
    print(
        f"FAIL {png} [{theme}] {width}x{height}: ink right of x={boundary}, "
        f"max_x={max_x}, y=[{min(ys)}..{max(ys)}], bg={bg}"
    )
    print(f"  offending columns: {cols}")
    print(f"  top offender colors: {colors}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
