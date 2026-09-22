#!/usr/bin/env python3
"""Which tree did each screenshot set come from?

Measures nav icon-column spread (F4b discriminator) for every QA shot and every
in-repo render, then pixel-compares sets to decide provenance.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

ROWS = [
    ("glav", 56, 100), ("perf", 106, 150), ("pit", 154, 198), ("ohl", 198, 242),
    ("graf", 242, 286), ("podsv", 286, 330), ("ekr", 330, 374), ("sist", 374, 418),
    ("nastr", 570, 614), ("opro", 618, 662),
]


def icon_x0_spread(path):
    """Leftmost glyph pixel per nav row; spread across rows."""
    img = Image.open(path).convert("RGB")
    a = np.asarray(img, dtype=np.int16)
    if a.shape[0] < 680 or a.shape[1] < 232:
        return None
    bg = a[500, 150]
    diff = np.abs(a - bg).sum(axis=2)
    xs = []
    for _, y0, y1 in ROWS:
        band = diff[y0:y1, 0:232]
        fg = band > 60
        colcount = fg.sum(axis=0)
        fill_cols = colcount > (y1 - y0) * 0.75
        glyph = np.where((fg.any(axis=0)) & (~fill_cols))[0]
        if len(glyph) == 0:
            continue
        xs.append(int(glyph[0]))
    if not xs:
        return None
    return max(xs) - min(xs), len(set(xs))


def px_diff(p, q):
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    b = np.asarray(Image.open(q).convert("RGB"), dtype=np.int16)
    if a.shape != b.shape:
        return None
    return int((np.abs(a - b).sum(axis=2) > 12).sum())


print("=== QA shot sets: nav icon-column spread (0-3 == aligned/F4b PASS; >20 == zigzag/pre-fix) ===")
for p in sorted(QA.glob("*.png")):
    r = icon_x0_spread(p)
    if r is None:
        print(f"  {p.name:34s} (not a 980-wide app frame)")
        continue
    spread, distinct = r
    tag = "ALIGNED(post-fix)" if spread <= 3 else ("ZIGZAG(pre-fix)" if spread > 20 else "mixed")
    print(f"  {p.name:34s} spread={spread:3d} distinct_cols={distinct}  {tag}")

print()
print("=== in-repo renders ===")
for p in sorted((ROOT / ".int-smoke/merged").glob("*980x680*.png"))[:4] + \
         sorted((ROOT / "docs/ui-audit-baseline").glob("*980x680*.png"))[:4]:
    r = icon_x0_spread(p)
    tag = "" if r is None else ("ALIGNED" if r[0] <= 3 else "ZIGZAG")
    print(f"  {p.relative_to(ROOT)!s:64s} spread={r} {tag}")

print()
print("=== pixel comparisons (differing pixels, threshold 12) ===")
pairs = [
    ("p980-settings", QA / "p980-settings.png",
     "baseline/settings-980x680-dark", ROOT / "docs/ui-audit-baseline/settings-980x680-dark.png"),
    ("p980-settings", QA / "p980-settings.png",
     "baseline/settings-980x680-light", ROOT / "docs/ui-audit-baseline/settings-980x680-light.png"),
    ("p980-settings", QA / "p980-settings.png",
     "merged/settings-980x680-dark", ROOT / ".int-smoke/merged/settings-980x680-dark.png"),
    ("c5ca-settings-light", QA / "c5ca-settings-light.png",
     "merged/settings-980x680-light", ROOT / ".int-smoke/merged/settings-980x680-light.png"),
    ("c5ca-dashboard-light", QA / "c5ca-dashboard-light.png",
     "merged/dashboard-980x680-light", ROOT / ".int-smoke/merged/dashboard-980x680-light.png"),
]
for ln, p, rn, q in pairs:
    if not (p.exists() and q.exists()):
        print(f"  {ln} vs {rn}: MISSING")
        continue
    d = px_diff(p, q)
    print(f"  {ln:22s} vs {rn:34s} diff_px={d}")
