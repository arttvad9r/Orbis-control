#!/usr/bin/env python3
"""Final consolidated verification report for the frozen candidate evidence."""
import json
import subprocess
from pathlib import Path

import numpy as np
from PIL import Image

R = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
L = []
def p(s=""):
    L.append(s)


def A(path):
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)


def page_bg(a):
    return np.median(a[:, 962:978, :].reshape(-1, 3), axis=0)


def card_right(path, y0, y1):
    a = A(path)
    bg = page_bg(a)
    d = np.abs(a[y0:y1, :, :] - bg).sum(axis=2)
    cc = (d > 20).sum(axis=0)
    cand = np.nonzero(cc > (y1 - y0) * 0.85)[0]
    return int(cand.max()) if len(cand) else None


p("### F3 :: About page — text ink vs card right border")
import subprocess as sp
p("  source state (about.slint: does it still set width: parent.width?)")
for r in ["860bc07", "2c3e3af", "f45f55d", "5f0d3da", "a21139e8"]:
    src = sp.run(["git", "-C", str(R), "show", f"{r}:ui/audited/sections/about.slint"],
                 capture_output=True, text=True).stdout
    n = sum(1 for l in src.splitlines() if "width: parent.width" in l and l.strip().startswith("width"))
    p(f"      {r}: 'width: parent.width' occurrences = {n}")

for lbl, path, yband in [("LIVE   c5ca-about", QA / "c5ca-about.png", (150, 640)),
                         ("POSTFIX merged/about", R / ".int-smoke/merged/about-980x680-dark.png", (150, 640)),
                         ("PREFIX  baseline/about", R / "docs/ui-audit-baseline/about-980x680-dark.png", (150, 640))]:
    a = A(path)
    b = card_right(path, *yband)
    bg = page_bg(a)
    # text ink: inside the card, excluding the border column itself
    inner = a[yband[0]:yband[1], 240:b - 2, :]
    d = np.abs(inner - bg).sum(axis=2)
    ink = d > 30
    if ink.any():
        xs = np.nonzero(ink)[1]
        tr = int(xs.max()) + 240
        p(f"  {lbl}: border x={b}  rightmost text ink x={tr}  margin={b - tr}px")
    else:
        p(f"  {lbl}: border x={b}  (no text ink above threshold inside the card)")

p()
p("### F2 :: honesty — the limits card in a declared-Unavailable scenario")
n = A(R / ".int-smoke/merged/performance-980x680-dark.png")
u = A(R / ".int-smoke/merged/performance-980x680-dark-unsupported.png")
reg = (slice(312, 628), slice(222, 960))
diff = (n[reg] != u[reg]).any(axis=2)
p(f"  limits-card region: differing px = {int(diff.sum())} of {diff.size} "
  f"({100 * diff.mean():.3f}%)")
p(f"  -> identical in normal vs unsupported: {int(diff.sum()) == 0}")

p()
p("### honesty :: read-only discipline (state probes)")
for f in ["status-before-reselect.json", "status-after-reselect.json"]:
    d = json.loads((QA.parent / "logs" / f).read_text())
    p(f"  {f}:")
    p(f"      perf.current        = {d['performance']['current']}")
    p(f"      charge_limit.value  = {d['battery']['charge_limit']['value']}")
    p(f"      gpu.power           = {d['gpu']['power']}")


def strip(o):
    if isinstance(o, dict):
        return {k: strip(v) for k, v in o.items() if k != "observed_at"}
    if isinstance(o, list):
        return [strip(v) for v in o]
    return o


b = json.loads((QA.parent / "logs/status-before-reselect.json").read_text())
af = json.loads((QA.parent / "logs/status-after-reselect.json").read_text())
p(f"  both probes identical once observation timestamps are removed: {strip(b) == strip(af)}")

p()
p("### F4a :: icon vertical centring (live verdict set vs pre-fix)")
p("  see out/final_probe.txt for the image-derived row measurement")

(R / ".qa-t0c45ec21/out/final_consolidated.txt").write_text("\n".join(L) + "\n")
print("\n".join(L))
