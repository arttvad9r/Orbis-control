#!/usr/bin/env python3
"""Robust nav-column discriminator + PNG tIME/tEXt provenance for QA shots.

Robust method: for each nav row band, measure the LABEL start x (leftmost
foreground pixel to the right of the icon box) and the ICON glyph bbox, using
only pixels whose color matches icon/text tones. Selected-row and hover fills
are excluded by ignoring pixels that are horizontal runs wider than 60px.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

ROWS = [
    ("Главная", 56, 100),
    ("Производительность", 106, 150),
    ("Питание", 154, 198),
    ("Охлаждение", 198, 242),
    ("Графика", 242, 286),
    ("Подсветка", 286, 330),
    ("Экран", 330, 374),
    ("Система", 374, 418),
    ("Настройки", 570, 614),
    ("О программе", 618, 662),
]


def icon_and_label_x(path):
    img = Image.open(path).convert("RGB")
    a = np.asarray(img, dtype=np.int16)
    # sidebar background: sample a quiet spot
    bg = a[500, 150]
    diff = np.abs(a - bg).sum(axis=2)
    out = []
    for name, y0, y1 in ROWS:
        band = diff[y0:y1, 0:232]
        fg = band > 60
        # find columns with any fg, then drop wide fills
        cols = np.where(fg.any(axis=0))[0]
        if len(cols) == 0:
            out.append((name, None, None, "no-fg"))
            continue
        # row fill detection: count fg pixels per column; a full-width fill has ~row_height
        colcount = fg.sum(axis=0)
        fill_cols = colcount > (y1 - y0) * 0.75
        glyph_cols = np.where((fg.any(axis=0)) & (~fill_cols))[0]
        if len(glyph_cols) == 0:
            out.append((name, None, None, "fill-only"))
            continue
        # icon = first cluster, split on gaps >= 6px
        xs = sorted(set(int(c) for c in glyph_cols))
        icon_right = xs[0]
        for a1, b1 in zip(xs, xs[1:]):
            if b1 - a1 >= 6:
                icon_right = a1
                break
        label_x = next((x for x in xs if x > icon_right + 1), None)
        out.append((name, xs[0], label_x, f"fill_cols={int(fill_cols.sum())}"))
    return out


def png_chunks(path):
    data = path.read_bytes()
    pos = 8
    chunks = []
    while pos < len(data):
        ln = int.from_bytes(data[pos:pos + 4], "big")
        typ = data[pos + 4:pos + 8].decode("latin-1")
        body = data[pos + 8:pos + 8 + ln]
        extra = ""
        if typ == "tIME" and ln == 7:
            y = int.from_bytes(body[0:2], "big")
            mo, d, h, mi, s = body[2], body[3], body[4], body[5], body[6]
            extra = f" {y:04d}-{mo:02d}-{d:02d} {h:02d}:{mi:02d}:{s:02d}"
        if typ == "tEXt":
            extra = " " + repr(body.decode("latin-1"))[:120]
        chunks.append(typ + extra)
        pos += 12 + ln
    return chunks


TARGETS = [
    ("p980-settings (QA live)", QA / "p980-settings.png"),
    ("c5ca-settings-dark-restore (QA live)", QA / "c5ca-settings-dark-restore.png"),
    ("merged/settings (offscreen 5f0d3da)", ROOT / ".int-smoke/merged/settings-980x680-dark.png"),
    ("audit-baseline/settings (offscreen PRE-fix)", ROOT / "docs/ui-audit-baseline/settings-980x680-dark.png"),
]

for label, path in TARGETS:
    print(f"== {label} :: {path.name}")
    print("   chunks:", png_chunks(path))
    for name, ix, lx, note in icon_and_label_x(path):
        print(f"   {name:22s} icon_x={ix} label_x={lx} {note}")
