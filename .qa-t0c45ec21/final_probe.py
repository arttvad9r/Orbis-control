#!/usr/bin/env python3
"""Clean F4a measurement with IMAGE-DERIVED nav row bands.

Row bands are detected from the image (no hard-coded y offsets): the sidebar
rows are found by clustering rows that contain icon-column ink (x 22..42) or
label ink (x 46..228), separated by >= 6 blank rows.

F4a claim: after the fix the icon is vertically centred within its row, i.e.
|icon_ink_center_y - row_center_y| is small and the icon/label midpoints agree.
Pre-fix (audit D-AUD-G04): icon top-aligned -> icon centre sits ABOVE the row
centre by ~10-15px.
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")

ICON_X = (20, 44)
LABEL_X = (46, 228)


def A(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def sidebar_bg(a):
    return np.median(a[430:470, 120:200].reshape(-1, 3), axis=0)


def nav_rows(path):
    a = A(path)
    bg = sidebar_bg(a)
    d = np.abs(a - bg).sum(axis=2)
    ink = d > 60
    icon_ink = ink[:, ICON_X[0]:ICON_X[1]].sum(axis=1)
    label_ink = ink[:, LABEL_X[0]:LABEL_X[1]].sum(axis=1)

    rows = []
    inb, s = False, 0
    for y in range(50, min(680, a.shape[0])):
        present = icon_ink[y] > 0 or label_ink[y] > 0
        if present and not inb:
            s, inb = y, True
        elif not present and inb:
            rows.append((s, y - 1))
            inb = False
    if inb:
        rows.append((s, min(679, a.shape[0] - 1)))
    # merge bands separated by <8 blank rows (row internals can have gaps)
    merged = []
    for b in rows:
        if merged and b[0] - merged[-1][1] <= 8:
            merged[-1] = (merged[-1][0], b[1])
        else:
            merged.append(list(b))
    return a, ink, [tuple(m) for m in merged]


def probe(path, label):
    a, ink, rows = nav_rows(path)
    print(f"  {label}")
    print(f"      detected {len(rows)} nav bands")
    deltas = []
    for (y0, y1) in rows:
        ic = ink[y0:y1 + 1, ICON_X[0]:ICON_X[1]]
        lc = ink[y0:y1 + 1, LABEL_X[0]:LABEL_X[1]]
        if not ic.any() or not lc.any():
            continue
        iys = np.nonzero(ic)[0]
        lys = np.nonzero(lc)[0]
        icon_c = y0 + (iys.min() + iys.max()) / 2
        lab_c = y0 + (lys.min() + lys.max()) / 2
        row_c = (y0 + y1) / 2
        deltas.append((icon_c - row_c, icon_c - lab_c))
    if deltas:
        di = [d[0] for d in deltas]
        dl = [d[1] for d in deltas]
        print(f"      icon_centre - row_centre : vals={[round(v,1) for v in di]}")
        print(f"          mean={np.mean(di):+.2f}  |max|={max(abs(v) for v in di):.2f}")
        print(f"      icon_centre - label_centre: vals={[round(v,1) for v in dl]}")
        print(f"          mean={np.mean(dl):+.2f}  |max|={max(abs(v) for v in dl):.2f}")
    return deltas


print("### F4a: icon vertical centring (image-derived rows)")
TARGETS = [
    ("LIVE   c5ca-about (verdict set)", QA / "c5ca-about.png"),
    ("LIVE   c5ca-power (verdict set)", QA / "c5ca-power.png"),
    ("LIVE   c5ca-system (verdict set)", QA / "c5ca-system.png"),
    ("REF    merged/dashboard (candidate tree)", R / ".int-smoke/merged/dashboard-980x680-dark.png"),
    ("REF    baseline/dashboard (PRE-fix tree)", R / "docs/ui-audit-baseline/dashboard-980x680-dark.png"),
    ("CTRL   p980-settings (pre-merge)", QA / "p980-settings.png"),
]
for lbl, p in TARGETS:
    if p.exists():
        probe(p, lbl)
        print()

print("### F1/F3 sweep on every c5ca page shot (content-column gutters)")
def gutters(path):
    a = A(path)
    bg = np.median(a[:, 962:978, :].reshape(-1, 3), axis=0)
    d = np.abs(a[:, 216:980, :] - bg).sum(axis=2)
    ink = d > 10
    cols = np.nonzero(ink.any(axis=0))[0]
    return (cols.min() + 216, cols.max() + 216)


print(f"  {'shot':38s} content-x span")
for p in sorted(QA.glob("c5ca-*.png")):
    a = A(p)
    if a.shape[1] != 980:
        continue
    print(f"  {p.name:38s} {gutters(p)}")
print(f"  {'merged/perf (candidate, post-fix)':38s} {gutters(R / '.int-smoke/merged/performance-980x680-dark.png')}")
print(f"  {'baseline/perf (PRE-fix)':38s} {gutters(R / 'docs/ui-audit-baseline/performance-980x680-dark.png')}")
