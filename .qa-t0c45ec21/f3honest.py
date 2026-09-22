#!/usr/bin/env python3
"""Focused F3 (about overflow) verification and the F2 unsupported-state honesty.

F3: the About card's right border is a vertical line; paragraph TEXT must stay
inside it. Measure the card's right border x, then the rightmost TEXT ink.
F2-honesty: in the merged "unsupported" scenario the Performance page declares
perf Unavailable, yet the limits card region renders as fully populated Ready.
"""
import json
from pathlib import Path

import numpy as np
from PIL import Image

R = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")


def A(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def card_right_border(path, y0=200, y1=600):
    """The card border is a 1px vertical line brighter than the page background."""
    a = A(path)
    bg = np.median(a[:, 962:978, :].reshape(-1, 3), axis=0)
    band = a[y0:y1, :, :]
    d = np.abs(band - bg).sum(axis=2)
    # verticality: a border column is inked for >70% of the band
    colcount = (d > 8).sum(axis=0)
    cand = np.nonzero(colcount > (y1 - y0) * 0.70)[0]
    return int(cand.max()) if len(cand) else None


print("### F3 :: About paragraph text vs card right border")
for lbl, p in [("LIVE   c5ca-about (verdict set)", QA / "c5ca-about.png"),
               ("POSTFIX merged/about (candidate tree)", R / ".int-smoke/merged/about-980x680-dark.png"),
               ("PREFIX  baseline/about (PRE-fix tree)", R / "docs/ui-audit-baseline/about-980x680-dark.png"),
               ("CTRL   p980-about (pre-merge)", QA / "p980-about.png")]:
    if not p.exists():
        print(f"  {lbl}: MISSING")
        continue
    a = A(p)
    border = card_right_border(p)
    # text ink strictly inside the card, i.e. left of the border and not the border itself
    bg = np.median(a[:, 962:978, :].reshape(-1, 3), axis=0)
    body = a[150:640, 236:(border - 1), :]
    d = np.abs(body - bg).sum(axis=2)
    ink = d > 24
    if ink.any():
        ys, xs = np.nonzero(ink)
        text_r = int(xs.max()) + 236
        print(f"  {lbl}")
        print(f"      card right border x = {border}; rightmost text ink x = {text_r}"
              f"  -> margin = {border - text_r}px (positive = inside)")
    else:
        print(f"  {lbl}: no text ink found")

print()
print("### F2 honesty :: does 'unsupported' change the limits-card region?")
n = A(R / ".int-smoke/merged/performance-980x680-dark.png")
u = A(R / ".int-smoke/merged/performance-980x680-dark-unsupported.png")
region = (slice(312, 628), slice(222, 960))
diff = (n[region] != u[region]).any(axis=2)
print(f"  limits-card region y=312..627 x=222..959: differing px = {int(diff.sum())} "
      f"of {diff.size} ({100 * diff.mean():.3f}%)")
print(f"  -> the limits card renders IDENTICALLY in normal and unsupported: "
      f"{int(diff.sum()) == 0}")
whole = (n != u).any(axis=2)
print(f"  whole-page differing px = {int(whole.sum())} (the Unavailable messaging "
      f"lives in the profile-card area)")

print()
print("### honesty :: read-only discipline from the state probes")
for f in ["status-before-reselect.json", "status-after-reselect.json"]:
    p = QA / "logs" / f
    d = json.loads(p.read_text())
    print(f"  {f}: perf.current={d['performance']['current']} "
          f"charge_limit={d['battery']['charge_limit']['value']} "
          f"gpu.power={d['gpu']['power']['state']}")

b = json.loads((QA / "logs/status-before-reselect.json").read_text())
af = json.loads((QA / "logs/status-after-reselect.json").read_text())
def strip_times(o):
    if isinstance(o, dict):
        return {k: strip_times(v) for k, v in o.items() if k not in ("observed_at",)}
    if isinstance(o, list):
        return [strip_times(v) for v in o]
    return o
print(f"  identical after removing observation timestamps: {strip_times(b) == strip_times(af)}")
print(f"  -> no hardware value changed across the live interaction: "
      f"{strip_times(b) == strip_times(af)}")
