#!/usr/bin/env python3
import json

d = json.load(open("/home/artt/Orbis-control-implementation/.qa-t0c45ec21/out/envelope.json"))
r = d["orchestration_result"]
print("version:", r["orchestration_contract_version"], "task:", r["task_id"], "run:", r["run_id"])
s = r["stage_results"]["qa"]
print("verdict:", s["verdict"], "sha:", s["candidate"]["git_sha"])
for c in s["checks"]:
    print("  check", c["id"], c["result"], "evrefs:", len(c["evidence_references"]))
print("evidence:", len(s["evidence"]))
print("checker:", s["checker"], "ts:", s["timestamp"])
