#!/usr/bin/env python3
"""Independent page-identity check for the c5ca-* live set.

For every c5ca-*.png we ask: which known page render is it closest to?
Reference = .int-smoke/merged/<section>-980x680-dark.png (offscreen render of the
merged tree, proven byte-identical to the UI-QA build of f45f55d).

A wrong filename claim (e.g. a dashboard screenshot named c5ca-perf.png) shows up
as "best match != claimed section" or as a tiny best-vs-second margin.
"""
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
MERGED = ROOT / ".int-smoke/merged"

SECTIONS = ["dashboard", "performance", "power", "cooling", "graphics",
            "backlight", "display", "system", "settings", "about"]

refs = {}
for s in SECTIONS:
    p = MERGED / f"{s}-980x680-dark.png"
    refs[s] = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def frac(a, b):
    if a.shape != b.shape:
        return None
    return float((np.abs(a - b).sum(axis=2) > 30).mean())


def content_mask(a):
    """Rough 'content present' check: fraction of pixels differing from the modal bg."""
    return float(a.std())


print("=== c5ca-* page identity vs merged (5f0d3da) offscreen renders ===")
print(f"{'file':34s} {'size':10s} {'best(frac)':26s} {'2nd':22s} margin")
for p in sorted(QA.glob("c5ca-*.png")):
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    if a.shape[0] != 680 or a.shape[1] != 980:
        print(f"  {p.name:32s} {str(a.shape[1]) + 'x' + str(a.shape[0]):10s} "
              f"(not 980x680; skipped)")
        continue
    scores = sorted(((frac(a, refs[s]), s) for s in SECTIONS))
    best, second = scores[0], scores[1]
    margin = second[0] - best[0]
    print(f"  {p.name:32s} 980x680    best={best[1]:12s} {best[0]:.5f}  "
          f"2nd={second[1]:10s} {second[0]:.5f}  margin={margin:.5f}")

print()
print("=== full score matrix (rows = c5ca shots, cols = merged section) ===")
hdr = "  " + " " * 32 + "".join(f"{s[:5]:>8s}" for s in SECTIONS)
print(hdr)
for p in sorted(QA.glob("c5ca-*.png")):
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    if a.shape[0] != 680 or a.shape[1] != 980:
        continue
    row = "".join(f"{frac(a, refs[s]):8.4f}" for s in SECTIONS)
    print(f"  {p.name:32s}{row}")
