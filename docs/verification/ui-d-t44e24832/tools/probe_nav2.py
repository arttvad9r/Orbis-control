#!/usr/bin/env python3
"""Nav column + limits-row measurement from a live orbis-control capture.

Extends the audited D2 methodology (run t_86a3dbba, EVIDENCE.md) with:
- caption/selected-row filtering by left-ink position (D2's rule: captions
  have ink at x <= 20; the selected-row rail lives at x 12..20);
- icon-box measurement against the approved nav-item.slint contract
  (icon box 18x18 at absolute x 22..40 -> expected ink 23..25 at density 1);
- the F4a/F4b constant-column verdicts (icon spread <= 3px, label spread
  spread = 0 across nav rows).
"""
import json
import sys

from PIL import Image

DARK_BG = (15, 18, 22)
LIGHT_BG = (243, 245, 248)


def dist2(c, ref):
    return sum((a - b) ** 2 for a, b in zip(c[:3], ref))


def main():
    path = sys.argv[1]
    theme_arg = sys.argv[2] if len(sys.argv) > 2 else "auto"
    img = Image.open(path).convert("RGB")
    w, h = img.size
    px = img.load()

    corner = px[2, h // 2]
    theme = theme_arg
    if theme == "auto":
        theme = "dark" if dist2(corner, DARK_BG) < dist2(corner, LIGHT_BG) else "light"

    if theme == "dark":
        def is_ink(c):
            return sum(c) > 210
    else:
        # Calibrated on live light captures: icon/label ink core measures
        # sum 342-386, backgrounds and the selected-pill fill 707-747. The
        # midpoint cutoff (550) separates ink from background by >100 sum
        # units on both sides, so the geometry verdict is insensitive to
        # antialiasing at the edges.
        def is_ink(c):
            return sum(c) < 550

    # Row-band scan over the sidebar icon slab (x 12..45), below the titlebar.
    bands = []
    start = None
    gap = 0
    for y in range(56, h - 8):
        xs = [x for x in range(12, 45) if is_ink(px[x, y])]
        if xs:
            if start is None:
                start = y
            gap = 0
        elif start is not None:
            gap += 1
            if gap > 2:
                bands.append((start, y - gap))
                start = None
                gap = 0
    if start is not None:
        bands.append((start, h - 9))

    # Merge close bands (accent rail + row content on the selected row).
    merged = []
    for b in bands:
        if merged and b[0] - merged[-1][1] <= 6:
            merged[-1] = (merged[-1][0], b[1])
        else:
            merged.append(list(b))

    rows = []
    for y0, y1 in merged:
        band_h = y1 - y0 + 1
        # icon slab ink columns
        cols = []
        for x in range(12, 45):
            if any(is_ink(px[x, y]) for y in range(y0, y1 + 1)):
                cols.append(x)
        # label slab ink columns
        lcols = []
        for x in range(46, 221):
            if any(is_ink(px[x, y]) for y in range(y0, y1 + 1)):
                lcols.append(x)
        left = min(cols) if cols else None
        lleft = min(lcols) if lcols else None
        rows.append({
            "y0": y0,
            "y1": y1,
            "height": band_h,
            "ink_left": left,
            "label_left": lleft,
            "kind": (
                "caption" if left is not None and left <= 20 else
                "selected-rail" if left is not None and left <= 21 else
                "nav-row"
            ),
        })

    # A selected nav row merges its accent rail (x12..20) with the row content;
    # reclassify by re-deriving icon ink from x>=21 within the same band.
    for r in rows:
        if r["kind"] == "selected-rail":
            cols2 = [
                x for x in range(21, 45)
                if any(is_ink(px[x, y]) for y in range(r["y0"], r["y1"] + 1))
            ]
            if cols2:
                r["kind"] = "nav-row(selected)"
                r["ink_left"] = min(cols2)

    nav = [r for r in rows if r["kind"].startswith("nav-row") and r["height"] >= 10]
    icon_lefts = [r["ink_left"] for r in nav if r["ink_left"] is not None]
    label_lefts = [r["label_left"] for r in nav if r["label_left"] is not None]

    out = {
        "capture": path,
        "size": [w, h],
        "theme": theme,
        "bands": rows,
        "nav_rows_measured": len(nav),
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
    print(json.dumps(out, indent=1, ensure_ascii=False))


if __name__ == "__main__":
    main()
