#!/usr/bin/env python3
"""Independent inventory of the QA shot sets + in-repo renders.

Prints: file, size, mean/std (blankness), and per-category grouping.
Read-only.
"""
from pathlib import Path
from PIL import Image
import numpy as np

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

print("=== QA shots ===")
for p in sorted(QA.glob("*.png")):
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.uint8)
    print("  %-34s %sx%s mean=%.1f std=%.1f uniq_colors=%d" % (
        p.name, a.shape[1], a.shape[0], a.mean(), a.std(),
        len(np.unique(a.reshape(-1, 3), axis=0))))

print()
print("=== in-repo reference renders ===")
for d in (".int-smoke/merged", ".int-smoke/base", "docs/ui-audit-baseline"):
    dd = ROOT / d
    if not dd.is_dir():
        print("  MISSING", d)
        continue
    fs = sorted(dd.glob("*.png"))
    print("  %s : %d files" % (d, len(fs)))
