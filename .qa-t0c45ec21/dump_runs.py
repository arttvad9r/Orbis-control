#!/usr/bin/env python3
"""Dump task_runs envelopes (metadata) for cards of interest, read-only."""
import json
import sqlite3
import sys

DB = "/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db"
db = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
db.row_factory = sqlite3.Row
ids = sys.argv[1:]
for tid in ids:
    rows = db.execute(
        "SELECT id, profile, status, outcome, summary, metadata, error, started_at, ended_at "
        "FROM task_runs WHERE task_id=? ORDER BY id", (tid,)).fetchall()
    print(f"\n===== {tid}: {len(rows)} runs =====")
    for r in rows:
        print(f"-- run {r['id']} profile={r['profile']} status={r['status']} outcome={r['outcome']} "
              f"started={r['started_at']} ended={r['ended_at']}")
        if r["error"]:
            print(f"   error={r['error'][:300]}")
        if r["summary"]:
            print(f"   summary={r['summary'][:400]}")
        md = r["metadata"]
        if md:
            try:
                parsed = json.loads(md)
            except Exception:
                parsed = md
            print("   metadata=", json.dumps(parsed, ensure_ascii=False, indent=2)[:4000])
