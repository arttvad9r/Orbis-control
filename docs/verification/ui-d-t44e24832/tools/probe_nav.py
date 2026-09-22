#!/usr/bin/env python3
"""Measure the nav icon/label left-edge columns from a live orbis-control capture.

Contract (F4a/F4b, nav-item.slint): icon box 18x18 at constant x 22..40,
label column starts at constant x 51 (window-local logical px). The probe
finds icon-slab ink per nav row and reports the spread of left edges.

Slabs (window-local): icon x 12..44, label x 46..220. Rows are located by
scanning the sidebar (x 0..224) below the 48px titlebar for ink bands.
"""
import json
import sys

from PIL import Image


def ink_columns(px, x0, x1, y0, y1, is_ink):
    cols = []
    for x in range(x0, x1):
        for y in range(y0, y1):
            if is_ink(px[x, y]):
                cols.append(x)
                break
    return cols


def main():
    path = sys.argv[1]
    theme = sys.argv[2] if len(sys.argv) > 2 else "dark"
    img = Image.open(path).convert("RGB")
    w, h = img.size
    px = img.load()

    if theme == "dark":
        def is_ink(c):
            r, g, b = c
            return r + g + b > 210
    else:
        def is_ink(c):
            r, g, b = c
            return r + g + b < 330

    rows = []
    y = 48  # below the 48px titlebar
    while y < h - 8:
        band = any(is_ink(px[x, y]) for x in range(12, 45))
        rows.append((y, band))
        y += 1

    bands = []
    start = None
    for y, has in rows:
        if has and start is None:
            start = y
        elif not has and start is not None:
            bands.append((start, y - 1))
            start = None
    if start is not None:
        bands.append((start, rows[-1][0]))

    merged = []
    for b in bands:
        if merged and b[0] - merged[-1][1] <= 6:
            merged[-1] = (merged[-1][0], b[1])
        else:
            merged.append(list(b))

    result = []
    for y0, y1 in merged:
        band_h = y1 - y0 + 1
        cols = ink_columns(px, 12, 45, y0, y1 + 1, is_ink)
        label_cols = ink_columns(px, 46, 221, y0, y1 + 1, is_ink)
        result.append({
            "y0": y0,
            "y1": y1,
            "height": band_h,
            "icon_left": min(cols) if cols else None,
            "icon_right": max(cols) if cols else None,
            "label_left": min(label_cols) if label_cols else None,
        })

    icon_rows = [r for r in result if r["height"] >= 12 and r["icon_left"] is not None]

    icon_lefts = [r["icon_left"] for r in icon_rows]
    label_lefts = [r["label_left"] for r in icon_rows if r["label_left"] is not None]

    out = {
        "capture": path,
        "size": [w, h],
        "theme": theme,
        "bands": result,
        "icon_left_spread_px": (max(icon_lefts) - min(icon_lefts)) if icon_lefts else None,
        "icon_left_distinct": len(set(icon_lefts)),
        "label_left_spread_px": (max(label_lefts) - min(label_lefts)) if label_lefts else None,
        "label_left_distinct": len(set(label_lefts)),
        "icon_rows_measured": len(icon_rows),
        "identity": {
            "icon_box_left_absolute": 22,
            "icon_ink_expected_range": [23, 25],
            "label_left_absolute": 51,
            "icon_spread_max_px": 3,
        },
    }
    print(json.dumps(out, indent=1, ensure_ascii=False))


if __name__ == "__main__":
    main()
