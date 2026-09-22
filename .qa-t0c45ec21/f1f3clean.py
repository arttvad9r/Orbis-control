#!/usr/bin/env python3
"""Theme-safe F1 (dashboard gutter) and F3 (about text inside card border).

Page background is the ScrollView's page fill. It is sampled from an area that
is page background in every target: the vertical strip immediately LEFT of the
card column but RIGHT of the sidebar (x 218..226 at a y inside the card band is
the card's own inset; instead sample the row-gap between two cards, which is
uniform across the content width).
"""
from pathlib import Path

import numpy as np
from PIL import Image

R = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")


def A(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def page_fill(path):
    """Modal colour of the content column below the last card = page background."""
    a = A(path)
    col = a[:, 230:950, :].reshape(-1, 3)
    vals, counts = np.unique(col, axis=0, return_counts=True)
    return vals[int(np.argmax(counts))]


def card_edges(path, y0, y1):
    a = A(path)
    bg = page_fill(path)
    band = a[y0:y1, 205:975, :]
    d = np.abs(band - bg).sum(axis=2)
    colcount = (d > 12).sum(axis=0)
    cand = np.nonzero(colcount > (y1 - y0) * 0.80)[0] + 205
    if len(cand) == 0:
        return None, None
    return int(cand.min()), int(cand.max())


print("### F1 :: dashboard card column x-edges (gutter symmetry)")
for lbl, p in [("LIVE  c5ca-dashboard-early", QA / "c5ca-dashboard-early.png"),
               ("LIVE  c5ca-dashboard-top-again", QA / "c5ca-dashboard-top-again.png"),
               ("LIVE  c5ca-dashboard-light (light)", QA / "c5ca-dashboard-light.png"),
               ("POST  merged/dashboard (candidate)", R / ".int-smoke/merged/dashboard-980x680-dark.png"),
               ("PRE   baseline/dashboard (pre-fix)", R / "docs/ui-audit-baseline/dashboard-980x680-dark.png")]:
    if not p.exists():
        print(f"  {lbl}: MISSING")
        continue
    l, r = card_edges(p, 120, 660)
    print(f"  {lbl:38s} card x = ({l}, {r})   right-margin={980 - r if r else None}")

print()
print("### F3 :: About — rightmost paragraph text ink vs card right border")
for lbl, p in [("LIVE  c5ca-about (verdict set)", QA / "c5ca-about.png"),
               ("POST  merged/about (candidate)", R / ".int-smoke/merged/about-980x680-dark.png"),
               ("PRE   baseline/about (pre-fix)", R / "docs/ui-audit-baseline/about-980x680-dark.png")]:
    a = A(p)
    bg = page_fill(p)
    l, r = card_edges(p, 120, 600)
    inner = a[150:600, 245:r - 1, :]
    d = np.abs(inner - bg).sum(axis=2)
    ink = d > 30
    tr = int(np.nonzero(ink)[1].max()) + 245 if ink.any() else None
    print(f"  {lbl:38s} card border right x={r}  text right x={tr}  "
          f"margin={(r - tr) if tr else None}px")
