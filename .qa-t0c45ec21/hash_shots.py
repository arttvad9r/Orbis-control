#!/usr/bin/env python3
"""Hash every QA artifact; find duplicate/identical pairs; flag no-op captures."""
import hashlib
from pathlib import Path

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")

def h(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

files = sorted(QA.glob("*.png"))
print("== sha256 of QA shots ==")
seen = {}
for p in files:
    d = h(p)
    seen.setdefault(d, []).append(p.name)
    print(f"  {d[:16]}  {p.stat().st_size:>7}  {p.name}")

print("\n== identical-content groups ==")
for d, names in sorted(seen.items(), key=lambda kv: kv[1][0]):
    if len(names) > 1:
        print(f"  {d[:16]}: {names}")

print("\n== same-size groups (candidate no-op pairs) ==")
bysize = {}
for p in files:
    bysize.setdefault(p.stat().st_size, []).append(p.name)
for s, names in sorted(bysize.items()):
    if len(names) > 1:
        print(f"  {s}: {names}")

print("\n== key pairs ==")
pairs = [
    ("c5ca-graphics.png", "c5ca-graphics-after-click.png"),
    ("c5ca-cooling.png", "c5ca-cooling-gpu-tab.png"),
    ("c5ca-cooling-gpu-tab.png", "c5ca-cooling-gpu-discarded.png"),
    ("c5ca-dashboard-bottom.png", "c5ca-dashboard-bottom2.png"),
    ("c5ca-settings-toggled.png", "c5ca-settings-toggled-back.png"),
    ("c5ca-about.png", "p980-about.png"),
    ("c5ca-cooling.png", "p980-cooling.png"),
]
for a, b in pairs:
    pa, pb = QA / a, QA / b
    if pa.exists() and pb.exists():
        print(f"  {a} vs {b}: identical={h(pa) == h(pb)} "
              f"a_size={pa.stat().st_size} b_size={pb.stat().st_size}")
