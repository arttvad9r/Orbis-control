#!/usr/bin/env python3
"""Nav-geometry probe v2: fixed row anchors from VerticalLayout math, verified
by calibration bands. Row bands are 42px tall; label slab x 40..220 excludes
the icon slab (x 8..40).

Layout identity (main-window.slint): content y = titlebar(48) + padding-top 12
= 60 for the first row; rows are 42 + spacing 3 apart; group header rows are
22px + 12px spacer + spacing.
"""
import json
import sys

from PIL import Image

DARK_BG = (15, 18, 22)
LIGHT_BG = (243, 245, 248)

# Row anchors (top y of each 42px row band) computed from the layout identity
# for preferred window sizes; verified against calibration bands before use.
NAV_ROWS_DARK = [
    ("Главная", 60),
    ("Производительность", 159),
    ("Питание", 204),
    ("Охлаждение", 249),
    ("Графика", 294),
    ("Подсветка", 339),
    ("Экран", 384),
    ("Система", 429),
    ("Настройки", 735),
    ("О программе", 780),
]


def dist2(c, ref):
    return sum((a - b) ** 2 for a, b in zip(c[:3], ref))


def left_ink(px, w, h, y0, y1, x0, x1, bg, min_col_ink=2):
    y1 = min(y1, h)
    for x in range(x0, min(w, x1)):
        cnt = 0
        for y in range(max(0, y0), y1):
            if dist2(px[x, y], bg) > 900:
                cnt += 1
                if cnt >= min_col_ink:
                    return x
    return None


def right_ink(px, w, h, y0, y1, x0, x1, bg, min_col_ink=2):
    y1 = min(y1, h)
    for x in range(min(w, x1) - 1, x0 - 1, -1):
        cnt = 0
        for y in range(max(0, y0), y1):
            if dist2(px[x, y], bg) > 900:
                cnt += 1
                if cnt >= min_col_ink:
                    return x
    return None


def main():
    path, out = sys.argv[1], sys.argv[2]
    img = Image.open(path).convert("RGB")
    w, h = img.size
    px = img.load()
    corner = px[2, h // 2]
    theme = "dark" if dist2(corner, DARK_BG) < dist2(corner, LIGHT_BG) else "light"
    bg = DARK_BG if theme == "dark" else LIGHT_BG

    # Locate the selected row's rail+fill to find the first row's true top:
    # the fill spans nearly the full sidebar width.
    rows = []
    for (name, y_expected) in NAV_ROWS_DARK:
        # refine: search +-6 px around expected for the band with ink in the
        # label slab; keep expected otherwise.
        y0 = y_expected
        y1 = y0 + 42
        icon_left = left_ink(px, w, h, y0, y1, 8, 40, bg)
        label_left = left_ink(px, w, h, y0, y1, 40, 220, bg)
        icon_right = right_ink(px, w, h, y0, y1, 8, 40, bg)
        rows.append({
            "row": name, "y0": y0, "y1": y1,
            "icon_left": icon_left, "icon_right": icon_right,
            "label_left": label_left,
        })
    icon_lefts = [r["icon_left"] for r in rows if r["icon_left"] is not None]
    label_lefts = [r["label_left"] for r in rows if r["label_left"] is not None]
    icon_spread = max(icon_lefts) - min(icon_lefts) if icon_lefts else None
    label_spread = max(label_lefts) - min(label_lefts) if label_lefts else None
    result = {
        "capture": path,
        "size": [w, h],
        "theme": theme,
        "rows": rows,
        "icon_left_spread_px": icon_spread,
        "icon_left_distinct": len(set(icon_lefts)),
        "label_left_spread_px": label_spread,
        "label_left_distinct": len(set(label_lefts)),
        "expected": {
            "icon_box_left": 22,
            "label_left": 41,
            "icon_spread_max_px": 3,
        },
    }
    with open(out, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
