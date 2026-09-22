#!/usr/bin/env python3
"""Cross-compare QA 'live' screenshots against known offscreen renders.

Question: are the QA artifacts (c5ca-*, p980-*) real live GUI captures of the
candidate, or byte-identical to the offscreen ui_snapshot example renders?
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
MERGED = Path("/home/artt/Orbis-control-implementation/.int-smoke/merged")
BASE_OFFSCREEN = Path("/home/artt/Orbis-control-implementation/.int-smoke/base")
BASELINE = Path("/home/artt/Orbis-control-implementation/docs/ui-audit-baseline")
QA3 = Path("/home/artt/Orbis-control-implementation/.worktrees/t_23cd8fb8/.qa3-shots")


def load(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def cmp(a_path, b_path):
    if not a_path.exists() or not b_path.exists():
        return f"MISSING a={a_path.exists()} b={b_path.exists()}"
    a, b = load(a_path), load(b_path)
    if a.shape != b.shape:
        return f"SHAPE {a.shape} vs {b.shape}"
    same = a.tobytes() == b.tobytes()
    frac = float((a != b).any(axis=2).mean())
    return f"identical={same} diff_frac={frac:.6f}"


print("== QA shots inventory ==")
for p in sorted(QA.glob("*.png")):
    with Image.open(p) as im:
        print(f"  {p.name:42s} {im.size} {p.stat().st_size}")

print()
print("== candidate render sources present ==")
for d in (MERGED, BASE_OFFSCREEN, BASELINE, QA3):
    print(f"  {d}: exists={d.exists()} pngs={len(list(d.glob('*.png'))) if d.exists() else 0}")

print()
print("== p980-* vs .int-smoke/merged (offscreen renders of merged 5f0d3da) ==")
for stem in ["dashboard", "performance", "power", "cooling", "graphics",
             "backlight", "display", "system", "settings", "about"]:
    qa_p = QA / f"p980-{stem}.png"
    mg = MERGED / f"{stem}-980x680-dark.png"
    print(f"  p980-{stem:12s} vs merged/{stem}-980x680-dark.png : {cmp(qa_p, mg)}")

print()
print("== p980-* vs .int-smoke/base (offscreen renders of 5ca36ab baseline) ==")
for stem in ["dashboard", "performance", "power", "cooling", "graphics",
             "backlight", "display", "system", "settings", "about"]:
    qa_p = QA / f"p980-{stem}.png"
    bp = BASE_OFFSCREEN / f"{stem}-980x680-dark.png"
    print(f"  p980-{stem:12s} vs base/{stem}-980x680-dark.png   : {cmp(qa_p, bp)}")

print()
print("== p980-* vs docs/ui-audit-baseline (symmetry-line audit baseline) ==")
for stem in ["dashboard", "performance", "power", "cooling", "graphics",
             "backlight", "display", "system", "settings", "about"]:
    qa_p = QA / f"p980-{stem}.png"
    bp = BASELINE / f"{stem}-980x680-dark.png"
    print(f"  p980-{stem:12s} vs baseline/{stem}-980x680-dark.png: {cmp(qa_p, bp)}")

print()
print("== p980-* vs QA3 live shots (f45f55d UI-QA) ==")
if QA3.exists():
    for stem in sorted(p.name for p in QA3.glob("*.png")):
        print(f"  qa3 {stem}")
    for stem in ["dashboard", "performance", "power", "cooling", "graphics",
                 "backlight", "display", "system", "settings", "about"]:
        qa_p = QA / f"p980-{stem}.png"
        c = QA3 / f"{stem}-980x680-dark.png"
        print(f"  p980-{stem:12s} vs qa3/{stem}-980x680-dark.png    : {cmp(qa_p, c)}")
else:
    print("  qa3 dir missing")
