#!/usr/bin/env python3
"""Which tree produced which render, and did the merge change any UI pixels?"""
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path("/home/artt/Orbis-control-implementation")
MERGED = ROOT / ".int-smoke/merged"
BASE = ROOT / ".int-smoke/base"
AUDIT = ROOT / "docs/ui-audit-baseline"
QA3 = ROOT / ".worktrees/t_23cd8fb8/.qa3-shots"

STEMS = ["dashboard", "performance", "power", "cooling", "graphics",
         "backlight", "display", "system", "settings", "about"]


def load(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def report(label, ax, ay, bx, by):
    pa, pb = ax / ay, bx / by
    if not pa.exists() or not pb.exists():
        print(f"  {label}: MISSING a={pa.exists()} b={pb.exists()}")
        return
    a, b = load(pa), load(pb)
    if a.shape != b.shape:
        print(f"  {label}: SHAPE {a.shape} vs {b.shape}")
        return
    same = a.tobytes() == b.tobytes()
    frac = float((a != b).any(axis=2).mean())
    print(f"  {label}: identical={same} diff_frac={frac:.6f}")


print("== A. merged vs base (did merging the UI line change 980x680 dark pixels?) ==")
for s in STEMS:
    report(f"{s:12s}", MERGED, f"{s}-980x680-dark.png", BASE, f"{s}-980x680-dark.png")

print()
print("== B. base vs docs/ui-audit-baseline (baseline vs audit reference line) ==")
for s in STEMS:
    report(f"{s:12s}", BASE, f"{s}-980x680-dark.png", AUDIT, f"{s}-980x680-dark.png")

print()
print("== C. merged vs docs/ui-audit-baseline ==")
for s in STEMS:
    report(f"{s:12s}", MERGED, f"{s}-980x680-dark.png", AUDIT, f"{s}-980x680-dark.png")

print()
print("== D. merged vs QA3 (f45f55d live shots) ==")
for s in ["about", "dashboard", "settings"]:
    report(f"{s:12s}", MERGED, f"{s}-980x680-dark.png", QA3, f"{s}-980x680-dark.png")

print()
print("== E. .qa3-shots inventory (UI-QA2 live shots) ==")
if QA3.exists():
    for p in sorted(QA3.glob("*.png")):
        with Image.open(p) as im:
            print(f"  {p.name:36s} {im.size}")
