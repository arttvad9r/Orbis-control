#!/usr/bin/env python3
"""Final theme-safe F1 / F3 measurement. Content column is x >= 218 (right of the
sidebar). Page background is sampled from the right gutter strip just OUTSIDE the
card column (x 966..978), which is page fill in every render compared.
"""
from pathlib import Path

import numpy as np
from PIL import Image

R = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
X0, X1 = 218, 976


def A(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def bg_of(a):
    return np.median(a[:, 968:979, :].reshape(-1, 3), axis=0)


def col_span(path, y0, y1):
    a = A(path)
    bg = bg_of(a)
    band = a[y0:y1, X0:X1, :]
    d = np.abs(band - bg).sum(axis=2)
    ink = d > 12
    cols = np.nonzero(ink.any(axis=0))[0]
    return (int(cols.min()) + X0, int(cols.max()) + X0) if len(cols) else (None, None)


def right_border(path, y0, y1):
    """Vertical border line inside the content column: strong ink in x 900..975
    for >=80% of the band."""
    a = A(path)
    bg = bg_of(a)
    d = np.abs(a[y0:y1, 900:976, :] - bg).sum(axis=2)
    cc = (d > 24).sum(axis=0)
    cand = np.nonzero(cc > (y1 - y0) * 0.80)[0] + 900
    return int(cand.max()) if len(cand) else None


print("### F1 :: dashboard content-column x span")
for lbl, p in [("LIVE  c5ca-dashboard-early", QA / "c5ca-dashboard-early.png"),
               ("LIVE  c5ca-dashboard-top-again", QA / "c5ca-dashboard-top-again.png"),
               ("LIVE  c5ca-perf (page identity: Главная)", QA / "c5ca-perf.png"),
               ("POST  merged/dashboard (candidate tree)", R / ".int-smoke/merged/dashboard-980x680-dark.png"),
               ("PRE   baseline/dashboard (PRE-fix tree)", R / "docs/ui-audit-baseline/dashboard-980x680-dark.png")]:
    if not p.exists():
        print(f"  {lbl}: MISSING")
        continue
    l, r = col_span(p, 110, 670)
    print(f"  {lbl:42s} x=( {l} , {r} )  width={r - l if l else None}  "
          f"left-margin={l - 216 if l else None}")

print()
print("### F3 :: About — rightmost paragraph text ink vs the card's right border")
for lbl, p in [("LIVE  c5ca-about (verdict set)", QA / "c5ca-about.png"),
               ("POST  merged/about (candidate tree)", R / ".int-smoke/merged/about-980x680-dark.png"),
               ("PRE   baseline/about (PRE-fix tree)", R / "docs/ui-audit-baseline/about-980x680-dark.png")]:
    a = A(p)
    bg = bg_of(a)
    rb = right_border(p, 130, 600)
    if rb is None:
        print(f"  {lbl}: right border not detected")
        continue
    inner = a[130:600, 250:rb - 1, :]
    d = np.abs(inner - bg).sum(axis=2)
    ink = d > 30
    tr = int(np.nonzero(ink)[1].max()) + 250 if ink.any() else None
    print(f"  {lbl:42s} card right border x={rb}  text right x={tr}  "
          f"inside-margin={rb - tr if tr else None}px")
