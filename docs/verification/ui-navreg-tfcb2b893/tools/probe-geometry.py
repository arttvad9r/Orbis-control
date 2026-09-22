#!/usr/bin/env python3
"""Live geometry probe for orbis-control nav column (UI-D card t_fcb2b893).

Method: deterministic pixel measurement on a live window capture (ImageMagick
`import`), replicating the audited F4b baseline method (icon-column left-ink per
nav row, spread and distinct-position count), plus label-left detection per row
via first text-ink column inside the row band.

Usage: probe-geometry.py <capture.png> <out.json> [--rows-y y1,y2,...]
Falls back to automatic row detection inside the sidebar band when --rows-y is
not given.
"""
import json
import sys

from PIL import Image

DARK_BG = (15, 18, 22)        # #0F1216 window/sidebar background
LIGHT_BG = (243, 245, 248)    # #F3F5F8


def dist2(c, ref):
    return sum((a - b) ** 2 for a, b in zip(c[:3], ref))


def is_bg(c, bg, tol=900):
    return dist2(c, bg) <= tol


def row_ink(img, y0, y1, x0, x1, bg):
    """Rows y in [y0,y1) where any pixel in [x0,x1) is ink (non-bg)."""
    px = img.load()
    w, h = img.size
    rows = []
    for y in range(max(0, y0), min(h, y1)):
        ink = False
        for x in range(x0, min(w, x1)):
            if not is_bg(px[x, y], bg):
                ink = True
                break
        rows.append((y, ink))
    return rows


def runs(rows, min_len=6):
    """Contiguous ink runs of at least min_len px."""
    out = []
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
                if end - start + 1 >= min_len:
                    out.append((start, end))
                start = None
                gap = 0
    if start is not None:
        end = rows[-1][0]
        if end - start + 1 >= min_len:
            out.append((start, end))
    return out


def left_ink(img, y0, y1, x0, x1, bg):
    px = img.load()
    w, h = img.size
    best = None
    for x in range(x0, min(w, x1)):
        col_ink = 0
        for y in range(max(0, y0), min(h, y1)):
            if not is_bg(px[x, y], bg):
                col_ink += 1
        if col_ink >= 2:
            best = x
            break
    return best


def main():
    path, out = sys.argv[1], sys.argv[2]
    rows_y = None
    if len(sys.argv) > 3 and sys.argv[3].startswith("--rows-y="):
        rows_y = [int(v) for v in sys.argv[3].split("=", 1)[1].split(",")]
    img = Image.open(path).convert("RGB")
    w, h = img.size
    px = img.load()
    # Sidebar band: x in [0, 224). Sample corner to pick theme background.
    corner = px[2, h // 2]
    theme = "dark" if dist2(corner, DARK_BG) < dist2(corner, LIGHT_BG) else "light"
    bg = DARK_BG if theme == "dark" else LIGHT_BG
    sb_x0, sb_x1 = 0, 224
    # Find ink bands vertically across the sidebar width, skipping the top
    # identity block (y<64) and bottom edge.
    bands = runs(row_ink(img, 56, h - 8, 0, sb_x1, bg), min_len=8)
    # Filter to nav-row-sized bands (>= 18px tall: icon box + label line)
    bands = [b for b in bands if b[1] - b[0] >= 17]
    rows = []
    for (y0, y1) in bands:
        icon_left = left_ink(img, y0, y1 + 1, 8, 40, bg)
        label_left = left_ink(img, y0, y1 + 1, 40, sb_x1 - 4, bg)
        rows.append({
            "y0": y0, "y1": y1, "icon_left": icon_left, "label_left": label_left,
        })
    if rows_y is not None:
        manual = []
        for y0 in rows_y:
            y1 = y0 + 41
            icon_left = left_ink(img, y0, y1 + 1, 8, 40, bg)
            label_left = left_ink(img, y0, y1 + 1, 40, sb_x1 - 4, bg)
            manual.append({"y0": y0, "y1": y1, "icon_left": icon_left,
                           "label_left": label_left})
        rows = manual
    icon_lefts = [r["icon_left"] for r in rows if r["icon_left"] is not None]
    label_lefts = [r["label_left"] for r in rows if r["label_left"] is not None]
    spread = (max(icon_lefts) - min(icon_lefts)) if icon_lefts else None
    distinct = len(set(icon_lefts)) if icon_lefts else 0
    label_spread = (max(label_lefts) - min(label_lefts)) if label_lefts else None
    label_distinct = len(set(label_lefts)) if label_lefts else 0
    result = {
        "capture": path,
        "size": [w, h],
        "theme": theme,
        "sidebar_x": [0, 224],
        "bands": rows,
        "icon_left_spread_px": spread,
        "icon_left_distinct": distinct,
        "label_left_spread_px": label_spread,
        "label_left_distinct": label_distinct,
    }
    with open(out, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
