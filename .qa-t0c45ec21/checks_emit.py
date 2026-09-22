#!/usr/bin/env python3
"""Emit the exact planner check ids + expected_observable strings (verbatim) and
one clean pooled F4b measurement with a theme-safe sidebar background sample.
"""
import json
import sqlite3
from pathlib import Path

import numpy as np
from PIL import Image

DB = "file:/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db?mode=ro"
QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")

con = sqlite3.connect(DB, uri=True)
con.row_factory = sqlite3.Row
row = con.execute("select body from tasks where id=?", ("t_0c45ec21",)).fetchone()
card = json.loads(row["body"])
checks = card["verification"]["checks"]
print("### planner checks (verbatim)")
print(json.dumps(checks, ensure_ascii=False, indent=2))
(Path("/home/artt/Orbis-control-implementation/.qa-t0c45ec21/out/checks.json")).write_text(
    json.dumps(checks, ensure_ascii=False, indent=2))

print()
print("### clean pooled F4b: nav icon left edge (theme-safe bg sample)")
ROWS = [("Главная", 56, 100), ("Производительность", 100, 144), ("Питание", 144, 188),
        ("Охлаждение", 188, 232), ("Графика", 232, 276), ("Подсветка", 276, 320),
        ("Экран", 320, 364), ("Система", 364, 408), ("Настройки", 570, 614),
        ("О программе", 614, 658)]


def nav_lefts(path):
    a = np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)
    bg = np.median(a[430:470, 120:200].reshape(-1, 3), axis=0)
    out = []
    for name, y0, y1 in ROWS:
        band = a[y0:y1, 4:228, :]
        d = np.abs(band - bg).sum(axis=2)
        fg = d > 60
        cc = fg.sum(axis=0)
        fill = cc > (y1 - y0) * 0.75
        cols = np.where(fg.any(axis=0) & (~fill))[0]
        out.append(int(cols[0]) + 4 if len(cols) else None)
    return out


for label, pat in [("c5ca-* (VERDICT SET, post-merge)", "c5ca-*.png"),
                   ("p980-* (CONTROL, pre-merge)", "p980-*.png"),
                   ("merged/* (candidate tree offscreen)", None)]:
    pool = []
    if pat:
        files = sorted(QA.glob(pat))
    else:
        files = sorted((Path("/home/artt/Orbis-control-implementation/.int-smoke/merged")).glob("*-980x680-dark.png"))
    per_shot = []
    for f in files:
        v = [x for x in nav_lefts(f) if x is not None]
        if v:
            per_shot.append((f.name, sorted(set(v)), max(v) - min(v)))
            pool += v
    print(f"  {label}: files={len(files)} rows_measured={len(pool)}")
    if pool:
        print(f"      pooled distinct={sorted(set(pool))}  spread={max(pool) - min(pool)}")
    worst = sorted(per_shot, key=lambda t: -t[2])[:3]
    print(f"      per-shot worst spreads: {worst}")
