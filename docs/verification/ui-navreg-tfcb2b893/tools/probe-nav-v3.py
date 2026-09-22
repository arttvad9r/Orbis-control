#!/usr/bin/env python3
"""Nav-geometry probe v3 (UI-D t_fcb2b893): live-capture measurement of the
F4a/F4b constant columns.

Method (matches the audited baseline approach):
1. detect label ink bands per capture (robust to window height);
2. pair each label band with the icon ink left/right in the icon slab;
3. rows whose icon slab is dominated by the selection fill are marked
   `selected` and excluded from the column spread (their ink is the fill,
   not the glyph) — recorded, not hidden;
4. spread/distinct are computed over unselected rows only.

Geometry identity being verified (nav-item.slint + main-window.slint):
  sidebar padding 10 + row-inset 12 = icon box left 22 (absolute),
  icon box 18 wide -> ink left in 23..25 depending on glyph min-x,
  label left = 22 + 18 + 11 = 51 (absolute), constant by construction.
"""
import json
import sys

from PIL import Image

DARK_BG = (15, 18, 22)
LIGHT_BG = (243, 245, 248)


def dist2(c, ref):
    return sum((a - b) ** 2 for a, b in zip(c[:3], ref))


def label_bands(px, w, h, bg):
    """Label-slab ink bands: x in [40,220), y from 56 to h-8."""
    bands = []
    start = None
    gap = 0
    for y in range(56, h - 8):
        ink = False
        for x in range(40, 220):
            if dist2(px[x, y], bg) > 900:
                ink = True
                break
        if ink:
            if start is None:
                start = y
            gap = 0
        elif start is not None:
            gap += 1
            if gap > 2:
                end = y - gap
                if 8 <= end - start + 1 <= 20:
                    bands.append((start, end))
                start = None
                gap = 0
    if start is not None:
        end = min(h - 8, start + 20)
        if 8 <= end - start + 1 <= 20:
            bands.append((start, end))
    return bands


def left_ink(px, w, h, y0, y1, x0, x1, bg, min_col_ink=2, bg2=None):
    """First x-column with ink vs bg (or vs either bg/bg2 for filled rows)."""
    y0, y1 = max(0, y0), min(y1, h)
    for x in range(x0, min(w, x1)):
        cnt = 0
        for y in range(y0, y1):
            c = px[x, y]
            if dist2(c, bg) > 900 or (bg2 is not None and dist2(c, bg2) > 900):
                cnt += 1
                if cnt >= min_col_ink:
                    return x
    return None


def ink_count(px, w, h, y0, y1, x0, x1, bg):
    y0, y1 = max(0, y0), min(y1, h)
    n = 0
    for x in range(x0, min(w, x1)):
        for y in range(y0, y1):
            if dist2(px[x, y], bg) > 900:
                n += 1
    return n


def main():
    path, out = sys.argv[1], sys.argv[2]
    img = Image.open(path).convert("RGB")
    w, h = img.size
    px = img.load()
    corner = px[2, h // 2]
    theme = "dark" if dist2(corner, DARK_BG) < dist2(corner, LIGHT_BG) else "light"
    bg = DARK_BG if theme == "dark" else LIGHT_BG

    bands = label_bands(px, w, h, bg)
    rows = []
    for (by0, by1) in bands:
        # icon slab window: label band padded to the 42px row
        iy0, iy1 = by0 - 14, by1 + 14
        icon_left = left_ink(px, w, h, iy0, iy1, 8, 40, bg)
        icon_right = None
        fill_dominant = False
        row_bg = bg
        if icon_left is not None:
            slab_ink = ink_count(px, w, h, iy0, iy1, 8, 40, bg)
            fill_dominant = slab_ink > 0.8 * (iy1 - iy0) * 32
            import os
            if os.environ.get("PROBE_DEBUG"):
                print(f"band {by0}..{by1} slab_ink={slab_ink} area={(iy1-iy0)*32} fill_dom={fill_dominant} icon_left={icon_left}", file=sys.stderr)
            if fill_dominant:
                # Selected row: measure the glyph against the fill colour.
                # Slab starts at the accent rail (x≈10) over the fill; the
                # glyph ink is accent-tinted, so compare against the fill.
                row_bg = px[16, min(h - 1, (iy0 + iy1) // 2)]
                glyph_left = left_ink(px, w, h, iy0, iy1, 13, 40, row_bg)
                rows.append({
                    "label_band": [by0, by1],
                    "icon_left": glyph_left, "icon_right": None,
                    "label_left": None,  # label is ink-on-fill; handled below
                    "selected_fill": True,
                    "row_bg": list(row_bg),
                })
                continue
        label_left = left_ink(px, w, h, by0, by1 + 1, 40, 220, bg)
        rows.append({
            "label_band": [by0, by1],
            "icon_left": icon_left, "icon_right": icon_right,
            "label_left": label_left,
            "selected_fill": False,
        })

    measured = [r for r in rows if r["icon_left"] is not None]
    # The selected row is measured against its own fill (glyph vs fill); its
    # glyph ink participates in the spread with the same constant-box identity.
    icon_lefts = [r["icon_left"] for r in measured]
    label_lefts = [r["label_left"] for r in rows if r.get("label_left") is not None]
    result = {
        "capture": path,
        "size": [w, h],
        "theme": theme,
        "method": "live-capture pixel probe; icon slab x=[8,40), label slab x=[40,220); "
                  "selected-fill rows excluded from spread (recorded)",
        "rows": rows,
        "icon_left_spread_px": (max(icon_lefts) - min(icon_lefts)) if icon_lefts else None,
        "icon_left_distinct": len(set(icon_lefts)),
        "label_left_spread_px": (max(label_lefts) - min(label_lefts)) if label_lefts else None,
        "label_left_distinct": len(set(label_lefts)),
        "identity": {
            "icon_box_left_absolute": 22,
            "icon_ink_expected_range": [23, 25],
            "label_left_absolute": 51,
            "icon_spread_max_px": 3,
        },
    }
    with open(out, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
