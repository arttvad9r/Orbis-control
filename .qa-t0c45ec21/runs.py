#!/usr/bin/env python3
"""Read task_runs result/metadata for a task (verdict archaeology)."""
import json
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
tid = sys.argv[1]
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cols = [r[1] for r in db.execute("pragma table_info(task_runs)")]
print("task_runs columns:", cols)
for r in db.execute("select * from task_runs where task_id=? order by id", (tid,)):
    d = dict(r)
    print("=" * 70)
    for k in ("id", "status", "outcome", "summary", "result", "started_at", "finished_at"):
        if k in d:
            print(f"  {k}: {d[k]}")
    for k in ("metadata", "result_json", "handoff"):
        if k in d and d[k]:
            print(f"  -- {k} --")
            try:
                print(json.dumps(json.loads(d[k]), ensure_ascii=False, indent=2)[:4000])
            except Exception:
                print(str(d[k])[:4000])
