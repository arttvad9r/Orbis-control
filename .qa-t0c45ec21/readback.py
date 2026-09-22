#!/usr/bin/env python3
import json
import sqlite3

c = sqlite3.connect("file:/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db?mode=ro", uri=True)
c.row_factory = sqlite3.Row
t = c.execute("select status, completed_at, current_run_id from tasks where id='t_0c45ec21'").fetchone()
print("task status:", t["status"], "completed_at:", t["completed_at"], "run:", t["current_run_id"])
r = c.execute("select status, outcome, summary, metadata from task_runs where id=34").fetchone()
print("run status:", r["status"], "outcome:", r["outcome"])
md = json.loads(r["metadata"]) if r["metadata"] else {}
env = md.get("orchestration_result", {})
print("envelope at metadata root?:", "orchestration_result" in md)
print("nested version:", env.get("orchestration_contract_version"), "task:", env.get("task_id"), "run:", env.get("run_id"))
sr = env.get("stage_results", {})
print("stage keys:", list(sr))
qa = sr.get("qa", {})
print("verdict:", qa.get("verdict"), "sha:", qa.get("candidate", {}).get("git_sha"))
for ch in qa.get("checks", []):
    print("  check:", ch["id"], ch["result"], "| expected starts:", ch["expected"][:40])
