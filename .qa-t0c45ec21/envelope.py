#!/usr/bin/env python3
"""Emit the exact metadata envelope JSON for kanban_complete, built from the
card's own expected_observable strings (byte-exact, no transcription)."""
import json
import sqlite3
from datetime import datetime, timezone

DB = "file:/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db?mode=ro"
c = sqlite3.connect(DB, uri=True)
c.row_factory = sqlite3.Row
body = json.loads(c.execute(
    "select body from tasks where id='t_0c45ec21'").fetchone()["body"])
checks = {x["id"]: x["expected_observable"] for x in body["verification"]["checks"]}

SHA = "a21139e8f4591237c38075d54350e5bf7afc8974"
RUN = c.execute("select current_run_id from tasks where id='t_0c45ec21'").fetchone()[0]
EV = "/home/artt/Orbis-control-implementation/.qa-t0c45ec21"

env = {
    "orchestration_result": {
        "orchestration_contract_version": 1,
        "task_id": "t_0c45ec21",
        "run_id": RUN,
        "stage_results": {
            "qa": {
                "verdict": "FAIL",
                "candidate": {"git_sha": SHA},
                "checks": [
                    {
                        "id": "liveness-verdict",
                        "result": "PASS",
                        "expected": checks["liveness-verdict"],
                        "observed": (
                            "27 live captures c5ca-* (07:17-07:34, post-merge tree 5f0d3da) all decode at "
                            "980x680/1200x800, non-blank, correct page identity via sidebar selected-row fill "
                            "coverage + header band match against candidate-tree references (dashboard 0.99, "
                            "performance 0.72, power 0.67, about 0.76, settings 0.81). Navigation stable across "
                            "rapid repetition (c5ca-after-rapid-nav), back-navigation (c5ca-dashboard-top-again, "
                            "c5ca-settings-toggled-back) and state toggling; no crash/blank/tear. Verdict: PASS."
                        ),
                        "evidence_references": [
                            f"{EV}/out/selrow.txt",
                            f"{EV}/out/matchall.txt",
                            f"{EV}/out/navfull.txt",
                        ],
                    },
                    {
                        "id": "honesty-verdict",
                        "result": "PASS",
                        "expected": checks["honesty-verdict"],
                        "observed": (
                            "Read-only discipline held: logs/status-before-reselect.json and "
                            "status-after-reselect.json are byte-identical once observation timestamps are "
                            "stripped (only secs/nanos differ); no hardware mutation observed, "
                            "perf.current=balanced and charge_limit unchanged (enabled=false, configured="
                            "effective=100). gpu.power=unavailable because hardwared is not running, so the "
                            "privileged mutation path (SC-POWER-APPLY/SC-BOOST/SC-FAN-*/SC-GPU-MODE writes) is "
                            "NOT covered by this evidence set and is recorded INCONCLUSIVE with that cause, "
                            "not guessed. Verdict: PASS with declared INCONCLUSIVE."
                        ),
                        "evidence_references": [
                            f"{EV}/out/final_consolidated.txt",
                            "/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/logs/status-before-reselect.json",
                            "/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/logs/status-after-reselect.json",
                        ],
                    },
                    {
                        "id": "visual-verdict",
                        "result": "FAIL",
                        "expected": checks["visual-verdict"],
                        "observed": (
                            "Measurement performed as required. Nav icon left edge pooled: c5ca-* = [25,26,27] "
                            "spread 2 over 27 files/266 rows; control p980-* (pre-merge) = spread 47 -> F4b "
                            "conforms. F1 conforms (dashboard.slint:77-78 uses Grid.page-gutter=28px; live "
                            "content column x=(218,975) equals candidate-tree reference). F3 conforms "
                            "('width: parent.width' 2->0 occurrences, wrap: word-wrap retained). "
                            "BUT F2 / D-AUD-G02 does NOT conform to the recorded F1-F4b symmetry: the "
                            "Performance page still contains the unfixed 'visible:' card + trailing 'if' "
                            "fallback pair at ui/audited/sections/performance.slint:133 and :154 (identical at "
                            "860bc07, 2c3e3af, f45f55d, 5ca36ab, 5f0d3da and the pinned candidate a21139e8; "
                            "never committed as a conditional pair on any ref). Live reproduction in the "
                            "verdict set: c5ca-performance-reselect.png (Performance page, row "
                            "'Производительность' selected, fill 0.72) has ink bands "
                            "[(17,26),(61,80),(89,101),(120,298),(471,525)] -> a 172px empty band between the "
                            "profile card (ends y=298) and the 55px fallback limits card (y=471..525); the "
                            "band body y=302..466 x=222..959 carries only 1.351% ink, entirely the card's left "
                            "border column x=220..229, i.e. empty page background. The same 172px band is "
                            "present on the pre-merge control p980-performance.png and the pre-fix baseline, "
                            "while the post-fix candidate reference merged/performance-980x680-dark.png has no "
                            "band (limits card y=312..627, 316px). The commit that claims F2 (2c3e3af) only "
                            "changed the harness demo data (crates/orbis-ui/examples/ui_snapshot.rs:113 "
                            "power_limits_ready=true), masking the defect in offscreen renders instead of "
                            "fixing the UI; the live app runs against the real backend where hardwared is "
                            "absent, so power-limits-ready=false and the band is user-visible. Verdict: FAIL."
                        ),
                        "evidence_references": [
                            f"{EV}/VERDICT-t_0c45ec21.md",
                            f"{EV}/out/perf_profile.txt",
                            f"{EV}/out/f2proof.txt",
                            f"{EV}/out/final_probe.txt",
                            f"{EV}/out/srcev.txt",
                            f"{EV}/out/checks_emit.txt",
                            f"{EV}/out/f1f3final.txt",
                            f"{EV}/out/selrow.txt",
                        ],
                    },
                ],
                "evidence": [
                    f"{EV}/VERDICT-t_0c45ec21.md",
                    f"{EV}/out/perf_profile.txt",
                    f"{EV}/out/f2proof.txt",
                    f"{EV}/out/final_probe.txt",
                    f"{EV}/out/srcev.txt",
                    f"{EV}/out/checks_emit.txt",
                    f"{EV}/out/navfull.txt",
                    f"{EV}/out/selrow.txt",
                    f"{EV}/out/matchall.txt",
                    f"{EV}/out/final_consolidated.txt",
                    f"{EV}/out/f1f3final.txt",
                ],
                "checker": "qa",
                "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            }
        },
    }
}
print(json.dumps(env, ensure_ascii=False, indent=1))
