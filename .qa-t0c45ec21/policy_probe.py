#!/usr/bin/env python3
"""Inspect the ACTIVE board-protocol orchestration policy: how it parses the
completion envelope, which stage a qa card requires, how it compares candidate
git_sha, and what it accepts for evidence paths. Read-only."""
import sys
sys.path.insert(0, "/home/artt/.hermes/plugins/board-protocol")
import orchestration as orch  # noqa: E402

print("module file:", orch.__file__)
names = [n for n in dir(orch) if not n.startswith("__")]
print("public names:", names)
