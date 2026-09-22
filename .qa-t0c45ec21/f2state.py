#!/usr/bin/env python3
"""Decisive F2 state analysis on the frozen candidate evidence."""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")
BASE = R / "docs/ui-audit-baseline"
MERGED = R / ".int-smoke/merged"


def a(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def ident(p1, p2):
    x, y = a(p1), a(p2)
    if x.shape != y.shape:
        return f"shape {x.shape} vs {y.shape}"
    m = (x != y).any(axis=2)
    return "IDENTICAL" if not m.any() else f"DIFFER n={int(m.sum())} frac={m.mean():.6f}"


print("### 1. state-scenario collapse checks (renders that must NOT be identical)")
pairs = [
    (MERGED / "performance-980x680-dark.png", MERGED / "performance-980x680-dark-unsupported.png",
     "merged normal vs merged UNSUPPORTED (perf)"),
    (MERGED / "performance-980x680-dark.png", MERGED / "performance-980x680-light.png",
     "merged normal-dark vs merged LIGHT (perf)"),
    (BASE / "performance-980x680-dark.png", BASE / "performance-980x680-dark-unsupported.png",
     "baseline normal vs baseline unsupported (perf)"),
    (BASE / "performance-980x680-dark.png", BASE / "performance-980x680-dark-dirty.png",
     "baseline performance normal vs dirty"),
    (BASE / "performance-980x680-dark.png", BASE / "performance-980x680-dark-error.png",
     "baseline performance normal vs error"),
    (BASE / "performance-980x680-dark.png", BASE / "performance-980x680-dark-pending.png",
     "baseline performance normal vs pending"),
    (QA / "p980-performance.png", QA / "c5ca-performance-reselect.png",
     "p980-performance (pre-fix ctrl) vs c5ca-performance-reselect (verdict set)"),
    (QA / "c5ca-perf.png", QA / "c5ca-dashboard-early.png",
     "c5ca-perf vs c5ca-dashboard-early"),
]
for p1, p2, label in pairs:
    if not (p1.exists() and p2.exists()):
        print(f"  {label:56s} MISSING")
        continue
    print(f"  {label:56s} {ident(p1, p2)}")

print()
print("### 2. content-area row diff: c5ca-performance-reselect vs references")
LIVE = QA / "c5ca-performance-reselect.png"
for label, ref in [
    ("baseline/performance-980x680-dark (PRE-fix tree, not-ready)", BASE / "performance-980x680-dark.png"),
    ("merged/performance-980x680-dark (candidate tree, READY)", MERGED / "performance-980x680-dark.png"),
    ("merged/performance-980x680-dark-unsupported (candidate, unsupported)", MERGED / "performance-980x680-dark-unsupported.png"),
]:
    A, B = a(LIVE), a(ref)
    if A.shape != B.shape:
        print(f"  {label}: shape mismatch {A.shape} {B.shape}")
        continue
    m = (A[:, 216:, :] != B[:, 216:, :]).any(axis=2)
    ys, xs = np.nonzero(m)
    print(f"  vs {label}")
    if len(ys) == 0:
        print("        content area IDENTICAL")
        continue
    print(f"        content diff n={len(ys)} frac={m.mean():.6f} y=[{ys.min()},{ys.max()}] x=[{xs.min()+216},{xs.max()+216}]")
    rows = np.zeros(680, dtype=int)
    for y in ys:
        rows[y] += 1
    bands = []
    inb = False
    for y in range(680):
        if rows[y] > 0 and not inb:
            s = y
            inb = True
        elif rows[y] == 0 and inb:
            bands.append((s, y - 1, int(rows[s:y].sum())))
            inb = False
    if inb:
        bands.append((s, 679, int(rows[s:].sum())))
    print(f"        changed bands (y0,y1,npx): {bands}")

print()
print("### 3. card-border geometry on the performance page (content area)")
def card_borders(p, label):
    A = a(p)
    # card border strokes: light/dark lines spanning a wide x-range
    content = A[:, 216:980, :]
    bg = np.median(content.reshape(-1, 3), axis=0)
    d = np.abs(content - bg).sum(axis=2)
    wide = (d > 25).sum(axis=1)          # px per row significantly off background
    rows = np.where(wide > 300)[0]        # rows that are ~ full-width horizontal edges
    print(f"  {label}")
    print(f"     full-width edge rows: {rows.tolist()[:40]}")

for label, p in [
    ("LIVE c5ca-performance-reselect", LIVE),
    ("LIVE p980-performance (pre-fix ctrl)", QA / "p980-performance.png"),
    ("REF baseline/performance-980x680-dark (pre-fix)", BASE / "performance-980x680-dark.png"),
    ("REF merged/performance-980x680-dark (candidate)", MERGED / "performance-980x680-dark.png"),
]:
    if p.exists():
        card_borders(p, label)
