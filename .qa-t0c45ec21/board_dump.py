#!/usr/bin/env python3
"""Read-only dump of the orbis-v01 board for QA orientation (t_0c45ec21)."""
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
db = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()
cur.execute("select name from sqlite_master where type='table'")
tables = [r[0] for r in cur.fetchall()]
print("TABLES:", tables)
for t in tables:
    cur.execute("pragma table_info(%s)" % t)
    cols = [r[1] for r in cur.fetchall()]
    print("  %s: %s" % (t, cols))

print("=== tasks ===")
cur.execute("select * from tasks order by created_at")
rows = cur.fetchall()
for r in rows:
    d = dict(r)
    print("%s | %s | %s | %s | created=%s completed=%s run=%s" % (
        d.get("id"), d.get("status"), d.get("assignee"),
        str(d.get("title"))[:75], d.get("created_at"),
        d.get("completed_at"), d.get("current_run_id")))

if len(sys.argv) > 1:
    tid = sys.argv[1]
    cur.execute("select * from tasks where id=?", (tid,))
    for r in cur.fetchall():
        d = dict(r)
        print("=== FULL TASK %s ===" % tid)
        for k, v in d.items():
            print("--- %s:" % k)
            print(v)
