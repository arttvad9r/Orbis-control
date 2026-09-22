#!/usr/bin/env python3
"""F4a: icon vertical centre vs LABEL vertical centre within each nav row.

Pre-fix: HorizontalLayout alignment:center -> both centered, but the audit
observed the icon top-aligned. Post-fix: explicit y centres the 18x18 icon box
and the label is centred by vertical-alignment.
Discriminator: per row, |icon_ink_cy - label_ink_cy|.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")

ROWS = [
    ("Главная", 56, 100), ("Производительность", 100, 144), ("Питание", 144, 188),
    ("Охлаждение", 188, 232), ("Графика", 232, 276), ("Подсветка", 276, 320),
    ("Экран", 320, 364), ("Система", 364, 408),
    ("Настройки", 570, 614), ("О программе", 614, 658),
]


def f4a(path, label):
    A = np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)
    bg = np.median(A[430:470, 150:210].reshape(-1, 3), axis=0)
    d = np.abs(A - bg).sum(axis=2)
    print(f"  -- {label}")
    out = []
    for name, y0, y1 in ROWS:
        band = d[y0:y1, 8:228]
        fg = band > 70
        colcount = fg.sum(axis=0)
        fill = colcount > (y1 - y0) * 0.75
        glyph = fg & ~fill
        ys, xs = np.nonzero(glyph)
        if len(xs) == 0:
            continue
        ux = sorted(set(xs.tolist()))
        split = ux[0]
        for a1, b1 in zip(ux, ux[1:]):
            if b1 - a1 >= 6:
                split = a1
                break
        iy = ys[xs <= split]
        ly = ys[xs > split]
        if len(iy) == 0 or len(ly) == 0:
            continue
        # ink centroid is biased by glyph shape; use the extent midpoint of each
        ic_mid = (iy.min() + iy.max()) / 2
        lc_mid = (ly.min() + ly.max()) / 2
        out.append(ic_mid - lc_mid)
        print(f"       {name:22s} icon_mid={ic_mid + y0:7.2f} label_mid={lc_mid + y0:7.2f} "
              f"delta={ic_mid - lc_mid:+6.2f}")
    if out:
        print(f"       -> |icon-label| mid max={max(abs(v) for v in out):.2f} "
              f"mean={np.mean([abs(v) for v in out]):.2f}")


for lbl, p in [
    ("LIVE c5ca-perf (verdict set)", QA / "c5ca-perf.png"),
    ("LIVE c5ca-about (verdict set)", QA / "c5ca-about.png"),
    ("REF merged/perf (candidate tree)", R / ".int-smoke/merged/performance-980x680-dark.png"),
    ("REF baseline/perf (PRE-fix tree)", R / "docs/ui-audit-baseline/performance-980x680-dark.png"),
    ("CONTROL p980-performance (pre-merge)", QA / "p980-performance.png"),
]:
    f4a(p, lbl)
