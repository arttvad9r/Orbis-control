#!/usr/bin/env python3
"""Crop the page-header band (content area, below the 44px title bar) from each
c5ca-* capture so page identity can be read directly from the section title."""
import sys
from pathlib import Path

from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
OUT = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/probes/headers")
OUT.mkdir(parents=True, exist_ok=True)

names = sys.argv[1:] or [
    "c5ca-perf.png", "c5ca-performance-reselect.png", "c5ca-dashboard-early.png",
    "c5ca-after-nastroit.png", "c5ca-after-nastroit3.png", "c5ca-dashboard-bottom.png",
    "c5ca-dashboard-light.png", "c5ca-power.png", "c5ca-system.png",
    "c5ca-cooling-dirty.png", "c5ca-cooling-gpu-tab.png",
]
for n in names:
    im = Image.open(QA / n).convert("RGB")
    w, h = im.size
    im.crop((232, 44, min(w, 980), 130)).resize(
        (min(w, 980) - 232, (130 - 44))).save(OUT / n.replace(".png", "-hdr.png"))
    print(OUT / n.replace(".png", "-hdr.png"))
