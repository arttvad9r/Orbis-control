#!/usr/bin/env python3
"""Print title + body of one task, read-only."""
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
tid = sys.argv[1]
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()
cur.execute("select id,title,body,assignee,status,current_run_id from tasks where id=?", (tid,))
for r in cur.fetchall():
    print("TITLE:", r["title"])
    print("ASSIGNEE:", r["assignee"], "STATUS:", r["status"], "RUN:", r["current_run_id"])
    print("BODY:")
    print(r["body"])
