#!/usr/bin/env python3
"""Full cross-matrix: every QA artifact set vs every known render source.

Sets:
  c5ca-*  (live captures, 07:17-07:34, post-merge)
  p980-*  (live captures, 07:06, pre-merge)
  win-01/02 (live captures)
Sources:
  .int-smoke/base        = offscreen renders of 5ca36ab (PRE-fix UI)
  .int-smoke/merged      = offscreen renders of 5f0d3da (POST-fix UI)
  docs/ui-audit-baseline = offscreen renders of the pre-fix symmetry line
  .worktrees/t_23cd8fb8/.qa3-shots = UI-QA2 shots of f45f55d
"""
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")

SECTIONS = ["dashboard", "performance", "power", "cooling", "graphics",
            "backlight", "display", "system", "settings", "about"]

SOURCES = {
    "base(5ca36ab,PREfix)": ROOT / ".int-smoke/base",
    "merged(5f0d3da,POSTfix)": ROOT / ".int-smoke/merged",
    "baseline(PREfix)": ROOT / "docs/ui-audit-baseline",
}
QA3 = ROOT / ".worktrees/t_23cd8fb8/.qa3-shots"


def arr(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def cmp(a, b):
    if a is None or b is None:
        return None
    if a.shape != b.shape:
        return "shape-mismatch"
    return float((a != b).any(axis=2).mean())


def qa_set(prefix, suffix=""):
    out = {}
    for s in SECTIONS:
        p = QA / f"{prefix}-{s}{suffix}.png"
        if p.exists():
            out[s] = arr(p)
    return out


for label, pref in (("c5ca", "c5ca"), ("p980", "p980")):
    qs = qa_set(pref)
    print(f"\n===== {label}-* vs sources (980x680 dark) =====")
    for src_name, src in SOURCES.items():
        for s in SECTIONS:
            if s not in qs:
                print(f"  {src_name:24s} {s:12s} : QA-side MISSING")
                continue
            b = src / f"{s}-980x680-dark.png"
            d = cmp(qs[s], arr(b) if b.exists() else None)
            if d is None:
                print(f"  {src_name:24s} {s:12s} : source MISSING")
            elif isinstance(d, str):
                print(f"  {src_name:24s} {s:12s} : {d}")
            else:
                print(f"  {src_name:24s} {s:12s} : diff_frac={d:.6f}")

# qa3 overlaps
print("\n===== QA sets vs qa3 (f45f55d live shots) =====")
for label, pref in (("c5ca", "c5ca"), ("p980", "p980")):
    for s, fn in (("about", "about-980x680-dark.png"),
                  ("dashboard", "dashboard-980x680-dark.png"),
                  ("settings", "settings-980x680-dark.png")):
        p = QA / f"{pref}-{s}.png"
        q = QA3 / fn
        if p.exists() and q.exists():
            d = cmp(arr(p), arr(q))
            print(f"  {label}-{s:12s} vs qa3/{fn:28s} diff_frac={d:.6f}")

# c5ca vs p980 directly (same page, two live runs)
print("\n===== c5ca-* vs p980-* (same page, two live captures) =====")
for s in SECTIONS:
    a, b = QA / f"c5ca-{s}.png", QA / f"p980-{s}.png"
    if a.exists() and b.exists():
        d = cmp(arr(a), arr(b))
        print(f"  {s:12s}: diff_frac={d:.6f}")
