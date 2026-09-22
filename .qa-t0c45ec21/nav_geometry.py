#!/usr/bin/env python3
"""Measure sidebar nav geometry across artifact sets.

Discriminator: the F4a/F4b fix pins every nav icon to a constant x column
(12px sidebar padding + 12px row inset = 24px window x) and starts every label
at the same x. Pre-fix layout centered each row's content, so the icon column
shifted with label length (a zigzag). Measuring icon_x and label_x0 per row on
pre-merge vs post-merge artifacts tells us which UI revision each set depicts.
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")

# Nav row bands (y) measured from the 980x680 window layout.
ROWS_980 = [
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


def rows_for_height(h):
    # 1200x800 shifts the bottom group; probe with the same relative offsets.
    return ROWS_980


def analyse(path, x_lo=8, x_hi=228):
    img = Image.open(path).convert("RGB")
    a = np.asarray(img, dtype=np.int16)
    h, w, _ = a.shape
    bg = np.median(a[500:520, 140:170].reshape(-1, 3), axis=0)
    out = []
    for name, y0, y1 in rows_for_height(h):
        if y1 > h:
            continue
        band = a[y0:y1, x_lo:x_hi, :]
        diff = np.abs(band - bg).sum(axis=2)
        ys, xs = np.nonzero(diff > 40)
        if len(xs) == 0:
            out.append((name, None, None))
            continue
        xs = xs + x_lo
        ux = sorted(set(xs.tolist()))
        gap = None
        for p, q in zip(ux, ux[1:]):
            if q - p >= 6:
                gap = q
                break
        icon_right = gap if gap else min(ux) + 18
        icon_x = [x for x in xs if x <= icon_right]
        label_x = [x for x in xs if x > icon_right]
        out.append((name, min(icon_x), min(label_x) if label_x else None))
    return img.size, out


def show(label, path):
    if not path.exists():
        print(f"-- {label}: MISSING {path}")
        return
    size, rows = analyse(path)
    print(f"-- {label} {size} ({path.name})")
    icol, lcol = [], []
    for name, ix, lx in rows:
        icol.append(ix)
        if lx is not None:
            lcol.append(lx)
        print(f"     {name:22s} icon_x0={ix} label_x0={lx}")
    if icol:
        uniq = sorted({v for v in icol if v is not None})
        print(f"     icon_x0 distinct values: {uniq}  spread={max(uniq)-min(uniq) if len(uniq) > 1 else 0}")
    if lcol:
        uniq = sorted(set(lcol))
        print(f"     label_x0 distinct values: {uniq}  spread={max(uniq)-min(uniq) if len(uniq) > 1 else 0}")


TARGETS = [
    ("p980-settings (pre-merge capture 04:06Z)", QA / "p980-settings.png"),
    ("p980-cooling (pre-merge capture)", QA / "p980-cooling.png"),
    ("c5ca-settings-light (post-merge capture)", QA / "c5ca-settings-light.png"),
    ("c5ca-cooling (post-merge capture)", QA / "c5ca-cooling.png"),
    ("INT-merged/settings (offscreen, fixed tree)", ROOT / ".int-smoke/merged/settings-980x680-dark.png"),
    ("audit-baseline/settings (offscreen, PRE-fix tree)", ROOT / "docs/ui-audit-baseline/settings-980x680-dark.png"),
    ("UIQA2-qa3/settings (offscreen f45f55d)", ROOT / ".worktrees/t_23cd8fb8/.qa3-shots/settings-980x680-dark.png"),
]

for label, path in TARGETS:
    show(label, path)
