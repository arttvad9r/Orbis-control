#!/usr/bin/env python3
"""Objectively identify the SELECTED sidebar row and the page heading of a shot,
by comparing the sidebar column and header band against reference renders whose
page identity is already known (rendered by the harness with an explicit
section argument).
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
R = Path("/home/artt/Orbis-control-implementation")
M = R / ".int-smoke/merged"

ROWS = [("Главная", 56, 100), ("Производительность", 100, 144), ("Питание", 144, 188),
        ("Охлаждение", 188, 232), ("Графика", 232, 276), ("Подсветка", 276, 320),
        ("Экран", 320, 364), ("Система", 364, 408), ("Настройки", 570, 614),
        ("О программе", 614, 658)]


def load(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def selection_rows(A):
    """A selected row shows a tinted fill across the row (not text-ink columns)."""
    bg = np.median(A[430:470, 150:210].reshape(-1, 3), axis=0)
    res = []
    for name, y0, y1 in ROWS:
        band = A[y0:y1, 12:220, :]
        d = np.abs(band - bg).sum(axis=2)
        # coverage: fraction of the band's pixels that differ from the sidebar bg
        cov = (d > 12).mean()
        res.append((name, cov))
    return res


print("### sidebar row fill coverage (selected row = highest, near-full coverage)")
refs = {
    "REF merged/dashboard": M / "dashboard-980x680-dark.png",
    "REF merged/performance": M / "performance-980x680-dark.png",
    "REF merged/power": M / "power-980x680-dark.png",
    "REF merged/about": M / "about-980x680-dark.png",
    "REF merged/settings": M / "settings-980x680-dark.png",
    "LIVE c5ca-perf": QA / "c5ca-perf.png",
    "LIVE c5ca-performance-reselect": QA / "c5ca-performance-reselect.png",
    "LIVE c5ca-about": QA / "c5ca-about.png",
    "LIVE c5ca-power": QA / "c5ca-power.png",
    "LIVE c5ca-settings-dark-restore": QA / "c5ca-settings-dark-restore.png",
    "LIVE c5ca-dashboard-early": QA / "c5ca-dashboard-early.png",
    "CONTROL p980-performance": QA / "p980-performance.png",
}
for label, p in refs.items():
    A = load(p)
    cov = selection_rows(A)
    top = sorted(cov, key=lambda t: -t[1])[:2]
    print(f"  {label:36s} top: " + ", ".join(f"{n}={c:.2f}" for n, c in top))

print()
print("### header band (page title) match: live vs known-identity references")
def hdr_match(live, ref):
    A, B = load(live), load(ref)
    band = (slice(55, 105), slice(216, 980))
    x, y = A[band], B[band]
    return float((x != y).any(axis=2).mean())

known = {k: v for k, v in refs.items() if k.startswith("REF")}
for live_label in ["LIVE c5ca-perf", "LIVE c5ca-performance-reselect", "LIVE c5ca-about",
                   "LIVE c5ca-dashboard-early"]:
    lp = refs[live_label]
    scores = sorted(((hdr_match(lp, rp), rl) for rl, rp in known.items()))
    print(f"  {live_label}")
    for s, rl in scores[:3]:
        print(f"        header diff {s:.6f}  -> {rl}")
