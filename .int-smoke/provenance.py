#!/usr/bin/env python3
"""Provenance check: baseline binary render == QA's own attached screenshot?"""
import sys

import numpy as np
from PIL import Image

a = np.asarray(Image.open("/tmp/qa-provenance-check.png").convert("RGB"))
b = np.asarray(Image.open(
    "/home/artt/Orbis-control-implementation/.worktrees/t_23cd8fb8/.qa3-shots/"
    "about-980x680-light.png").convert("RGB"))
identical = a.tobytes() == b.tobytes()
print(f"baseline-binary vs QA-attached-shoot: identical={identical} "
      f"shape={a.shape} vs {b.shape}")
sys.exit(0 if identical else 1)
