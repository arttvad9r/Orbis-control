#!/usr/bin/env python3
"""Print comments of a task, read-only."""
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
tid = sys.argv[1]
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()
cur.execute("select id,author,body,created_at from task_comments where task_id=? order by id", (tid,))
for r in cur.fetchall():
    print("=== comment %s by %s @%s ===" % (r["id"], r["author"], r["created_at"]))
    print(r["body"])
    print()
