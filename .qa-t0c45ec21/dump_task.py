#!/usr/bin/env python3
"""Dump one task body to a file (read-only DB access)."""
import json
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
tid = sys.argv[1]
out = sys.argv[2]
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()
cur.execute("select id,title,body,assignee,status,created_at,completed_at,result from tasks where id=?", (tid,))
r = cur.fetchone()
with open(out, "w", encoding="utf-8") as fh:
    fh.write("TITLE: %s\n" % r["title"])
    fh.write("ASSIGNEE: %s STATUS: %s\n" % (r["assignee"], r["status"]))
    fh.write("RESULT: %s\n" % (r["result"],))
    fh.write("BODY:\n")
    try:
        fh.write(json.dumps(json.loads(r["body"]), ensure_ascii=False, indent=2))
    except Exception:
        fh.write(r["body"])
    fh.write("\n")
print("wrote", out, "chars", len(r["body"]))
