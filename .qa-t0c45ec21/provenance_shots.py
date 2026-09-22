#!/usr/bin/env python3
"""Provenance of QA screenshots: hashes, duplicates, and PNG encoder chunks.

Different producers leave different PNG chunk fingerprints (IHDR/SRGB/gAMA/IDAT
sizes and any tEXt/iTXt). Comparing QA 'live' shots against the offscreen
ui_snapshot renders and the UI-QA2 shots tells us who actually wrote them.
"""
import hashlib
import struct
from collections import defaultdict
from pathlib import Path

ROOT = Path("/home/artt/Orbis-control-implementation")
GROUPS = {
    "QA-t5b9d96c6": Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots"),
    "INT-merged": ROOT / ".int-smoke/merged",
    "INT-base": ROOT / ".int-smoke/base",
    "UIQA2-qa3": ROOT / ".worktrees/t_23cd8fb8/.qa3-shots",
    "audit-baseline": ROOT / "docs/ui-audit-baseline",
}


def chunks(path):
    out = []
    with open(path, "rb") as fh:
        assert fh.read(8) == b"\x89PNG\r\n\x1a\n", path
        while True:
            head = fh.read(8)
            if len(head) < 8:
                break
            length, ctype = struct.unpack(">I4s", head)
            data = fh.read(length)
            fh.read(4)
            name = ctype.decode("latin1")
            if name in ("tEXt", "iTXt", "zTXt", "tIME"):
                out.append(f"{name}:{data[:60]!r}")
            else:
                out.append(name)
    return out


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


print("== per-group fingerprints ==")
sigs = {}
for label, d in GROUPS.items():
    print(f"\n-- {label} ({d}) --")
    if not d.exists():
        print("   MISSING")
        continue
    seen = {}
    for p in sorted(d.glob("*.png")):
        h = sha(p)
        sig = "|".join(chunks(p))
        seen.setdefault(sig, []).append(p.name)
        print(f"   {h[:16]}  {p.name:44s} {sig[:110]}")
    sigs[label] = seen
    if len(seen) > 1:
        print("   >> distinct PNG chunk signatures in this group:")
        for sig, names in seen.items():
            print(f"      [{sig[:70]}] x{len(names)}: {names[:4]}")

print()
print("== duplicate content inside QA group ==")
qa = GROUPS["QA-t5b9d96c6"]
by_hash = defaultdict(list)
for p in sorted(qa.glob("*.png")):
    by_hash[sha(p)].append(p.name)
for h, names in sorted(by_hash.items(), key=lambda kv: -len(kv[1])):
    if len(names) > 1:
        print(f"   {h[:16]}  {names}")
print("   total pngs:", sum(len(v) for v in by_hash.values()),
      " distinct:", len(by_hash))

print()
print("== cross-group identical files ==")
allh = {}
for label, d in GROUPS.items():
    if not d.exists():
        continue
    for p in sorted(d.glob("*.png")):
        allh.setdefault(sha(p), []).append(f"{label}/{p.name}")
for h, names in sorted(allh.items()):
    if len(names) > 1:
        print(f"   {h[:16]}  {names}")
