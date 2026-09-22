#!/usr/bin/env python3
"""Full dump of one run's metadata JSON (read-only)."""
import json
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
db = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
db.row_factory = sqlite3.Row
tid, rid = sys.argv[1], int(sys.argv[2])
row = db.execute("SELECT metadata, summary FROM task_runs WHERE task_id=? AND id=?",
                 (tid, rid)).fetchone()
md = json.loads(row["metadata"]) if row["metadata"] else {}
print("TOP-LEVEL KEYS:", sorted(md.keys()))
print()
for k, v in md.items():
    if k == "orchestration_result":
        continue
    print(f"--- {k} ---")
    print(json.dumps(v, ensure_ascii=False, indent=2)[:2000])
print()
print("--- orchestration_result.stage_results keys ---")
print(sorted(md.get("orchestration_result", {}).get("stage_results", {}).keys()))
print()
for stage, body in md.get("orchestration_result", {}).get("stage_results", {}).items():
    print(f"=== stage={stage} verdict={body.get('verdict')} candidate={body.get('candidate')} "
          f"checker={body.get('checker')} timestamp={body.get('timestamp')}")
    for c in body.get("checks", []):
        print(f"   check id={c.get('id')} result={c.get('result')}")
        print(f"     expected[:90]={str(c.get('expected'))[:90]!r}")
        print(f"     observed[:110]={str(c.get('observed'))[:110]!r}")
