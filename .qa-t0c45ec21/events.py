#!/usr/bin/env python3
"""Dump task_events for given task ids (read-only)."""
import json
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()
for tid in sys.argv[1:]:
    print("=== events for %s ===" % tid)
    cur.execute("select id,run_id,kind,payload,created_at from task_events where task_id=? order by id", (tid,))
    for r in cur.fetchall():
        print("  #%s run=%s %s @%s" % (r["id"], r["run_id"], r["kind"], r["created_at"]))
        if r["payload"]:
            try:
                print("     %s" % json.dumps(json.loads(r["payload"]), ensure_ascii=False)[:900])
            except Exception:
                print("     %s" % r["payload"][:900])
