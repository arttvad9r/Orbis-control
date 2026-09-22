#!/usr/bin/env python3
import json
from pathlib import Path

d = json.load(open("/home/artt/Orbis-control-implementation/.qa-t0c45ec21/out/envelope.json"))
s = d["orchestration_result"]["stage_results"]["qa"]
paths = list(s["evidence"]) + [p for c in s["checks"] for p in c["evidence_references"]]
bad = [p for p in set(paths) if not Path(p).exists()]
print("total refs:", len(set(paths)), "missing:", len(bad))
for p in bad:
    print("  MISSING", p)
