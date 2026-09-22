#!/usr/bin/env python3
"""Final integrity check of the envelope + repo state facts."""
import json
import os
import subprocess

ROOT = "/home/artt/Orbis-control-implementation"
env = json.load(open(os.path.join(ROOT, ".int-smoke/envelope.json"), encoding="utf-8"))
stage = env["orchestration_result"]["stage_results"]["self_verification"]
print("json parses OK, checks:", len(stage["checks"]), "verdict:", stage["verdict"])
for c in stage["checks"]:
    n_exp, n_obs = len(c["expected"]), len(c["observed"])
    print(f"  {c['id']:14s} expected_len={n_exp:3d} observed_len={n_obs:4d} "
          f"ev={len(c['evidence_references'])}")
print("stage evidence files all exist:",
      all(os.path.exists(p) for p in stage["evidence"]))
print("check evidence files all exist:",
      all(os.path.exists(p) for c in stage["checks"] for p in c["evidence_references"]))
head = subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"],
                      capture_output=True, text=True).stdout.strip()
print("workspace HEAD:", head)
print("matches envelope candidate:", head == stage["candidate"]["git_sha"])
status = subprocess.run(["git", "-C", ROOT, "status", "--short"],
                        capture_output=True, text=True).stdout.strip()
print("tracked-file changes (expect none):", repr(status) if status else "CLEAN")
