#!/usr/bin/env python3
"""Read-only: links, runs metadata, attachments, comments for the orbis-v01 board."""
import json
import sqlite3

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
db = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
db.row_factory = sqlite3.Row
cur = db.cursor()

print("=== task_links ===")
cur.execute("select parent_id, child_id from task_links")
for r in cur.fetchall():
    print("  %s -> %s" % (r["parent_id"], r["child_id"]))

print("=== attachments ===")
cur.execute("select id,task_id,filename,stored_path,content_type,size,uploaded_by,created_at from task_attachments")
for r in cur.fetchall():
    print("  %s | %s | %s | %s | %s | %s | %s" % (
        r["id"], r["task_id"], r["filename"], r["stored_path"], r["content_type"], r["size"], r["created_at"]))

print("=== comments ===")
cur.execute("select task_id,author,created_at,body from task_comments order by created_at")
for r in cur.fetchall():
    print("  --- %s by %s at %s:" % (r["task_id"], r["author"], r["created_at"]))
    print(r["body"][:4000])

print("=== runs (metadata/summary) ===")
cur.execute("select id,task_id,profile,status,started_at,ended_at,outcome,summary,metadata,error from task_runs order by id")
for r in cur.fetchall():
    print("  --- run %s task=%s profile=%s status=%s outcome=%s" % (
        r["id"], r["task_id"], r["profile"], r["status"], r["outcome"]))
    if r["summary"]:
        print("      summary: %s" % r["summary"][:600])
    if r["metadata"]:
        try:
            md = json.loads(r["metadata"])
            print("      metadata keys: %s" % list(md.keys()))
            print("      metadata: %s" % json.dumps(md, ensure_ascii=False)[:3000])
        except Exception as exc:
            print("      metadata raw: %s (%s)" % (r["metadata"][:600], exc))
    if r["error"]:
        print("      error: %s" % r["error"][:300])
