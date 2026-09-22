#!/usr/bin/env python3
"""Probe the board-protocol policy for THIS board's pinned standard and rules."""
import sys

sys.path.insert(0, "/home/artt/.hermes/plugins/board-protocol")
import orchestration as o  # noqa: E402

print("== board_context_rules(DESKTOP, EXISTING) ==")
print(o.board_context_rules("DESKTOP", "EXISTING", ""))
print()
print("== EXEMPT_TASK_CLASSES ==", sorted(o.EXEMPT_TASK_CLASSES))
print("== UI_GEOMETRY_CHECKERS ==", sorted(o.UI_GEOMETRY_CHECKERS))
print()
print("== active standard id ==", o._active_standard_id())
print("== domain for desktop ==", o._domain("desktop"), o._domain("desktop").__name__)
print()
print("== available domain drivers ==")
for d in o._installed_drivers():
    print("   ", getattr(d, "STANDARD_ID", d))
print()
print("== visual_measurement_errors('visual-verdict', [c5ca-shot]) ==")
print(o._domain("desktop").visual_measurement_errors(
    "visual-verdict",
    ["/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots/p980-about.png"]))
