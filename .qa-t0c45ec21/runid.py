#!/usr/bin/env python3
import sqlite3

c = sqlite3.connect("file:/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db?mode=ro", uri=True)
c.row_factory = sqlite3.Row
cols = [r[1] for r in c.execute("pragma table_info(task_runs)")]
print("task_runs cols:", cols)
for r in c.execute("select * from task_runs where task_id='t_0c45ec21' order by id desc limit 5"):
    d = dict(r)
    keep = {k: v for k, v in d.items() if k in ("id", "task_id", "status", "started_at", "finished_at", "profile")}
    print(keep)
print("current_run_id:", c.execute("select current_run_id from tasks where id='t_0c45ec21'").fetchone()[0])
