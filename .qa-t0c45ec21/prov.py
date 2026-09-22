#!/usr/bin/env python3
"""Provenance: who produced the c5ca-* shots, and epoch<->local conversion."""
import sqlite3
from datetime import datetime, timezone, timedelta

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()

MSK = timezone(timedelta(hours=3))


def t(epoch):
    return datetime.fromtimestamp(epoch, MSK).strftime("%Y-%m-%d %H:%M:%S MSK")


print("=== tasks whose id contains t5b9d96c6 or t_0c45ec21 ===")
for r in cur.execute("select id,title,assignee,status,created_at from tasks where id like '%5b9d96c6%' or id like '%0c45ec21%'"):
    print(" ", r["id"], "|", r["assignee"], "|", r["status"], "|", t(r["created_at"]))
    print("    title:", r["title"])

print()
print("=== all tasks created/modified around the c5ca window ===")
for r in cur.execute("select id,title,assignee,status,created_at from tasks order by created_at"):
    print("  %-14s %-8s %-8s %s  %s" % (r["id"], r["assignee"], r["status"], t(r["created_at"]), r["title"][:70]))

print()
print("=== key epochs ===")
for label, e in [("card t_0c45ec21 created", 1790052337),
                 ("run33 claimed", 1790052352),
                 ("run33 blocked", 1790053353),
                 ("planner comment", 1790053501),
                 ("run34 claimed", 1790053502)]:
    print("  %-28s %s" % (label, t(e)))
