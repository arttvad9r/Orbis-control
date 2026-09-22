#!/usr/bin/env python3
"""Extract the exact card contract for a task: body + orchestration_result."""
import json
import sqlite3
import sys

DB = "file:/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db?mode=ro"
tid = sys.argv[1]
con = sqlite3.connect(DB, uri=True)
con.row_factory = sqlite3.Row
cur = con.cursor()
cur.execute("select * from tasks where id=?", (tid,))
r = cur.fetchone()
if not r:
    print("no such task")
    sys.exit(1)
keys = r.keys()
for k in keys:
    v = r[k]
    if k in ("body", "result", "metadata"):
        continue
    print(f"{k} = {v!r}")
print()
print("########## BODY ##########")
print(r["body"] or "(empty)")
print()
print("########## RESULT ##########")
print(r["result"] or "(empty)")
print()
print("########## METADATA ##########")
try:
    md = json.loads(r["metadata"]) if r["metadata"] else {}
except Exception:
    md = {"__raw": r["metadata"]}
print(json.dumps(md, ensure_ascii=False, indent=2)[:20000])
