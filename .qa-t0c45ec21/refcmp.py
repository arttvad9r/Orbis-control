#!/usr/bin/env python3
"""Compare base vs merged reference renders and merged vs baseline, per section."""
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path("/home/artt/Orbis-control-implementation")
B = ROOT / ".int-smoke/base"
M = ROOT / ".int-smoke/merged"
BL = ROOT / "docs/ui-audit-baseline"


def a(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def cmp(p1, p2, label):
    if not (p1.exists() and p2.exists()):
        print(f"  {label:44s} MISSING")
        return
    x, y = a(p1), a(p2)
    if x.shape != y.shape:
        print(f"  {label:44s} shape mismatch")
        return
    m = (x != y).any(axis=2)
    n = int(m.sum())
    if n == 0:
        print(f"  {label:44s} IDENTICAL")
        return
    ys, xs = np.nonzero(m)
    print(f"  {label:44s} differ n={n:6d} frac={m.mean():.6f} y=[{ys.min()},{ys.max()}] x=[{xs.min()},{xs.max()}]")


names = sorted(p.name for p in M.glob("*.png"))
print("=== base vs merged (same name) ===")
for n in names:
    cmp(B / n, M / n, n)

print()
print("=== merged vs docs/ui-audit-baseline (same name) ===")
for n in names:
    cmp(BL / n, M / n, n)

print()
print("=== merged vs docs/ui-audit-baseline, matched ignoring state suffix ===")
for n in names:
    base_sec = n.split("-")[0]
    for bl in sorted(BL.glob(f"{base_sec}-*.png")):
        pass
