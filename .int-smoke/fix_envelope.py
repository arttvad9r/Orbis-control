#!/usr/bin/env python3
"""Fix the completion envelope for t_3864d46e.

- Copy each check's expected_observable verbatim from the card JSON.
- Point evidence at files that exist.
- Keep the card-pinned candidate git_sha and the candidate_note.
"""
import json
import os

ROOT = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(ROOT, "card.json"), encoding="utf-8") as fh:
    card = json.load(fh)
body = json.loads(card["task"]["body"])
checks = body["verification"]["checks"]
contract = {c["id"]: c["expected_observable"] for c in checks}
print("card check ids:", sorted(contract))

with open(os.path.join(ROOT, "envelope.json"), encoding="utf-8") as fh:
    env = json.load(fh)

stage = env["orchestration_result"]["stage_results"]["self_verification"]

renames = {
    "merge": "merge",
    "build-merged": "build-merged",
    "verify-merged": "verify-merged",
    "smoke-merged": "smoke-merged",
}
for chk in stage["checks"]:
    cid = chk["id"]
    chk["expected"] = contract[cid]

# evidence fixes: the red-error render is the cooling one (exists on disk)
smoke = next(c for c in stage["checks"] if c["id"] == "smoke-merged")
smoke["evidence_references"] = [
    "/home/artt/Orbis-control-implementation/.int-smoke/smoke_probes.log",
    "/home/artt/Orbis-control-implementation/.int-smoke/merge_identity.md",
    "/home/artt/Orbis-control-implementation/.int-smoke/merged/dashboard-980x680-dark.png",
    "/home/artt/Orbis-control-implementation/.int-smoke/merged/cooling-980x680-dark-error.png",
]
smoke["observed"] = smoke["observed"].replace(
    "the merged performance power-limit-error surface renders with red error text (156 red pixels)",
    "the merged cooling fan-curve invalid-draft error surface renders with red error text (156 red pixels)")

stage["evidence"] = [
    "/home/artt/Orbis-control-implementation/.int-smoke/merge_identity.md",
    "/home/artt/Orbis-control-implementation/.int-smoke/verify.log",
    "/home/artt/Orbis-control-implementation/.int-smoke/smoke_probes.log",
    "/home/artt/Orbis-control-implementation/.int-smoke/merged/dashboard-980x680-dark.png",
    "/home/artt/Orbis-control-implementation/.int-smoke/merged/cooling-980x680-dark-error.png",
]

out = os.path.join(ROOT, "envelope.json")
with open(out, "w", encoding="utf-8") as fh:
    json.dump(env, fh, ensure_ascii=False, indent=2)
print("envelope rewritten:", out)
