#!/usr/bin/env python3
"""Check which image probe tools are available."""
import importlib.util
import shutil

for mod in ("PIL", "numpy"):
    spec = importlib.util.find_spec(mod)
    print(f"{mod}: {'OK' if spec else 'MISSING'}")
for tool in ("identify", "compare", "magick"):
    print(f"{tool}: {shutil.which(tool) or 'MISSING'}")
