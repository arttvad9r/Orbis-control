#!/usr/bin/env python3
"""Band strip detail + collect F2 source evidence into one report."""
import subprocess
from pathlib import Path

import numpy as np
from PIL import Image

R = Path("/home/artt/Orbis-control-implementation")
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
OUT = R / ".qa-t0c45ec21/out"

lines = []
lines.append("### F2 (D-AUD-G02) source at every relevant revision")
lines.append("")
for r in ["860bc07", "2c3e3af", "f45f55d", "5ca36ab", "5f0d3da", "a21139e8"]:
    src = subprocess.run(["git", "-C", str(R), "show", f"{r}:ui/audited/sections/performance.slint"],
                         capture_output=True, text=True).stdout
    hits = [f"line {i+1}: {l.strip()}" for i, l in enumerate(src.splitlines())
            if "power-limits-ready" in l]
    lines.append(f"--- {r} ---")
    lines += [f"    {h}" for h in hits]
lines.append("")

info = subprocess.run(["git", "-C", str(R), "log", "-1", "--format=%H%n%s%n%b", "2c3e3af"],
                      capture_output=True, text=True).stdout
lines.append("### commit that CLAIMS to fix F1-F4b (self-report)")
lines.append(info.strip())
lines.append("")

ev = R / ".worktrees/t_29073c31/docs/ui-fix-evidence.txt"
lines.append("### UI line self-verification evidence")
lines.append(ev.read_text().strip() if ev.exists() else "(absent)")
lines.append("")

lines.append("### pixel strip across the card's left border at y=400")
for lbl, p in [("LIVE  c5ca-performance-reselect", QA / "c5ca-performance-reselect.png"),
               ("PREFIX  baseline/performance", R / "docs/ui-audit-baseline/performance-980x680-dark.png"),
               ("POSTFIX merged/performance", R / ".int-smoke/merged/performance-980x680-dark.png")]:
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)
    strip = [tuple(int(v) for v in c) for c in a[400, 214:238]]
    lines.append(f"  {lbl}:")
    lines.append(f"      {strip}")

(OUT / "f2_source_evidence.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
