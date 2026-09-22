#!/usr/bin/env python3
"""Crop a PNG region and write it back as a valid RGB PNG (zoom xN optional)."""
import struct
import sys
import zlib

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from qa_about_overflow import decode_png, pixel  # noqa: E402


def write_png(path, width, height, rows):
    raw = bytearray()
    for row in rows:
        raw.append(0)
        raw.extend(row)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)

    def chunk(tag, payload):
        body = tag + payload
        return (
            struct.pack(">I", len(payload))
            + body
            + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)
        )

    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as fh:
        fh.write(png)


def main():
    src, dst = sys.argv[1], sys.argv[2]
    x0, y0, x1, y1 = (int(v) for v in sys.argv[3:7])
    zoom = int(sys.argv[7]) if len(sys.argv) > 7 else 1
    width, height, channels, buf = decode_png(src)
    stride = width * channels
    x1 = min(x1, width)
    y1 = min(y1, height)
    cw, ch = x1 - x0, y1 - y0
    out_w, out_h = cw * zoom, ch * zoom
    rows = []
    for oy in range(out_h):
        row = bytearray()
        sy = y0 + oy // zoom
        for ox in range(out_w):
            sx = x0 + ox // zoom
            r, g, b = pixel(buf, channels, stride, sx, sy)
            row.extend((r, g, b))
        rows.append(row)
    write_png(dst, out_w, out_h, rows)
    print(f"wrote {dst} ({out_w}x{out_h})")


if __name__ == "__main__":
    raise SystemExit(main())
